use crate::{Address, Target};

use std::cell::Cell;
use std::time::Duration;

/// BFT params.
#[derive(Clone, Debug)]
pub(crate) struct BftParams {
    /// The local address.
    pub(crate) address: Address,
    /// A set of BFT timer settings.
    pub(crate) timer: BftTimer,
}

impl BftParams {
    /// A function to create a new BFT params.
    pub(crate) fn new(local_address: Target) -> Self {
        BftParams {
            address: local_address,
            timer: BftTimer::default(),
        }
    }
}

/// A set of BFT timer.
#[derive(Debug, Clone)]
pub(crate) struct BftTimer {
    // in milliseconds.
    total_duration: Cell<u64>,
    // fraction: (numerator, denominator)
    propose: (u64, u64),
    prevote: (u64, u64),
    precommit: (u64, u64),
}

impl Default for BftTimer {
    fn default() -> Self {
        BftTimer {
            total_duration: Cell::new(3000),
            propose: (24, 30),
            prevote: (1, 30),
            precommit: (1, 30),
        }
    }
}

impl BftTimer {
    /// A function to set total interval.
    pub(crate) fn set_total_duration(&self, duration: u64) {
        self.total_duration.set(duration);
    }

    /// A function to get propose wait duration.
    pub(crate) fn get_propose(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.propose.0 / self.propose.1)
    }

    /// A function to get prevote wait duration.
    pub(crate) fn get_prevote(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.prevote.0 / self.prevote.1)
    }

    /// A function to get precommit wait duration.
    pub(crate) fn get_precommit(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.precommit.0 / self.precommit.1)
    }
}
