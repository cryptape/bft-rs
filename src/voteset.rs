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

use super::Step;
use bincode::{serialize, Infinite};
use lru_cache::LruCache;
use std::collections::HashMap;
use protobuf::Message as Message_imported_for_functions;
use protobuf::ProtobufEnum as ProtobufEnum_imported_for_functions;
use types::{Address, H256};

pub struct VoteCollector {
    pub votes: LruCache<usize, RoundCollector>,
}

impl VoteCollector {
    pub fn new() -> Self {
        VoteCollector {
            votes: LruCache::new(16),
        }
    }

    pub fn add(
        &mut self,
        height: usize,
        round: usize,
        step: Step,
        sender: Address,
        vote: &VoteMessage,
    ) -> bool {
        if self.votes.contains_key(&height) {
            self.votes
                .get_mut(&height)
                .unwrap()
                .add(round, step, sender, vote)
        } else {
            let mut round_votes = RoundCollector::new();
            round_votes.add(round, step, sender, vote);
            self.votes.insert(height, round_votes);
            true
        }
    }

    pub fn get_voteset(&mut self, height: usize, round: usize, step: Step) -> Option<VoteSet> {
        self.votes
            .get_mut(&height)
            .and_then(|rc| rc.get_voteset(round, step))
    }
}

//1. sender's votemessage 2. proposal'hash count
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VoteSet {
    pub votes_by_sender: HashMap<Address, VoteMessage>,
    pub votes_by_proposal: HashMap<H256, usize>,
    pub count: usize,
}

impl VoteSet {
    pub fn new() -> Self {
        VoteSet {
            votes_by_sender: HashMap::new(),
            votes_by_proposal: HashMap::new(),
            count: 0,
        }
    }

    //just add ,not check
    pub fn add(&mut self, sender: Address, vote: &VoteMessage) -> bool {
        let mut is_add = false;
        self.votes_by_sender.entry(sender).or_insert_with(|| {
            is_add = true;
            vote.to_owned()
        });
        if is_add {
            self.count += 1;
            let hash = vote.proposal.unwrap_or_else(H256::default);
            *self.votes_by_proposal.entry(hash).or_insert(0) += 1;
        }
        is_add
    }

    pub fn check(
        &self,
        h: usize,
        r: usize,
        step: Step,
        authorities: &[Address],
    ) -> Result<Option<H256>, &str> {
        let mut votes_by_proposal: HashMap<H256, usize> = HashMap::new();
        for (sender, vote) in &self.votes_by_sender {
            if authorities.contains(sender) {
                let msg = serialize(&(h, r, step, sender, vote.proposal), Infinite).unwrap();
                let signature = &vote.signature;
                if let Ok(pubkey) = signature.recover(&msg.crypt_hash()) {
                    if pubkey_to_address(&pubkey) == *sender {
                        let hash = vote.proposal.unwrap_or_else(H256::default);
                        // inc the count of vote for hash
                        *votes_by_proposal.entry(hash).or_insert(0) += 1;
                    }
                }
            }
        }
        for (hash, count) in &votes_by_proposal {
            if *count * 3 > authorities.len() * 2 {
                if hash.is_zero() {
                    return Ok(None);
                } else {
                    return Ok(Some(*hash));
                }
            }
        }
        Err("vote set check error!")
    }
}

// height -> round collector
#[derive(Debug)]
pub struct VoteCollector {
    pub votes: LruCache<usize, RoundCollector>,
}

impl VoteCollector {
    pub fn new() -> Self {
        VoteCollector {
            votes: LruCache::new(16),
        }
    }

    pub fn add(
        &mut self,
        height: usize,
        round: usize,
        step: Step,
        sender: Address,
        vote: &VoteMessage,
    ) -> bool {
        if self.votes.contains_key(&height) {
            self.votes
                .get_mut(&height)
                .unwrap()
                .add(round, step, sender, vote)
        } else {
            let mut round_votes = RoundCollector::new();
            round_votes.add(round, step, sender, vote);
            self.votes.insert(height, round_votes);
            true
        }
    }

    pub fn get_voteset(&mut self, height: usize, round: usize, step: Step) -> Option<VoteSet> {
        self.votes
            .get_mut(&height)
            .and_then(|rc| rc.get_voteset(round, step))
    }
}

//round -> step collector
#[derive(Debug)]
pub struct RoundCollector {
    pub round_votes: LruCache<usize, StepCollector>,
}

impl RoundCollector {
    pub fn new() -> Self {
        RoundCollector {
            round_votes: LruCache::new(16),
        }
    }

