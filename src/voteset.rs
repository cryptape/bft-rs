use crate::{Address, Hash, Proposal, SignedVote, VoteType};

use std::collections::HashMap;

use lru_cache::LruCache;

/// BFT vote collector
#[derive(Debug)]
pub(crate) struct VoteCollector {
    /// A LruCache to store vote collect of each round.
    pub(crate) votes: LruCache<u64, RoundCollector>,
    /// A HashMap to record prevote count of each round.
    pub(crate) prevote_count: HashMap<u64, u64>,
}

impl VoteCollector {
    /// A function to create a new BFT vote collector.
    pub(crate) fn new() -> Self {
        VoteCollector {
            votes: LruCache::new(16),
            prevote_count: HashMap::new(),
        }
    }

    /// A function try to add a vote, return `bool`.
    pub(crate) fn add(&mut self, signed_vote: &SignedVote, vote_weight: u64) -> bool {
        let vote = &signed_vote.vote;
        let height = vote.height;
        let round = vote.round;
        let vote_type = &vote.vote_type;


        if *vote_type == VoteType::Prevote {
            if self.votes.contains_key(&height) {
                if self
                    .votes
                    .get_mut(&height)
                    .unwrap()
                    .add(signed_vote, vote_weight)
                {
                    // update prevote count hashmap
                    let counter = self.prevote_count.entry(round).or_insert(0);
                    *counter += vote_weight;
                    true
                } else {
                    // if add prevote fail, do not update prevote hashmap
                    false
                }
            } else {
                let mut round_votes = RoundCollector::new();
                round_votes.add(signed_vote, vote_weight);
                self.votes.insert(height, round_votes);
                // update prevote count hashmap
                let counter = self.prevote_count.entry(round).or_insert(0);
                *counter += vote_weight;
                true
            }
        } else if self.votes.contains_key(&height) {
            self.votes
                .get_mut(&height)
                .unwrap()
                .add(signed_vote, vote_weight)
        } else {
            let mut round_votes = RoundCollector::new();
            round_votes.add(signed_vote, vote_weight);
            self.votes.insert(height, round_votes);
            true
        }
    }

    /// A function to get the vote set of the height, the round, and the vote type.
    pub(crate) fn get_voteset(
        &mut self,
        height: u64,
        round: u64,
        vote_type: &VoteType,
    ) -> Option<VoteSet> {
        self.votes
            .get_mut(&height)
            .and_then(|rc| rc.get_voteset(round, vote_type))
    }

    /// A function to clean prevote count HashMap at the begining of a height.
    pub(crate) fn clear_prevote_count(&mut self) {
        self.prevote_count.clear();
    }
}

/// BFT round vote collector.
// round -> step collector
#[derive(Debug)]
pub(crate) struct RoundCollector {
    /// A LruCache to store step collect of a round.
    pub(crate) round_votes: LruCache<u64, StepCollector>,
}

impl RoundCollector {
    /// A function to create a new round collector.
    pub(crate) fn new() -> Self {
        RoundCollector {
            round_votes: LruCache::new(16),
        }
    }

    /// A function try to add a vote to a round collector.
    pub(crate) fn add(
        &mut self,
        signed_vote: &SignedVote,
        vote_weight: u64,
    ) -> bool {
        let round = signed_vote.vote.round;

        if self.round_votes.contains_key(&round) {
            self.round_votes
                .get_mut(&round)
                .unwrap()
                .add(signed_vote, vote_weight)
        } else {
            let mut step_votes = StepCollector::new();
            step_votes.add(signed_vote, vote_weight);
            self.round_votes.insert(round, step_votes);
            true
        }
    }

    /// A functionto get the vote set of the round, and the vote type.
    pub(crate) fn get_voteset(&mut self, round: u64, vote_type: &VoteType) -> Option<VoteSet> {
        self.round_votes
            .get_mut(&round)
            .and_then(|sc| sc.get_voteset(vote_type))
    }
}

