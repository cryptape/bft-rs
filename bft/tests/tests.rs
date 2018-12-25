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

extern crate bft_rs as bft;
extern crate cita_crypto as crypto;
extern crate crypto_hash as hash;
extern crate ethereum_types;
extern crate rand;

use bft::voteset::{AuthorityManage, Proposal};
use crypto::{pubkey_to_address, CreateKey, KeyPair};
use ethereum_types::Address;
use rand::{thread_rng, Rng};

use std::usize::MAX;

pub struct NodeInfo {
    keypair: KeyPair,
    address: Address,
}

fn gen_info() -> NodeInfo {
    let keypair = KeyPair::gen_keypair();
    let pubkey = *keypair.pubkey();
    let address = pubkey_to_address(&pubkey);
    NodeInfo {
        keypair: keypair,
        address: address,
    }
}

// This is use for generate authority list. And the Vec<NodeInfo> includes
// the keypairs of each node generated before, which is used while needs signature.
pub fn generate_auth() -> (AuthorityManage, Vec<NodeInfo>) {
    let info_1 = gen_info();
    let info_2 = gen_info();
    let info_3 = gen_info();
    let info_4 = gen_info();
    let proposers = vec![
        info_1.address,
        info_2.address,
        info_3.address,
        info_4.address,
    ];
    let validators = proposers.clone();
    let infos = vec![info_1, info_2, info_3, info_4];
    let auth = AuthorityManage {
        proposers: proposers,
        validators: validators,
        proposers_old: Vec::new(),
        validators_old: Vec::new(),
        height_old: MAX,
    };
    (auth, infos)
}

// generate a rand vec as proposal
pub fn generate_proposal() -> Proposal {
    let mut block = vec![1, 2, 3, 4];
    // make faster by caching thread_rng
    let mut rng = thread_rng();
    for x in block.iter_mut() {
        *x = rng.gen();
    }
    Proposal {
        block: block,
        lock_round: None,
        lock_votes: None,
    }
}