    pub fn add(&mut self, round: usize, step: Step, sender: Address, vote: &VoteMessage) -> bool {
        if self.round_votes.contains_key(&round) {
            self.round_votes
                .get_mut(&round)
                .unwrap()
                .add(step, sender, &vote)
        } else {
            let mut step_votes = StepCollector::new();
            step_votes.add(step, sender, &vote);
            self.round_votes.insert(round, step_votes);
            true
        }
    }

    pub fn get_voteset(&mut self, round: usize, step: Step) -> Option<VoteSet> {
        self.round_votes
            .get_mut(&round)
            .and_then(|sc| sc.get_voteset(step))
    }
}

//step -> voteset
#[derive(Debug)]
pub struct StepCollector {
    pub step_votes: HashMap<Step, VoteSet>,
}

impl StepCollector {
    pub fn new() -> Self {
        StepCollector {
            step_votes: HashMap::new(),
        }
    }

    pub fn add(&mut self, step: Step, sender: Address, vote: &VoteMessage) -> bool {
        self.step_votes
            .entry(step)
            .or_insert_with(VoteSet::new)
            .add(sender, vote)
    }

    pub fn get_voteset(&self, step: Step) -> Option<VoteSet> {
        self.step_votes.get(&step).cloned()
    }
}

pub struct ProposalCollector {
    pub proposals: LruCache<usize, ProposalRoundCollector>,
}

impl ProposalCollector {
    pub fn new() -> Self {
        ProposalCollector {
            proposals: LruCache::new(16),
        }
    }

    pub fn add(&mut self, height: usize, round: usize, proposal: Proposal) -> bool {
        if self.proposals.contains_key(&height) {
            self.proposals
                .get_mut(&height)
                .unwrap()
                .add(round, proposal)
        } else {
            let mut round_proposals = ProposalRoundCollector::new();
            round_proposals.add(round, proposal);
            self.proposals.insert(height, round_proposals);
            true
        }
    }

    pub fn get_proposal(&mut self, height: usize, round: usize) -> Option<Proposal> {
        self.proposals
            .get_mut(&height)
            .and_then(|prc| prc.get_proposal(round))
    }
}

#[derive(Debug)]
pub struct ProposalRoundCollector {
    pub round_proposals: LruCache<usize, Proposal>,
}

impl ProposalRoundCollector {
    pub fn new() -> Self {
        ProposalRoundCollector {
            round_proposals: LruCache::new(16),
        }
    }

    pub fn add(&mut self, round: usize, proposal: Proposal) -> bool {
        if self.round_proposals.contains_key(&round) {
            false
        } else {
            self.round_proposals.insert(round, proposal);
            true
        }
    }

    pub fn get_proposal(&mut self, round: usize) -> Option<Proposal> {
        self.round_proposals.get_mut(&round).cloned()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Proposal {
    pub block: Vec<u8>,
    pub lock_round: Option<usize>,
    pub lock_votes: Option<VoteSet>,
}

pub struct BlockWithProof {
    // message fields
    pub blk: ::protobuf::SingularPtrField<Block>,
    pub proof: ::protobuf::SingularPtrField<Proof>,
    // special fields
    unknown_fields: ::protobuf::UnknownFields,
    cached_size: ::protobuf::CachedSize,
}

impl BlockWithProof {
    pub fn new() -> BlockWithProof {
        ::std::default::Default::default()
    }

    // .Block blk = 1;
    pub fn clear_blk(&mut self) {
        self.blk.clear();
    }

    pub fn has_blk(&self) -> bool {
        self.blk.is_some()
    }

    // Param is passed by value, moved
    pub fn set_blk(&mut self, v: Block) {
        self.blk = ::protobuf::SingularPtrField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_blk(&mut self) -> &mut Block {
        if self.blk.is_none() {
            self.blk.set_default();
        }
        self.blk.as_mut().unwrap()
    }

    // Take field
    pub fn take_blk(&mut self) -> Block {
        self.blk.take().unwrap_or_else(|| Block::new())
    }

    pub fn get_blk(&self) -> &Block {
        self.blk.as_ref().unwrap_or_else(|| Block::default_instance())
    }

    // .Proof proof = 2;

    pub fn clear_proof(&mut self) {
        self.proof.clear();
    }

    pub fn has_proof(&self) -> bool {
        self.proof.is_some()
    }

    // Param is passed by value, moved
    pub fn set_proof(&mut self, v: Proof) {
        self.proof = ::protobuf::SingularPtrField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_proof(&mut self) -> &mut Proof {
        if self.proof.is_none() {
            self.proof.set_default();
        }
        self.proof.as_mut().unwrap()
    }

    // Take field
    pub fn take_proof(&mut self) -> Proof {
        self.proof.take().unwrap_or_else(|| Proof::new())
    }

    pub fn get_proof(&self) -> &Proof {
        self.proof.as_ref().unwrap_or_else(|| Proof::default_instance())
    }
}