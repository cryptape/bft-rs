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

use bft::algorithm::{Bft, Step};
use bft::message::{CommitMessage, Message, ProposalMessage};
use bft::params::{BftParams, BftTimer};
use bft::voteset::*;
use bft::CryptHash;
use crypto::{pubkey_to_address, CreateKey, KeyPair, PubKey, Signature, Signer};
use ethereum_types::{Address, H256};
use hash::{digest, Algorithm};
use rand::{random, thread_rng, Rng};

use std::sync::mpsc::channel;
use std::usize::MAX;
use std::vec::Vec;

struct NodeInfo {
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

fn create_auth() -> (AuthorityManage, Vec<NodeInfo>) {
    let info_1 = gen_info();
    let info_2 = gen_info();
    let info_3 = gen_info();
    let proposers = vec![info_1.address, info_2.address, info_3.address];
    let validators = proposers.clone();
    let info = vec![info_1, info_2, info_3];
    let auth = AuthorityManage {
        proposers: proposers,
        validators: validators,
        proposers_old: Vec::new(),
        validators_old: Vec::new(),
        height_old: MAX,
    };
    (auth, info)
}

fn generate_proposal() -> Proposal {
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

fn generate_message(
    engine: &mut Bft,
    auth: AuthorityManage,
    p: Proposal,
    s: Step,
    h: usize,
    r: usize,
) {
    let mut byzantine = p.block.clone();
    let mut rng = thread_rng();

    for x in byzantine.iter_mut() {
        *x = rng.gen();
    }

    let byzantine_node = auth.validators[2];
    for ii in 0..2 {
        if auth.validators[ii] != byzantine_node {
            engine.votes.add(
                h,
                r,
                s,
                auth.validators[ii],
                &VoteMessage {
                    proposal: Some(H256::from(digest(Algorithm::SHA256, &p.block).as_slice())),
                    signature: Signature::default(),
                },
            );
        } else {
            engine.votes.add(
                h,
                r,
                s,
                auth.validators[ii],
                &VoteMessage {
                    proposal: Some(H256::from(digest(Algorithm::SHA256, &byzantine).as_slice())),
                    signature: Signature::default(),
                },
            );
        }
    }
}

#[test]
fn test_bft_without_propose() {
    let key_pair = KeyPair::gen_keypair();
    let pub_key = *key_pair.pubkey();
    let address = pubkey_to_address(&pub_key);
    let params = BftParams {
        timer: BftTimer::default(),
        signer: Signer {
            keypair: key_pair,
            address: address,
        },
    };
    let (authority_list, _) = create_auth();
    let (main_to_timer, timer_from_main) = channel();
    let (timer_to_main, main_from_timer) = channel();
    let mut engine = Bft::new(
        main_to_timer,
        main_from_timer,
        params,
        authority_list.clone(),
    );
    let mut height = 1;
    let mut round = 0;
    let mut proposals: Vec<Vec<u8>> = Vec::new();
    let mut consensus_results: Vec<Vec<u8>> = Vec::new();

    while height < 1000 {
        // step commit
        let (auth, _) = create_auth();
        height += 1;
        round = 0;
        engine.new_round(height, round, auth.clone());

        // step proposal
        let proposal = generate_proposal();
        println!(
            "the proposal is {:?}, height{}, round{}.",
            proposal.block.clone(),
            height,
            round
        );
        proposals.push(proposal.block.clone());

        // step proposal wait
        let _ = engine.recv_proposal(height, round, proposal.clone());
        engine.proc_proposal(proposal.clone());
        let _ = engine.proc_prevote();
        engine.change_step(height, round, Step::Prevote, true);

        // step prevote
        generate_message(
            &mut engine,
            auth.clone(),
            proposal.clone(),
            Step::Prevote,
            height,
            round,
        );
        engine.check_prevote(height, round);
        engine.change_step(height, round, Step::PrevoteWait, true);

        // step prevote wait
        generate_message(
            &mut engine,
            auth.clone(),
            proposal,
            Step::Precommit,
            height,
            round,
        );
        engine.change_step(height, round, Step::Precommit, true);

        // step precommit
        engine.check_precommit(height, round);
        engine.change_step(height, round, Step::PrecommitWait, true);

        // step precommit wait
        let commit = engine.proc_commit(height, round);
        consensus_results.push(commit.clone().unwrap().proposal.block);
        println!(
            "the consensus is {:?}, height{}, round{}",
            commit.clone().unwrap().proposal.block,
            height,
            round
        );
        // write to log

        engine.change_step(height, round, Step::Commit, true);
    }
}
