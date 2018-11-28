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
use crypto::{PrivKey, Signer};
use ethereum_types::clean_0x;
use serde_derive::{Deserialize, Serialize};
use std::cell::Cell;
use std::fs::File;
use std::io::Read;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
pub struct PrivateKey {
    signer: PrivKey,
}

impl PrivateKey {
    pub fn new(path: &str) -> Self {
        let mut buffer = String::new();
        File::open(path)
            .and_then(|mut f| f.read_to_string(&mut buffer))
            .unwrap_or_else(|err| panic!("Error while loading PrivateKey: [{}]", err));

        let signer = PrivKey::from_str(clean_0x(&buffer)).unwrap();

        PrivateKey { signer }
    }
}

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
    pub fn set_total_duration(&self, duration: u64) {
        self.total_duration.set(duration);
    }

    pub fn get_propose(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.propose.0 / self.propose.1)
    }

    pub fn get_prevote(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.prevote.0 / self.prevote.1)
    }

    pub fn get_precommit(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.precommit.0 / self.precommit.1)
    }

    pub fn get_commit(&self) -> Duration {
        Duration::from_millis(self.total_duration.get() * self.commit.0 / self.commit.1)
    }
}

pub struct BftParams {
    pub timer: BftTimer,
    pub signer: Signer,
}

impl BftParams {
    pub fn new(priv_key: &PrivateKey) -> Self {
        BftParams {
            signer: Signer::from(priv_key.signer),
            timer: BftTimer::default(),
        }
    }
}
