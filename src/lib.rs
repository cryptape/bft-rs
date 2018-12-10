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
#![allow(unused_imports)]
#![allow(unused_results)]
#![feature(try_from)]

#[macro_use]
extern crate bincode;
extern crate ethereum_types;
#[macro_use]
extern crate logger;
// extern crate authority_manage;
extern crate crypto_hash;
extern crate lru_cache;
extern crate min_max_heap;
extern crate protobuf;
extern crate rustc_serialize;
#[macro_use]
extern crate serde_derive;
extern crate cita_crypto as crypto;
extern crate engine;
#[macro_use]
extern crate util;
extern crate log;

pub mod bft;
pub mod message;
pub mod params;
pub mod timer;
pub mod voteset;
pub mod wal;

use bincode::{deserialize, serialize, Infinite};
use crypto_hash::*;
use ethereum_types::{Address, H256};
use serde_derive::{Deserialize, Serialize};

pub const DATA_PATH: &'static str = "DATA_PATH";
pub const LOG_TYPE_AUTHORITIES: u8 = 1;

pub trait CryptHash {
    fn crypt_hash(&self) -> H256;
}
