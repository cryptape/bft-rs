use crate::{Address, Hash, Proposal, Vote, VoteType};

use std::collections::HashMap;

use lru_cache::LruCache;

/// BFT vote collector
#[derive(Debug)]
pub(crate) struct VoteCollector {
    /// A LruCache to store vote collect of each round.
    pub(crate) votes: LruCache<u64, RoundCollector>,
    /// A HashMap to record prevote count of each round.
    pub(crate) prevote_count: HashMap<u64, usize>,
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
    pub(crate) fn add(&mut self, vote: Vote) -> bool {
        let height = vote.height;
        let round = vote.round;
        let vote_type = vote.vote_type;
        let sender = vote.voter;
        let block_hash = vote.block_hash;

        if vote_type == VoteType::Prevote {
            if self.votes.contains_key(&height) {
                if self
                    .votes
                    .get_mut(&height)
                    .unwrap()
                    .add(vote)
                {
                    // update prevote count hashmap
                    let counter = self.prevote_count.entry(round).or_insert(0);
                    *counter += 1;
                    true
                } else {
                    // if add prevote fail, do not update prevote hashmap
                    false
                }
            } else {
                let mut round_votes = RoundCollector::new();
                round_votes.add(vote);
                self.votes.insert(height, round_votes);
                // update prevote count hashmap
                let counter = self.prevote_count.entry(round).or_insert(0);
                *counter += 1;
                true
            }
        } else if self.votes.contains_key(&height) {
            self.votes
                .get_mut(&height)
                .unwrap()
                .add(vote)
        } else {
            let mut round_votes = RoundCollector::new();
            round_votes.add(vote);
            self.votes.insert(height, round_votes);
            true
        }
    }

    /// A function to get the vote set of the height, the round, and the vote type.
    pub(crate) fn get_voteset(
        &mut self,
        height: u64,
        round: u64,
        vote_type: VoteType,
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
        vote: Vote,
    ) -> bool {
        let round = vote.round;
        let vote_type = vote.vote_type;
        let sender = vote.sender;
        let block_hash = vote.block_hash;

        if self.round_votes.contains_key(&round) {
            self.round_votes
                .get_mut(&round)
                .unwrap()
                .add(vote)
        } else {
            let mut step_votes = StepCollector::new();
            step_votes.add(vote);
            self.round_votes.insert(round, step_votes);
            true
        }
    }

    /// A functionto get the vote set of the round, and the vote type.
    pub(crate) fn get_voteset(&mut self, round: u64, vote_type: VoteType) -> Option<VoteSet> {
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
    pub(crate) fn add(&mut self, vote: Vote) -> bool {
        let vote_type = vote.vote_type;
        self.step_votes
            .entry(vote_type)
            .or_insert_with(VoteSet::new)
            .add(vote)
    }

    /// A function to get voteset of the vote type
    pub(crate) fn get_voteset(&self, vote_type: VoteType) -> Option<VoteSet> {
        self.step_votes.get(&vote_type).cloned()
    }
}


/// BFT vote set
// 1. sender's vote message  2. proposal's hash  3. count
#[derive(Clone, Debug)]
pub(crate) struct VoteSet {
    /// A HashMap that K is voter, V is proposal.
    pub(crate) votes_by_sender: HashMap<Address, Vote>,
    /// A HashMap that K is proposal V is count of the proposal.
    pub(crate) votes_by_proposal: HashMap<Hash, usize>,
    /// Count of vote set.
    pub(crate) count: usize,
}

impl VoteSet {
    /// A function to create a new vote set.
    pub(crate) fn new() -> Self {
        VoteSet {
            votes_by_sender: HashMap::new(),
            votes_by_proposal: HashMap::new(),
            count: 0,
        }
    }

    /// A function to add a vote to the vote set.
    pub(crate) fn add(&mut self, vote: Vote) -> bool {
        let mut is_add = false;
        self.votes_by_sender.entry(sender).or_insert_with(|| {
            is_add = true;
            vote.to_owned()
        });
        if is_add {
            self.count += 1;
            *self.votes_by_proposal.entry(vote).or_insert(0) += 1;
        }
        is_add
    }

    /// A function to abstract the PoLC of the round.
    pub(crate) fn extract_polc(
        &self,
        height: u64,
        round: u64,
        vote_type: VoteType,
        proposal: &[u8],
    ) -> Vec<Vote> {
        // abstract the votes for the polc proposal into a vec
        let mut polc = Vec::new();
        for (address, vote) in &self.votes_by_sender {
            let hash = vote.block_hash;
            let proposal = proposal.to_vec();
            if hash == &proposal {
                polc.push(vote.to_owned());
            }
        }
        polc
    }
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