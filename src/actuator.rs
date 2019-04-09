use crate::*;
use crate::{
    algorithm::Bft,
    error::BftError,
};

use crossbeam::crossbeam_channel::{unbounded, Sender};

/// A Bft Actuator
pub struct BftActuator (Sender<BftMsg>);

impl BftActuator {
    /// A function to create a new Bft actuator.
    pub fn new<T: BftSupport + Clone + Send + 'static>(support: T, address: Address, wal_path: &str) -> Self {
        let (sender, internal_receiver) = unbounded();
        Bft::start(internal_receiver, sender.clone(), support, address, wal_path);
        BftActuator(sender)
    }

    /// A function to create a new Bft actuator.
    pub fn send(&self, msg: BftMsg) -> BftResult<()> {
        self.0
            .send(msg)
            .map_err(|_| BftError::SendMsgErr)
    }
}