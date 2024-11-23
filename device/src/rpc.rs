use common::{rpc::FrameChan, LinkFrame};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{self, *},
};

#[derive(Clone, Copy)]
pub struct FrameChannel(&'static Channel<CriticalSectionRawMutex, LinkFrame, 4>);

impl FrameChan for FrameChannel {
    fn new() -> Self {
        static CHAN: Channel<CriticalSectionRawMutex, LinkFrame, 4> = channel::Channel::new();
        let c = &CHAN;
        Self(c)
    }
    async fn send_frame(&mut self, f: common::LinkFrame) {
        self.0.send(f).await;
    }
    async fn recv_frame(&mut self) -> LinkFrame {
        self.0.receive().await
    }
}
