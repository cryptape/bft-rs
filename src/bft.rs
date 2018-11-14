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
    pub_sender: Sender<PubType>,
    pub_recver: Receiver<TransType>,

    timer_seter: Sender<TimeoutInfo>,
    timer_notity: Receiver<TimeoutInfo>,

    params: BftParams,
    height: usize,
    round: usize,
    step: Step,
    proof: BftProof,
    pre_hash: Option<H256>,
    votes: VoteCollector,
    proposals: ProposalCollector,
    proposal: Option<H256>,
    lock_round: Option<usize>,
    locked_vote: Option<VoteSet>,
    // lock_round set, locked block means itself, else means proposal's block
    locked_block: Option<Block>,
    // wal_log: Wal,
    send_filter: HashMap<Address, (usize, Step, Instant)>,
    last_commit_round: Option<usize>,
    htime: Instant,
    auth_manage: AuthorityManage,
    //params meaning: key :index 0->height,1->round ,value:0->verified msg,1->verified result
    unverified_msg: BTreeMap<(usize, usize), (Message, i8)>,
    // VecDeque might work, Almost always it is better to use Vec or VecDeque instead of LinkedList
    block_txs: VecDeque<(usize, BlockTxs)>,
    block_proof: Option<(usize, BlockWithProof)>,
}

impl Bft {
    pub fn new(
        s: Sender<PubType>,
        r: Receiver<TransType>,
        ts: Sender<TimeoutInfo>,
        rs: Receiver<TimeoutInfo>,
        params: BftParams,
    ) -> Bft {
        let proof = BftProof::default();

        let logpath = DataPath::wal_path();
        Bft {
            pub_sender: s,
            pub_recver: r,
            timer_seter: ts,
            timer_notity: rs,

            params,
            height: 0,
            round: INIT_ROUND,
            step: Step::Propose,
            proof,
            pre_hash: None,
            votes: VoteCollector::new(),
            proposals: ProposalCollector::new(),
            proposal: None,
            lock_round: None,
            locked_vote: None,
            locked_block: None,
            // wal_log: Wal::new(&*logpath).unwrap(),
            send_filter: HashMap::new(),
            last_commit_round: None,
            htime: Instant::now(),
            auth_manage: AuthorityManage::new(),
            unverified_msg: BTreeMap::new(),
            block_txs: VecDeque::new(),
            block_proof: None,
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
    
}