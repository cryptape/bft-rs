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

use crypto::{pubkey_to_address, CreateKey, Sign, Signature, SIGNATURE_BYTES_LEN};
use engine::{unix_now, AsMillis, EngineError, Mismatch};
use message::Message;
use params::*;
use timer::TimeoutInfo;
use util::datapath::DataPath;
use util::Hashable;
use voteset::*;
use wal::Wal;
use {AuthorityManage, CryptHash};

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::{Into, TryFrom, TryInto};
use std::sync::mpsc::{Receiver, RecvError, Sender};
use std::time::{Duration, Instant};

use ethereum_types::{Address, H256};
use serde_derive::{Deserialize, Serialize};

const INIT_HEIGHT: usize = 1;
const INIT_ROUND: usize = 0;

const TIMEOUT_RETRANSE_MULTIPLE: u32 = 15;
const TIMEOUT_LOW_ROUND_MESSAGE_MULTIPLE: u32 = 20;

const VERIFIED_PROPOSAL_OK: i8 = 1;
const VERIFIED_PROPOSAL_FAILED: i8 = -1;
const VERIFIED_PROPOSAL_UNDO: i8 = 0;

const MAX_PROPOSAL_TIME_COEF: usize = 10;

#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Eq, Clone, Copy, Hash)]
pub enum Step {
    Propose,
    ProposeWait,
    Prevote,
    PrevoteWait,
    Precommit,
    PrecommitWait,
    Commit,
    CommitWait,
}

impl Default for Step {
    fn default() -> Step {
        Step::Propose
    }
}

impl From<u8> for Step {
    fn from(s: u8) -> Step {
        match s {
            0u8 => Step::Propose,
            1u8 => Step::ProposeWait,
            2u8 => Step::Prevote,
            3u8 => Step::PrevoteWait,
            4u8 => Step::Precommit,
            5u8 => Step::PrecommitWait,
            6u8 => Step::Commit,
            7u8 => Step::CommitWait,
            _ => panic!("Invalid step."),
        }
    }
}

pub struct Bft {
    timer_seter: Sender<TimeoutInfo>,
    timer_notity: Receiver<TimeoutInfo>,

    params: BftParams,
    height: usize,
    round: usize,
    step: Step,
    votes: VoteCollector,
    proposals: ProposalCollector,
    proposal: Option<H256>,
    lock_round: Option<usize>,
    lock_proposal: Option<Proposal>,
    lock_vote: Option<VoteSet>,
    wal_log: Wal,
    last_commit_round: Option<usize>,
    htime: Instant,
    auth_manage: AuthorityManage,
}

impl Bft {
    pub fn new(
        ts: Sender<TimeoutInfo>,
        rs: Receiver<TimeoutInfo>,
        params: BftParams,
        authority_list: AuthorityManage,
    ) -> Bft {
        let logpath = DataPath::wal_path();

        Bft {
            timer_seter: ts,
            timer_notity: rs,

            params,
            height: 0,
            round: INIT_ROUND,
            step: Step::Propose,
            votes: VoteCollector::new(),
            proposals: ProposalCollector::new(),
            proposal: None,
            lock_round: None,
            lock_proposal: None,
            lock_vote: None,
            wal_log: Wal::new(&*logpath).unwrap(),
            last_commit_round: None,
            htime: Instant::now(),
            auth_manage: authority_list,
        }
    }

    fn is_round_proposer(&self, height: usize, round: usize, address: &Address) -> bool {
        let authority_list = &self.auth_manage;
        if authority_list.authorities.is_empty() {
            warn!("There are no authorities");
            return false;
        }

        let proposer: &Address = authority_list
            .authorities
            .get((height + round) % authority_list.authorities.len())
            .expect(
                "There are validator_n() authorities; \
                 taking number modulo validator_n() gives number in validator_n() range; QED",
            );
        if proposer == address {
            return true;
        } else {
            info!("The node {} is not proposer", address);
            return false;
        }
    }

