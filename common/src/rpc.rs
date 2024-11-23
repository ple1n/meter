use crate::*;
use heapless::{FnvIndexMap, IndexMap, LinearMap};
use serde;
use ppproto::pppos::PPPoS;

use scapegoat::SgMap;
pub struct Dispatcher<T: TopicFallback<C>, C: FrameChan> {
    table: SgMap<Dispatch, Option<C>, 32>,
    handler: T,
}

pub trait TopicFallback<C: FrameChan> {
    async fn handle_new_topic(&self, f: LinkFrame, rx: C);
}

pub trait FrameChan: Clone + Send {
    fn new() -> Self;
    async fn send_frame(&mut self, f: LinkFrame);
    async fn recv_frame(&mut self) -> LinkFrame;
}

impl<C: FrameChan, T: TopicFallback<C>> Dispatcher<T, C> {
    pub fn new(handler: T) -> Self {
        Self {
            handler,
            table: SgMap::new(),
        }
    }
    pub async fn process(&mut self, f: LinkFrame) {
        if let Some(t) = f.topic {
            match self.table.get_mut(&t) {
                Some(sx) => {
                    sx.as_mut().unwrap().send_frame(f).await;
                }
                None => {
                    let c = C::new();
                    self.table.insert(t, Some(c.clone()));
                    self.handler.handle_new_topic(f, c).await;
                }
            }
        }

    }
    /// must be done pre send, returns a channel
    pub async fn make(&mut self, packet: &mut LinkFrame) -> C {
        let k = if let Some(en) = self.table.last_entry() {
            // largest + 1
            *en.key() + 1
        } else {
            0
        };
        packet.topic = Some(k);
        let c = C::new();
        self.table.insert(k, Some(c.clone()));
        c
    }
}
