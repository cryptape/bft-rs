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
use bft::Step;
use bincode::{deserialize, serialize, Infinite};
use crypto::{pubkey_to_address, Sign, Signature};
use ethereum_types::{Address, H256};
use lru_cache::LruCache;
use serde_derive::{Deserialize, Serialize};
use util::datapath::DataPath;
use util::Hashable;
use CryptHash;
use crypto_hash::{Algorithm, digest};

use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::usize::MAX;
use std::vec::Vec;

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

//1. sender's votemessage 2. proposal'hash count
#[derive(Clone, Debug)]
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

#[derive(Serialize, Clone, Debug)]
pub struct VoteMessage {
    pub proposal: Option<H256>,
    pub signature: Signature,
}

#[derive(Debug)]
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

#[derive(Clone, Debug, Default)]
pub struct Proposal {
    pub block: Vec<u8>,
    pub lock_round: Option<usize>,
    pub lock_votes: Option<VoteSet>,
}

impl CryptHash for Proposal {
    fn crypt_hash(&self) -> H256 {
        H256::from(digest(Algorithm::SHA256, &self.block).as_slice())
    }
}

#[derive(Clone, Debug)]
pub struct ProposalwithProof {
    pub proposal: Proposal,
    pub proof: BftProof,
}

#[derive(Clone, Debug, Default)]
pub struct SignProposal {
    pub proposal: Proposal,
    pub signature: Vec<u8>,
}

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct BftProof {
    pub proposal: H256,
    // Prev height
    pub height: usize,
    pub round: usize,
    pub commits: HashMap<Address, Signature>,
}

impl BftProof {
    pub fn new(
        height: usize,
        round: usize,
        proposal: H256,
        commits: HashMap<Address, Signature>,
    ) -> BftProof {
        BftProof {
            height: height,
            round: round,
            proposal: proposal,
            commits: commits,
        }
    }

    pub fn default() -> Self {
        BftProof {
            height: MAX,
            round: MAX,
            proposal: H256::default(),
            commits: HashMap::new(),
        }
    }

    pub fn store(&self) {
        let proof_path = DataPath::proof_bin_path();
        let mut file = File::create(&proof_path).unwrap();
        let encoded_proof: Vec<u8> = serialize(&self, Infinite).unwrap();
        file.write_all(&encoded_proof).unwrap();
        let _ = file.sync_all();
    }

    pub fn load(&mut self) {
        let proof_path = DataPath::proof_bin_path();
        if let Ok(mut file) = File::open(&proof_path) {
            let mut content = Vec::new();
            if file.read_to_end(&mut content).is_ok() {
                if let Ok(decoded) = deserialize(&content[..]) {
                    //self.round = decoded.round;
                    //self.proposal = decoded.proposal;
                    //self.commits = decoded.commits;
                    *self = decoded;
                }
            }
        }
    }

    pub fn is_default(&self) -> bool {
        if self.round == MAX {
            return true;
        }
        return false;
    }

    // Check proof commits
    pub fn check(&self, h: usize, authorities: &[Address]) -> bool {
        if h == 0 {
            return true;
        }
        if h != self.height {
            return false;
        }
        if 2 * authorities.len() >= 3 * self.commits.len() {
            return false;
        }
        self.commits.iter().all(|(sender, sig)| {
            if authorities.contains(sender) {
                let msg = serialize(
                    &(
                        h,
                        self.round,
                        Step::Precommit,
                        sender,
                        Some(self.proposal.clone()),
                    ),
                    Infinite,
                )
                .unwrap();
                let signature = Signature(sig.0.into());
                if let Ok(pubkey) = signature.recover(&msg.crypt_hash().into()) {
                    return pubkey_to_address(&pubkey) == sender.clone().into();
                }
            }
            false
        })
    }
}
