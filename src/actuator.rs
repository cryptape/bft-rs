use crate::*;
use crate::{
    algorithm::{Bft, INIT_HEIGHT},
    error::BftError,
};

use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};

/// Results of Bft actuator.
pub type Result<T> = ::std::result::Result<T, BftError>;

/// A Bft Actuator
pub struct BftActuator<T> {
    recv_msg: T,
    sender: Sender<BftMsg>,
    receiver: Receiver<BftMsg>,
    height: u64,
}

impl<T> BftActuator<T>
where
    T: SendMsg + Crypto,
{
    /// A function to create a new Bft actuator.
    pub fn new(recv_msg: T, address: Address) -> Self {
        let (sender, internal_receiver) = unbounded();
        let (internal_sender, receiver) = unbounded();
        Bft::start(internal_sender, internal_receiver, address);
        Self {
            recv_msg,
            sender,
            receiver,
            height: INIT_HEIGHT,
        }
    }

    ///
    pub fn start<F: BftSupport>(&mut self) {
        ::std::thread::spawn(move || {});
    }
}
