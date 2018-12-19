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

use bft::Bft;
use bft::voteset::*;
use timer::WaitTimer;
use engine::EngineError;
use bincode::{deserialize, serialize, Infinite};
use message::{Message, ProposalMessage, CommitMessage};
use params::{BftParams, Config, PrivateKey};

use std::sync::mpsc::channel;
use std::thread;

const VERIFIED_OK: i8 = 1;

fn new_proposal(h: usize, r: usize) -> Option<Proposal> {
    ProposalCollecotr::get_proposal(h, r)
}

fn broadcast_proposal(pm: Proposal) {

}

fn receive_proposal() -> Result<SignProposal, EngineError> {

}

fn broadcast_message(m: Message) {

}

fn receive_message() -> Result<VoteCollector, EngineError> {
    let log_msg = message.to_owned();
    let res = deserialize(&message[..]);
    if let Ok(decoded) = res {
        let (message, address): 
    }
}

fn broadcast_commit_message(cm: CommitMessage) {

}

fn bft_recv_timeout(engine: BFT) -> bool {
    match engine.step {
        Step::ProposeWait => {
            while engine.timer_notity.try_recv().is_err(){
                if let Some(rp) = receive_proposal() {
                    return true;
                }
            }
            return false;
        },
        Step::Prevote => {
            while engine.timer_notity.try_recv().is_err() {
                if receive_message().is_ok(){
                    let num = receive_message().unwrap().len();
                    if engine.above_threshold(num) {
                        return true;
                    }
                }
            }
            return false;
        },
        Step::PrevoteWait => {

        },
        Step::Precommit => {
            while engine.timer_notity.try_recv().is_err() {
                if receive_message().is_ok(){
                    let num = receive_message().unwrap().len();
                    if engine.above_threshold(num) {
                        return true;
                    }
                }
            }
            return false;
        },
        Step::PrecommitWait => {
            
        },
        Step::Commit => {

        },
    }
}

fn bft_process(mut engine: BFT) {
    loop {
        match engine.step {
            Step::Propose => {
                if engine.is_round_proposer(engine.height, engine.round, engine.params.signer.address) {
                    if let mut Some(prop) = engine.is_locked() {
                        let pm = ProposalMessage {
                            height: engine.height,
                            round: engine.round,
                            proposal: prop,
                        };
                        broadcast_proposal(pm);
                    } else {
                        let pm = ProposalMessage {
                            height: engine.height,
                            round: engine.round,
                            Proposal: new_proposal(engine.height, engine.round).unwrap()
                        };
                        broadcast_proposal(pm);
                    }
                }
                engine.change_step(engine.height, engine.round, Step::ProposeWait, true);
            },
            Step::ProposeWait => {
                engine.proc_timeout();
                let rp = receive_proposal();
                engine.handle_proposal(rp);
                engine.proc_proposal();
                let pvm = engine.proc_prevote();
                broadcast_message(pvm);
                engine.change_step(engine.height, engine.round, Step::Prevote);
            },
            Step::Prevote => {
                engine.proc_timeout();
                // storage and verify received prevote 
                engine.check_prevote(engine.height, engine.round);
                engine.change_step(engine.height, engine.round, Step::PrevoteWait);
            },
            Step::PrevoteWait => {
                engine.proc_timeout();
                let pcm = engine.proc_precommit(VERIFIED_OK);
                broadcast_message(pcm);
                engine.change_step(engine.height, engine.round, Step::Precommit);
            },
            Step::Precommit => {
                engin.proc_timeout();
                // storage and verify precommit
                engine.check_precommit(engine.height, engine.round); 
                engine.change_step(engine.height, engine.round, Step::PrecommitWait);
            },
            Step::PrecommitWait => {
                engine.proc_timeout();
                let pwp = engine.proc_commit(engine.height, engine.round);
                // 
                engine.change_step.(engine.height, engine.round, Step::Commit);
            },
            Step::Commit => {
                engine.proc_timeout();
                engine.new_round(engine.height, engine.round,);
                
                engine.change_step(engine.height, engine.round, Step::Proposal);
            },
            _ => panic!("Invalid step."),
        }
    }
}

fn main() {
    let (main_to_timer, timer_from_main) = channel();
    let (timer_to_main, main_from_timer) = channel();
    let timethd = thread::spawn(move || {
        let wt = WaitTimer::new(timer_to_main, timer_from_main);
        wt.start();
    });

    let pk = PrivateKey::new(pk_path);
    let params = BftParams::new(&pk);
    let mainthd = thread::spawn(move || {
        let mut engine = Bft::new(main_from_timer, main_to_timer, params);
        bft_process(engine);
    });
     
}