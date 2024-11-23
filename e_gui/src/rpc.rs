use crate::*;
use std::sync::Arc;

use anyhow::{Ok, Result};
use common::{
    rpc::{Dispatcher, FrameChan, TopicFallback},
    LinkFrame,
};
use futures::{future::select, lock::Mutex};
use ppproto::{
    pppos::{PPPoS, PPPoSAction},
    Config,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, oneshot},
};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

pub struct RPCHost<'p, H: TopicFallback<FrameCh>> {
    proto: PPPoS<'p>,
    pub disp: Dispatcher<H, FrameCh>,
    port: SerialStream,
}

#[derive(Clone, Debug)]
pub struct FrameCh(Arc<Mutex<(mpsc::Sender<LinkFrame>, mpsc::Receiver<LinkFrame>)>>);

impl FrameChan for FrameCh {
    fn new() -> Self {
        Self(Arc::new(mpsc::channel(4).into()))
    }
    async fn recv_frame(&mut self) -> LinkFrame {
        let mut k = self.0.lock().await;
        k.1.recv().await.unwrap()
    }
    async fn send_frame(&mut self, f: LinkFrame) {
        let k = self.0.lock().await;
        k.0.send(f).await.unwrap()
    }
}

#[derive(new)]
pub struct NewTopic {
    start_frame: LinkFrame,
    call_back: oneshot::Sender<FrameCh>,
}

pub trait RPCSending {
    async fn call(&self, start: LinkFrame) -> Result<FrameCh>;
}

impl RPCSending for mpsc::Sender<NewTopic> {
    async fn call(&self, start: LinkFrame) -> Result<FrameCh> {
        let (sx, rx) = oneshot::channel();

        let topic = NewTopic {
            start_frame: start,
            call_back: sx,
        };
        self.send(topic).await?;

        Ok(rx.await?)
    }
}

impl<'p, H: TopicFallback<FrameCh>> RPCHost<'p, H> {
    pub fn new(path: &str, handler: H) -> Result<Self> {
        let disp = Dispatcher::new(handler);
        let port = tokio_serial::new(path, 9600).open_native_async()?;
        let mut proto = ppproto::pppos::PPPoS::new(Config {
            password: b"p",
            username: b"u",
        });
        proto.open().unwrap();

        Ok(RPCHost { proto, disp, port })
    }
    pub async fn run(mut self, mut sending: mpsc::Receiver<NewTopic>) -> Result<()> {
        let mut read_buf = [0; 4096];
        let mut rbuf = [0; 4096];
        let mut tbuf = [0; 4096];
        let mut t_start = 0;
        loop {
            let mut size = 0;
            let mut consumed = 0;

            println!("wait");
            tokio::select! {
                s = self.port.read(&mut read_buf) => {
                    size = s?;
                    consumed = 0;
                },
                to_send = sending.recv() => {
                    println!("send");
                    if let Some(mut to_send) = to_send {
                        let c = self.disp.make(&mut to_send.start_frame).await;
                        to_send.call_back.send(c).unwrap();
                        let bytes  = postcard::to_stdvec(&to_send.start_frame)?;
                        self.proto.send(&bytes, &mut tbuf[t_start..]).unwrap();
                    }

                }
            };
            loop {
                if (&read_buf[consumed..size]).len() > 0 {
                    consumed += self.proto.consume(&read_buf[consumed..size], &mut rbuf);
                }
                let p = self.proto.poll(&mut tbuf, &mut rbuf);
                match p {
                    PPPoSAction::Received(frame) => {
                        let f = &rbuf[frame];
                        let obj: LinkFrame = postcard::from_bytes(f)?;
                        self.disp.process(obj).await;
                    }
                    PPPoSAction::Transmit(t) => {
                        dbg!(&tbuf[..t]);
                        self.port.write_all(&tbuf[..t]).await?;
                        t_start += t;
                        println!("tx {} {:?}", t, self.proto.status());
                    }
                    PPPoSAction::None => {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct HostHandler;

impl TopicFallback<FrameCh> for HostHandler {
    async fn handle_new_topic(&self, f: LinkFrame, rx: FrameCh) {}
}
