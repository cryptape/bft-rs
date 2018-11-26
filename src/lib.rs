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

use voteset::Proposal;
use wal::Wal;

use std::collections::HashMap;
use std::fs::File;
use std::usize::MAX;

pub const DATA_PATH: &'static str = "DATA_PATH";
pub const LOG_TYPE_AUTHORITIES: u8 = 1;

pub trait CryptHash {
    fn crypt_hash(&self) -> H256;
}

#[derive(Debug)]
pub struct AuthorityManage {
    pub authorities: Vec<Address>,
    pub validators: Vec<Address>,
    pub authorities_log: Wal,
    pub authorities_old: Vec<Address>,
    pub validators_old: Vec<Address>,
    pub authority_h_old: usize,
}

impl AuthorityManage {
    pub fn new() -> Self {
        let logpath = ::std::env::var(DATA_PATH)
            .expect(format!("{} must be set", DATA_PATH).as_str())
            + "/authorities";

        let mut authority_manage = AuthorityManage {
            authorities: Vec::new(),
            validators: Vec::new(),
            authorities_log: Wal::new(&*logpath).unwrap(),
            authorities_old: Vec::new(),
            validators_old: Vec::new(),
            authority_h_old: 0,
        };

        let vec_out = authority_manage.authorities_log.load();
        if !vec_out.is_empty() {
            if let Ok((h, authorities_old, authorities, validators)) = deserialize(&(vec_out[0].1))
            {
                let auth_old: Vec<Address> = authorities_old;
                let auth: Vec<Address> = authorities;
                let validators: Vec<Address> = validators;

                authority_manage.authorities.extend_from_slice(&auth);

                authority_manage
                    .authorities_old
                    .extend_from_slice(&auth_old);
                authority_manage.authority_h_old = h;
                authority_manage.validators.extend_from_slice(&validators);
            }
        }
        authority_manage
    }

    pub fn validator_n(&self) -> usize {
        self.validators.len()
    }

    pub fn receive_authorities_list(
        &mut self,
        height: usize,
        authorities: Vec<Address>,
        validators: Vec<Address>,
    ) {
        if self.authorities != authorities || self.validators != validators {
            self.authorities_old.clear();
            self.authorities_old.extend_from_slice(&self.authorities);
            self.validators_old.clear();
            self.validators_old.extend_from_slice(&self.validators);
            self.authority_h_old = height;

            self.authorities.clear();
            self.authorities.extend_from_slice(&authorities);
            self.validators.clear();
            self.validators.extend_from_slice(&validators);

            self.save();
        }
    }

    pub fn save(&mut self) {
        let bmsg = serialize(
            &(
                self.authority_h_old,
                self.authorities_old.clone(),
                self.authorities.clone(),
                self.validators.clone(),
            ),
            Infinite,
        )
        .unwrap();
        let _ = self.authorities_log.save(LOG_TYPE_AUTHORITIES, &bmsg);
    }
}