    pub fn is_locked(&mut self) -> Option<Proposal> {
        if let Some(lock_round) = self.lock_round {
            let locked_vote = &self.lock_vote;
            let locked_proposal = &self.lock_proposal.clone().unwrap();
            {
                let locked_proposal_hash = locked_proposal.block.crypt_hash();
                info!(
                    "proposal lock block: height {:?}, round {:?} block hash {:?}",
                    self.height, self.round, locked_proposal_hash
                );
                self.proposal = Some(locked_proposal_hash);
            }
            let blk = locked_proposal.clone().block;
            trace!(
                "pub_proposal proposer vote locked block: height {}, round {}",
                self.height,
                self.round
            );
            let proposal = Proposal {
                block: blk,
                lock_round: Some(lock_round),
                lock_votes: locked_vote.clone(),
            };
            Some(proposal)
        } else {
            None
        }
    }

    fn handle_proposal(&mut self, signed_proposal: SignProposal) {
        trace!(
            "handle proposal here self height {} round {} step {:?} ",
            self.height,
            self.round,
            self.step,
        );
    }

    fn proc_proposal(&mut self, height: usize, round: usize) -> bool {
        let proposal = self.proposals.get_proposal(height, round);
        if let Some(proposal) = proposal {
            trace!(
                "proc proposal height {},round {} self {} {} ",
                height,
                round,
                self.height,
                self.round
            );

            let proposal_lock_round = proposal.lock_round;
            if self.lock_round.is_some()
                && proposal_lock_round.is_some()
                && self.lock_round.unwrap() < proposal_lock_round.unwrap()
                && proposal_lock_round.unwrap() < round
            {
                trace!(
                    "unlock lock block: height {:?}, proposal {:?}",
                    height,
                    self.proposal
                );
                self.clean_save_info();
            }
            if self.lock_round.is_some() {
                let locked_proposal = &self.lock_proposal.clone().unwrap();
                self.proposal = Some(locked_proposal.crypt_hash());
                trace!(
                    "still have lock block {} locked round {} {:?}",
                    self.height,
                    self.lock_round.unwrap(),
                    self.proposal.unwrap()
                );
            } else {
                self.proposal = Some(proposal.crypt_hash());
                self.lock_proposal = Some(proposal);
            }
            return true;
        }
        false
    }

    fn proc_prevote(&mut self) -> Message {
        let now = Instant::now();
        let _ = self.timer_seter.send(TimeoutInfo {
            timeval: now + (self.params.timer.get_prevote() * TIMEOUT_RETRANSE_MULTIPLE),
            height: self.height,
            round: self.round,
            step: Step::Prevote,
        });

        if self.lock_round.is_some() || self.proposal.is_some() {
            Message {
                height: self.height,
                round: self.round,
                step: Step::Prevote,
                proposal: self.proposal,
            }
        } else {
            Message {
                height: self.height,
                round: self.round,
                step: Step::Prevote,
                proposal: Some(H256::default()),
            }
        }
    }

    fn check_prevote(&mut self, height: usize, round: usize) -> bool {
        debug!(
            "proc_prevote begin height {}, round {} vs self {}, round {}",
            height, round, self.height, self.round
        );
        if height < self.height
            || (height == self.height && round < self.round)
            || (height == self.height && round == self.round && self.step > Step::Prevote)
        {
            return false;
        }

        let vote_set = self.votes.get_voteset(height, round, Step::Prevote);
        trace!("proc_prevote vote_set {:?}", vote_set);
        if let Some(vote_set) = vote_set {
            if self.above_threshold(vote_set.count) {
                let mut tv = if self.all_vote(vote_set.count) {
                    Duration::new(0, 0)
                } else {
                    self.params.timer.get_prevote()
                };

                for (hash, count) in &vote_set.votes_by_proposal {
                    if self.above_threshold(*count) {
                        //we have lock block,and now polc  then unlock
                        if self.lock_round.is_some()
                            && self.lock_round.unwrap() < round
                            && round <= self.round
                        {
                            //we see new lock block unlock mine
                            trace!("unlock lock block height {:?}, hash {:?}", height, hash);
                            self.lock_round = None;
                            self.lock_vote = None;
                        }

                        if hash.is_zero() {
                            self.clean_save_info();
                            tv = Duration::new(0, 0);
                        } else if self.proposal == Some(*hash) {
                            self.lock_round = Some(round);
                            self.lock_vote = Some(vote_set.clone());
                            tv = Duration::new(0, 0);
                        } else {
                            return false;
                        }
                        //more than one hash have threahold is wrong !! do some check ??
                        break;
                    }
                }

                if self.step == Step::Prevote {
                    // self.change_state_step(height, round, Step::PrevoteWait, false);
                    let now = Instant::now();
                    let _ = self.timer_seter.send(TimeoutInfo {
                        timeval: now + tv,
                        height,
                        round,
                        step: Step::PrevoteWait,
                    });
                }
                return true;
            }
        }
        false
    }

