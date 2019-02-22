// CITA
// Copyright 2016-2017 Cryptape Technologies LLC.

// This program is free software: you can redistribute it
// and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation,
// either version 3 of the License, or (at your option) any
// later version.

// This program is distributed in the hope that it will be
// useful, but WITHOUT ANY WARRANTY; without even the implied
// warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
// PURPOSE. See the GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.
use super::{Address, Target};
use std::cell::Cell;
use std::time::Duration;

/// BFT params.
#[derive(Clone, Debug)]
pub struct BftParams {
    /// The local address.
    pub address: Address,
    /// A set of BFT timer setting.
    pub timer: BftTimer,
}

impl BftParams {
    /// A function to create a new BFT params.
    pub fn new(local_address: Target) -> Self {
        BftParams {
            address: local_address,
            timer: BftTimer::default(),
        }
    }
}

/// A set of BFT timer.
#[derive(Debug, Clone)]
pub struct BftTimer {
    // in milliseconds.
    total_duration: Cell<u64>,
    // fraction: (numerator, denominator)
    propose: (u64, u64),
    prevote: (u64, u64),
    precommit: (u64, u64),
    commit: (u64, u64),
}

impl Default for BftTimer {
    fn default() -> Self {
        BftTimer {
            total_duration: Cell::new(3000),
            propose: (24, 30),
            prevote: (1, 30),
            precommit: (1, 30),
            commit: (4, 30),
        }
    }
}

impl BftTimer {
    /// A function to set total interval.
    pub fn set_total_duration(&self, duration: u64) {
        self.total_duration.set(duration);
    }

    /// A function to get propose wait duration.
    pub fn get_propose(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.propose.0 / self.propose.1)
    }

    /// A function to get prevote wait duration.
    pub fn get_prevote(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.prevote.0 / self.prevote.1)
    }

    /// A function to get precommit wait duration.
    pub fn get_precommit(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.precommit.0 / self.precommit.1)
    }

    /// [Deprecated]A function to get commit wait duration.
    pub fn get_commit(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.commit.0 / self.commit.1)
    }
}
