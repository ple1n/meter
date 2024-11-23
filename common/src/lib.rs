#![no_std]
#![allow(async_fn_in_trait)]

pub use heapless;
use serde::{Deserialize, Serialize};


// pub use portable_atomic;
// pub use portable_atomic_util;

pub const VID: u16 = 0xb1e1;

pub const PID: u16 = 1;

#[derive(Serialize, Deserialize, defmt::Format)]
pub struct LinkFrame {
    pub topic: Option<u32>,
    pub verb: LinkVerb,
}

pub type Dispatch = u32;

#[derive(Serialize, Deserialize, defmt::Format)]
pub enum LinkVerb {
    Echo(heapless::String<24>),
}

impl LinkFrame {
    pub fn reply(&self, verb: LinkVerb) -> Self {
        LinkFrame {
            topic: self.topic.clone(),
            verb,
        }
    }
    pub fn new(verb: LinkVerb) -> Self {
        Self { topic: None, verb }
    }
}

pub mod rpc;
