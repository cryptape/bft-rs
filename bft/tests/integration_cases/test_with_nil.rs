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

use bft::algorithm::{Bft, Step};
use bft::params::{BftParams, BftTimer};
use bft::voteset::*;
use crypto::{pubkey_to_address, CreateKey, KeyPair, Signature, Signer};
use ethereum_types::{H160, H256};
use hash::{digest, Algorithm};
use rand::{thread_rng, Rng};
use {generate_auth, generate_proposal, ASCII_WORD};

use std::sync::mpsc::channel;
use std::vec::Vec;

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

    // there is 1/3 probability that node 2 become offline
    let mut offline_node = H160::zero();
    let mut rng = thread_rng();
    let x: usize = rng.gen_range(0, 10);
    if x < 4 {
        offline_node = auth.validators[2];
    }

    let byzantine_node = auth.validators[3];
    for ii in 0..4 {
        if auth.validators[ii] != byzantine_node && auth.validators[ii] != offline_node {
            let proposal = H256::from(digest(Algorithm::SHA256, &p.block).as_slice());
            println!(
                "do {:?} on height {}, round {}, for {:?}",
                s, h, r, proposal
            );

            engine.votes.add(
                h,
                r,
                s,
                auth.validators[ii],
                &VoteMessage {
                    proposal: Some(proposal),
                    signature: Signature::default(),
                },
            );
        } else {
            let proposal = H256::from(digest(Algorithm::SHA256, &byzantine).as_slice());
            println!(
                "do {:?} on height {}, round {}, for {:?}",
                s, h, r, proposal
            );

            engine.votes.add(
                h,
                r,
                s,
                auth.validators[ii],
                &VoteMessage {
                    proposal: Some(proposal),
                    signature: Signature::default(),
                },
            );
        }
    }
}

#[test]
fn test_bft_without_proposer() {
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
    let (authority_list, _) = generate_auth();
    let (main_to_timer, _timer_from_main) = channel();
    let (_timer_to_main, main_from_timer) = channel();
    let mut engine = Bft::new(
        main_to_timer,
        main_from_timer,
        params,
        authority_list.clone(),
    );
    let mut height = 2;
    let mut round = 0;
    let mut proposals: Vec<Vec<u8>> = Vec::new();
    let mut consensus_results: Vec<Vec<u8>> = Vec::new();
    println!("{}", ASCII_WORD);

    while height < 1000 {
        // step commit
        let (auth, _) = generate_auth();
        println!("height {}, self.height {}", height, engine.height);
        engine.new_round(height, round, auth.clone());

        // step proposal
        println!("start height{}, round{}", height, round);
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
            proposal.clone(),
            Step::Precommit,
            height,
            round,
        );
        engine.change_step(height, round, Step::Precommit, true);

        // step precommit
        engine.check_precommit(height, round);
        engine.change_step(height, round, Step::PrecommitWait, true);

        // step precommit wait
        let height_old = height;
        let round_old = round;
        let commit = engine.proc_commit(height, round);
        if commit.clone().is_some() {
            consensus_results.push(commit.clone().unwrap().proposal.block);
            println!(
                "consensus success! the result is {:?}, height{}, round{}",
                commit.clone().unwrap().proposal.block,
                height,
                round
            );
            height += 1;
            round = 0
        } else {
            println!(
                "the consensus result is nil, height{}, round{}, then go to next round",
                height, round
            );
            round += 1;
        }

        if commit.clone().is_some()
            && commit.clone().unwrap().proposal.block != proposal.clone().block
        {
            panic!("Consensus Error in height{}, round {}!", height - 1, round - 1);
        }
        engine.change_step(height_old, round_old, Step::Commit, true);
    }
}