    fn proc_precommit(&mut self, verify_result: i8) -> Message {
        let now = Instant::now();
        //timeout for resending vote msg
        let _ = self.timer_seter.send(TimeoutInfo {
            timeval: now + (self.params.timer.get_precommit() * TIMEOUT_RETRANSE_MULTIPLE),
            height: self.height,
            round: self.round,
            step: Step::Precommit,
        });

        if verify_result == VERIFIED_PROPOSAL_OK {
            Message {
                height: self.height,
                round: self.round,
                step: Step::Precommit,
                proposal: self.proposal,
            }
        } else {
            Message {
                height: self.height,
                round: self.round,
                step: Step::Precommit,
                proposal: Some(H256::default()),
            }
        }
    }

    fn check_lock(&self) -> bool {
        if let Some(lround) = self.lock_round {
            trace!(
                "pre_proc_precommit locked round,{},self round {}",
                lround,
                self.round
            );
            if lround == self.round {
                return true;
            }
        }
        false
    }

    fn check_precommit(&mut self, height: usize, round: usize) -> bool {
        debug!(
            "proc_precommit begin {} {} vs self {} {}",
            height, round, self.height, self.round
        );
        if height < self.height
            || (height == self.height && round < self.round)
            || (height == self.height && self.round == round && self.step > Step::PrecommitWait)
        {
            return false;
        }

        let vote_set = self.votes.get_voteset(height, round, Step::Precommit);
        trace!(
            "proc_precommit deal height {} round {} voteset {:?}",
            height,
            round,
            vote_set
        );
        false
    }

    fn proc_commit(&mut self, height: usize, round: usize) -> Option<ProposalwithProof> {
        if self.height == height && self.round == round {
            if let Some(cround) = self.last_commit_round {
                if cround == round && self.proposal.is_some() {
                    trace!("commit begin {} {}", height, round);
                    if let Some(hash) = self.proposal {
                        if self.lock_proposal.is_some() {
                            let proof = self.generate_proof(height, round, hash);
                            return Some(ProposalwithProof {
                                proposal: self.lock_proposal.clone().unwrap(),
                                proof: proof.unwrap(),
                            });
                        }
                    }
                }
            }
        }
        // goto next round
        info!("Commit fail and goto next round");
        None
    }

    fn generate_proof(&mut self, height: usize, round: usize, hash: H256) -> Option<BftProof> {
        let mut commits = HashMap::new();
        {
            let vote_set = self.votes.get_voteset(height, round, Step::Precommit);
            let mut num: usize = 0;
            if let Some(vote_set) = vote_set {
                for (sender, vote) in &vote_set.votes_by_sender {
                    if vote.proposal.is_none() {
                        continue;
                    }
                    if vote.proposal.unwrap() == hash {
                        num += 1;
                        commits.insert(*sender, vote.signature.clone());
                    }
                }
            }
            if !self.above_threshold(num) {
                trace!("Fail to generate proof height{} round{}.", height, round);
                return None;
            }
        }
        let mut proof = BftProof::default();
        proof.height = height;
        proof.round = round;
        proof.proposal = hash;
        proof.commits = commits;
        Some(proof)
    }

    fn new_round(&mut self, height: usize, round: usize) {
        let coef = {
            if round > MAX_PROPOSAL_TIME_COEF {
                MAX_PROPOSAL_TIME_COEF
            } else {
                round
            }
        };
        
        let mut tv = self.params.timer.get_propose() * 2u32.pow(coef as u32);
        if self.proposals.get_proposal(height, round).is_some() {
            tv = Duration::new(0, 0);
        } else if self.is_round_proposer(height, round, &self.params.signer.address) {
            self.is_locked();
            tv = Duration::new(0, 0);
        }
    }

    #[inline]
    fn above_threshold(&self, n: usize) -> bool {
        n * 3 > self.auth_manage.validator_n() * 2
    }

    #[inline]
    fn all_vote(&self, n: usize) -> bool {
        n == self.auth_manage.validator_n()
    }

    #[inline]
    fn clean_save_info(&mut self) {
        self.proposal = None;
        self.lock_round = None;
        self.lock_vote = None;
        self.lock_proposal = None;
        self.last_commit_round = None;
    }
}
