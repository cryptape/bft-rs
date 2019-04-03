use crate::*;
use crate::{
    algorithm::{Bft, INIT_HEIGHT},
    error::BftError,
};

use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};

/// Results of Bft actuator.
pub type Result<T> = ::std::result::Result<T, BftError>;

/// A Bft Actuator
pub struct BftActuator (Sender<BftMsg>);

impl BftActuator {
    /// A function to create a new Bft actuator.
    pub fn new<T: BftSupport + Send + 'static>(support: T, address: Address, wal_path: &str) -> Self {
        let (sender, internal_receiver) = unbounded();
        Bft::start(internal_receiver, support, address, wal_path);
        BftActuator(sender)
    }

    pub fn send(&self, input: BftInput) -> Result<()> {
        self.sender
            .send(input)
            .map_err(|_| BftError::SendMsgErr)
    }
}

#[cfg(test)]
mod test {

}