/// BFT step collector.
// step -> voteset
#[derive(Debug, Default)]
pub(crate) struct StepCollector {
    /// A HashMap that K is step, V is the vote set
    pub(crate) step_votes: HashMap<VoteType, VoteSet>,
}

impl StepCollector {
    /// A function to create a new step collector.
    pub(crate) fn new() -> Self {
        StepCollector {
            step_votes: HashMap::new(),
        }
    }

    /// A function to add a vote to the step collector.
    pub(crate) fn add(&mut self, signed_vote: &SignedVote, vote_weight: u64) -> bool {
        let vote = &signed_vote.vote;
        let vote_type = &vote.vote_type;
        self.step_votes
            .entry(vote_type.clone())
            .or_insert_with(VoteSet::new)
            .add(signed_vote, vote_weight)
    }

    /// A function to get voteset of the vote type
    pub(crate) fn get_voteset(&self, vote_type: &VoteType) -> Option<VoteSet> {
        self.step_votes.get(vote_type).cloned()
    }
}


/// BFT vote set
// 1. sender's vote message  2. proposal's hash  3. count
#[derive(Clone, Debug)]
pub(crate) struct VoteSet {
    /// A HashMap that K is voter, V is proposal.
    pub(crate) votes_by_sender: HashMap<Address, SignedVote>,
    /// A HashMap that K is proposal V is count of the proposal.
    pub(crate) votes_by_proposal: HashMap<Hash, u64>,
    /// Count of vote set.
    pub(crate) count: u64,
}

impl VoteSet {
    /// A function to create a new vote set.
    pub(crate) fn new() -> Self {
        VoteSet {
            votes_by_sender: HashMap::new(),
            votes_by_proposal: HashMap::new(),
            count: 0u64,
        }
    }

    /// A function to add a vote to the vote set.
    pub(crate) fn add(&mut self, signed_vote: &SignedVote, vote_weight: u64) -> bool {
        let vote = &signed_vote.vote;
        let mut is_add = false;
        self.votes_by_sender.entry(vote.voter.clone()).or_insert_with(|| {
            is_add = true;
            signed_vote.to_owned()
        });
        if is_add {
            self.count += vote_weight;
            *self.votes_by_proposal.entry(vote.block_hash.clone()).or_insert(0) += vote_weight;
        }
        is_add
    }

    /// A function to abstract the PoLC of the round.
    pub(crate) fn extract_polc(
        &self,
        block_hash: &[u8],
    ) -> Vec<SignedVote> {
        // abstract the votes for the polc proposal into a vec
        let mut polc = Vec::new();
        for (_, signed_vote) in &self.votes_by_sender {
            let hash = &signed_vote.vote.block_hash;
            if hash.to_vec() == block_hash.to_vec() {
                polc.push(signed_vote.to_owned());
            }
        }
        polc
    }
}

#[derive(Debug)]
pub struct ProposalCollector {
    pub proposals: LruCache<u64, ProposalRoundCollector>,
}

impl ProposalCollector {
    pub fn new() -> Self {
        ProposalCollector {
            proposals: LruCache::new(16),
        }
    }

    pub fn add(&mut self, proposal: &Proposal) -> bool {
        let height = proposal.height;
        let round = proposal.round;
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

    pub fn get_proposal(&mut self, height: u64, round: u64) -> Option<Proposal> {
        self.proposals
            .get_mut(&height)
            .and_then(|prc| prc.get_proposal(round))
    }
}

#[derive(Debug)]
pub struct ProposalRoundCollector {
    pub round_proposals: LruCache<u64, Proposal>,
}

impl ProposalRoundCollector {
    pub fn new() -> Self {
        ProposalRoundCollector {
            round_proposals: LruCache::new(16),
        }
    }

    pub fn add(&mut self, round: u64, proposal: &Proposal) -> bool {
        if self.round_proposals.contains_key(&round) {
            false
        } else {
            self.round_proposals.insert(round, proposal.clone());
            true
        }
    }

    pub fn get_proposal(&mut self, round: u64) -> Option<Proposal> {
        self.round_proposals.get_mut(&round).cloned()
    }
}