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

use core::timer::TimeoutInfo;

use std::collection::{BTreeMap, HashMap, VecDeque};
use std::sync::mpsc::{Receiver, RecvError, Sender};
use std::time::{Duration, Instant};

use type::{Address, H256};

const INIT_HEIGHT: usize = 1;
const INIT_ROUND: usize = 0;

const LOG_TYPE_PROPOSE: u8 = 1;
const LOG_TYPE_VOTE: u8 = 2;
const LOG_TYPE_STATE: u8 = 3;
const LOG_TYPE_PREV_HASH: u8 = 4;
const LOG_TYPE_COMMITS: u8 = 5;
const LOG_TYPE_VERIFIED_PROPOSE: u8 = 6;
const LOG_TYPE_AUTH_TXS: u8 = 7;

const TIMEOUT_RETRANSE_MULTIPLE: u32 = 15;
const TIMEOUT_LOW_ROUND_MESSAGE_MULTIPLE: u32 = 20;

const VERIFIED_PROPOSAL_OK: i8 = 1;
const VERIFIED_PROPOSAL_FAILED: i8 = -1;
const VERIFIED_PROPOSAL_UNDO: i8 = 0;

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

fn gen_reqid_from_idx(h: u64, r: u64) -> u64 {
    ((h & 0xffff_ffff_ffff) << 16) | r
}

fn get_idx_from_reqid(reqid: u64) -> (u64, u64) {
    (reqid >> 16, reqid & 0xffff)
}

pub struct Bft {
    timer_seter: Sender<TimeoutInfo>,
    timer_notity: Receiver<TimeoutInfo>,

    params: BftParams,
    height: usize,
    round: usize,
    step: Step,
    proof: BftProof,
    // pre_hash: Option<H256>,
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
        let proof = BftProof::default();
        let logpath = DataPath::wal_path();

        Bft {
            timer_seter: ts,
            timer_notity: rs,

            params,
            height: 0,
            round: INIT_ROUND,
            step: Step::Propose,
            proof,
            // pre_hash: None,
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

    fn is_round_proposer(
        &self,
        height: usize,
        round: usize,
        address: &Address,
    ) -> Result<(), EngineError> {
        let authority_list = &self.auth_manage;
        if authority_list.authorities.is_empty() {
            warn!("There are no authorities");
            return Err(EngineError::NotAuthorized(Address::zero()));
        }

        let proposer: &Address = authority_list
            .authorities
            .get((height + round) % authority_list.authorities.len())
            .expect(
                "There are validator_n() authorities; \
                 taking number modulo validator_n() gives number in validator_n() range; QED",
            );
        if proposer == address {
            Ok(())
        } else {
            Err(EngineError::NotProposer(Mismatch {
                expected: *proposer,
                found: *address,
            }))
        }
    }
    
    pub fn is_locked(&mut self) -> Option<Proposal> {
        if let Some(lock_round) = self.lock_round {
            let locked_vote = &self.lock_vote;
            let locked_proposal = &self.lock_proposal.clone();
            {
                let locked_proposal_hash = locked_proposal.crypt_hash();
                info!(
                    "proposal lock block: height {:?}, round {:?} block hash {:?}",
                    self.height, self.round, lock_blk_hash
                );
                self.proposal = Some(locked_proposal_hash);
            }
            let blk = locked_proposal.try_into().unwrap();
            trace!(
                "pub_proposal proposer vote locked block: height {}, round {}",
                self.height,
                self.round
            );
            let proposal = Proposal {
                block: blk,
                lock_round: Some(lock_round),
                lock_votes: lock_vote.clone(),
            };
            Some(proposal)
        } else {
            None
        }
    }


}