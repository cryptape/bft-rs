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

use bincode::{deserialize, serialize, Infinite};
use crypto::{pubkey_to_address, CreateKey, Sign, Signature, SIGNATURE_BYTES_LEN};
use engine::{unix_now, AsMillis, EngineError, Mismatch};
use message::Message;
use params::BftParams;
use timer::TimeoutInfo;
use util::datapath::DataPath;
use util::Hashable;
use voteset::AuthorityManage;
use voteset::*;
use wal::Wal;
use CryptHash;

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

const LOG_TYPE_PROPOSE: u8 = 1;
const LOG_TYPE_STATE: u8 = 3;
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
    pub timer_seter: Sender<TimeoutInfo>,
    pub timer_notity: Receiver<TimeoutInfo>,

    pub params: BftParams,
    pub height: usize,
    pub round: usize,
    pub step: Step,
    votes: VoteCollector,
    proposals: Proposal,
    proposal: Option<H256>,
    lock_round: Option<usize>,
    lock_proposal: Option<Proposal>,
    lock_vote: Option<VoteSet>,
    wal_log: Wal,
    last_commit_round: Option<usize>,
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
            step: Step::default(),
            votes: VoteCollector::new(),
            proposals: Proposal::new(),
            proposal: None,
            lock_round: None,
            lock_proposal: None,
            lock_vote: None,
            wal_log: Wal::new(&*logpath).unwrap(),
            last_commit_round: None,
            auth_manage: authority_list,
        }
    }

    // judge whether a node is the proposer of the round or not
    pub fn is_round_proposer(&mut self, address: &Address) -> bool {
        let authority_list = &self.auth_manage;
        let height = self.height;
        let round = self.round;

        if authority_list.proposers.is_empty() {
            warn!("There are no authorities");
            return false;
        }

        let proposer: &Address = authority_list
            .proposers
            .get((height + round) % authority_list.proposers.len())
            .expect(
                "There are validator_n() authorities; \
                 taking number modulo validator_n() gives number in validator_n() range; QED",
            );
        // if the node is not the proposer set timer 2.4 * (r + 1)s
        if proposer == address {
            return true;
        } else {
            info!("The node {} is not proposer", address);
            let coef = {
                if round > MAX_PROPOSAL_TIME_COEF {
                    MAX_PROPOSAL_TIME_COEF
                } else {
                    round
                }
            };
            let now = Instant::now();
            let _ = self.timer_seter.send(TimeoutInfo {
                timeval: now + self.params.timer.get_propose() * 2u32.pow(coef as u32),
                height,
                round,
                step: Step::ProposeWait,
            });
            return false;
        }
    }

    // if the node is proposer, check if it is locked on the proposal
    pub fn is_locked(&mut self) -> Option<Proposal> {
        let height = self.height;
        let round = self.round;

        if let Some(lock_round) = self.lock_round {
            let locked_vote = &self.lock_vote;
            let locked_proposal = &self.lock_proposal.clone().unwrap();
            {
                let locked_proposal_hash = locked_proposal.block.crypt_hash();
                info!(
                    "proposal lock block: height {:?}, round {:?} block hash {:?}",
                    height, round, locked_proposal_hash
                );
                self.proposal = Some(locked_proposal_hash);
            }
            let blk = locked_proposal.clone().block;
            trace!(
                "pub_proposal proposer vote locked block: height {}, round {}",
                height,
                round
            );
            let proposal = Proposal {
                block: blk,
                height: height,
                round: round,
                lock_round: Some(lock_round),
                lock_votes: locked_vote.clone(),
            };
            let now = Instant::now();
            let _ = self.timer_seter.send(TimeoutInfo {
                timeval: now + Duration::new(0, 0),
                height,
                round,
                step: Step::ProposeWait,
            });
            Some(proposal)
        } else {
            let now = Instant::now();
            let _ = self.timer_seter.send(TimeoutInfo {
                timeval: now + Duration::new(0, 0),
                height,
                round,
                step: Step::ProposeWait,
            });
            None
        }
    }

    // check something of signature
    pub fn handle_proposal(
        &mut self,
        // proposal_height: usize,
        // proposal_round: usize,
        signed_proposal: SignProposal,
    ) -> Result<(usize, usize), EngineError> {
        trace!(
            "handle proposal here self height {} round {} step {:?} ",
            self.height,
            self.round,
            self.step,
        );

        let proposal = signed_proposal.proposal;
        let signature = signed_proposal.signature;
        let proposal_height = proposal.height;
        let proposal_round = proposal.round;

        // check the length of signature
        if signature.len() != SIGNATURE_BYTES_LEN {
            return Err(EngineError::InvalidSignature);
        }

        // check proposal's height and round
        if proposal_height < self.height
            || (proposal_height == self.height && proposal_round < self.round)
            || (proposal_height == self.height
                && proposal_round == self.round
                && self.step > Step::ProposeWait)
        {
            debug!(
                "handle proposal get old proposal now height {} round {} step {:?}",
                self.height, self.round, self.step
            );
            return Err(EngineError::VoteMsgDelay(proposal_height));
        }

        if (proposal_height == self.height && proposal_round >= self.round)
            || proposal_height > self.height
        {
            // self.wal_log.save(proposal_height, LOG_TYPE_PROPOSE, &signed_proposal.proposal.block.clone()).unwrap();
            debug!(
                "add proposal height {} round {}",
                proposal_height, proposal_round
            );
            self.proposals = proposal;
            return Ok((proposal_height, proposal_round));
        }
        Err(EngineError::UnexpectedMessage)
    }

    // check lock
    pub fn proc_proposal(&mut self, proposal: Proposal) {
        let proposal_lock_round = proposal.lock_round;

        // try to unlock
        if self.lock_round.is_some()
            && proposal_lock_round.is_some()
            && self.lock_round.unwrap() < proposal_lock_round.unwrap()
            && proposal_lock_round.unwrap() < self.round
        {
            trace!(
                "unlock lock block: height {:?}, proposal {:?}",
                self.height,
                self.proposal
            );
            self.clean_save_info();
        }

        // fail to unlock, then prevote it
        if self.lock_round.is_some() {
            self.proposal = Some(self.lock_proposal.clone().unwrap().crypt_hash());
        } else {
            // use the new proposal and lock it
            self.proposal = Some(proposal.block.crypt_hash());
            self.lock_proposal = Some(proposal);
            debug!(
                "save the proposal's hash: height {:?}, round {}, proposal {:?}",
                self.height,
                self.round,
                self.proposal.unwrap()
            );
        }
    }

    // do prevote
    pub fn proc_prevote(&mut self) -> Message {
        let height = self.height;
        let round = self.round;

        let now = Instant::now();
        let _ = self.timer_seter.send(TimeoutInfo {
            timeval: now + (self.params.timer.get_prevote() * TIMEOUT_RETRANSE_MULTIPLE),
            height: height,
            round: round,
            step: Step::Prevote,
        });
        // if has a lock, prevote it, else prevote nil
        if self.lock_round.is_some() || self.proposal.is_some() {
            Message {
                height: height,
                round: round,
                step: Step::Prevote,
                proposal: self.proposal,
            }
        } else {
            Message {
                height: height,
                round: self.round,
                step: Step::Prevote,
                proposal: Some(H256::default()),
            }
        }
    }

    // check if prevotes is above 2/3
    pub fn check_prevote(&mut self, height: usize, round: usize) -> bool {
        info!(
            "check_prevote begin height {}, round {} vs self {}, round {}",
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
                    // if a proposal's prevote above 2/3, first unlock, then lock on the new one
                    if self.above_threshold(*count) {
                        if self.lock_round.is_some()
                            && self.lock_round.unwrap() < round
                            && round <= self.round
                        {
                            trace!("unlock lock block height {:?}, hash {:?}", height, hash);
                            self.lock_round = None;
                            self.lock_vote = None;
                        }

                        // if the hash is none, unlock, else lock on the new one
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

    // do precommit
    pub fn proc_precommit(&mut self, verify_result: i8) -> Message {
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

    // check if precommits is above 2/3
    pub fn check_precommit(&mut self, height: usize, round: usize) -> bool {
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

        if let Some(vote_set) = vote_set {
            if self.above_threshold(vote_set.count) {
                trace!(
                    "check_precommit is_above_threshold height {} round {}",
                    height,
                    round
                );
                let mut tv = if self.all_vote(vote_set.count) {
                    Duration::new(0, 0)
                } else {
                    self.params.timer.get_precommit()
                };

                for (hash, count) in vote_set.votes_by_proposal {
                    // if a proposal's precommit above 2/3, compare its hash and self.proposal
                    if self.above_threshold(count) {
                        trace!(
                            "proc_precommit is_above_threshold hash {:?} {}",
                            hash,
                            count
                        );

                        if hash.is_zero() {
                            tv = Duration::new(0, 0);
                            trace!("precommit's hash is zero");
                        } else if self.proposal.is_some() {
                            // if precommit hash neq self.proposal return false, else set last_commit_round
                            if hash != self.proposal.unwrap() {
                                trace!(
                                    "the self.proposal {:?} ne hash {:?}",
                                    self.proposal.unwrap(),
                                    hash
                                );
                                self.clean_save_info();
                                return false;
                            } else {
                                self.proposal = Some(hash);
                                self.last_commit_round = Some(round);
                                tv = Duration::new(0, 0);
                            }
                        } else {
                            trace!("there is no self.proposal");
                            return false;
                        }
                        break;
                    }
                }

                if self.step == Step::Precommit {
                    let now = Instant::now();
                    let _ = self.timer_seter.send(TimeoutInfo {
                        timeval: now + tv,
                        height,
                        round,
                        step: Step::PrecommitWait,
                    });
                }
                return true;
            }
        }
        false
    }

    // do commit
    pub fn proc_commit(&mut self, height: usize, round: usize) -> Option<ProposalwithProof> {
        if self.height == height && self.round == round {
            // check if the precommit's hash eq self.proposal
            if let Some(cround) = self.last_commit_round {
                if cround == round && self.proposal.is_some() {
                    trace!("commit begin {} {}", height, round);
                    if let Some(hash) = self.proposal {
                        if self.lock_proposal.is_some() {
                            // generate proof and return proposal with proof
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

    // start a new round
    pub fn new_round(&mut self, height: usize, round: usize, authority_list: AuthorityManage) {
        let height_now = self.height;
        // height_now != height => consensus success in the height, then goto next height
        if height_now != height {
            // clean lock info, update authority list
            self.clean_save_info();
            self.height = height;
            self.round = round;
            self.auth_manage = authority_list;
        } else {
            // only update the round info
            self.round = round;
        }
        // wait for 3s ?
    }

    pub fn change_step(&mut self, height: usize, round: usize, s: Step, newflag: bool) {
        self.height = height;
        self.round = round;
        self.step = s;

        if newflag {
            let _ = self.wal_log.set_height(height);
        }

        let message = serialize(&(height, round, s), Infinite).unwrap();
        let _ = self.wal_log.save(height, LOG_TYPE_STATE, &message);
    }

    #[inline]
    pub fn is_validator(&self, address: &Address) -> bool {
        self.auth_manage.validators.contains(address)
    }

    #[inline]
    pub fn above_threshold(&self, n: usize) -> bool {
        n * 3 > self.auth_manage.validators.len() * 2
    }

    #[inline]
    pub fn all_vote(&self, n: usize) -> bool {
        n == self.auth_manage.validators.len()
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
