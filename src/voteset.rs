// CITA
// Copyright 2016-2019 Cryptape Technologies LLC.

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
use super::{Address, Target, Vote};
use algorithm::Step;
use lru_cache::LruCache;

use std::collections::HashMap;

/// BFT vote collector
#[derive(Debug)]
pub struct VoteCollector {
    /// A LruCache to store vote collect of each round.
    pub votes: LruCache<usize, RoundCollector>,
    /// A HashMap to record prevote count of each round.
    pub prevote_count: HashMap<usize, usize>,
}

impl VoteCollector {
    /// A function to create a new BFT vote collector.
    pub fn new() -> Self {
        VoteCollector {
            votes: LruCache::new(16),
            prevote_count: HashMap::new(),
        }
    }

    /// A function try to add a vote, return `bool`.
    pub fn add(&mut self, vote: Vote) -> bool {
        let height = vote.height;
        let round = vote.round;
        let vote_type = vote.vote_type;
        let sender = vote.voter;
        let vote = vote.proposal;

        if vote_type == Step::Prevote {
            if self.votes.contains_key(&height) {
                if self
                    .votes
                    .get_mut(&height)
                    .unwrap()
                    .add(round, vote_type, sender, vote)
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
                round_votes.add(round, vote_type, sender, vote);
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
                .add(round, vote_type, sender, vote)
        } else {
            let mut round_votes = RoundCollector::new();
            round_votes.add(round, vote_type, sender, vote);
            self.votes.insert(height, round_votes);
            true
        }
    }

    /// A function to get the vote set of the height, the round, and the vote type.
    pub fn get_voteset(&mut self, height: usize, round: usize, vote_type: Step) -> Option<VoteSet> {
        self.votes
            .get_mut(&height)
            .and_then(|rc| rc.get_voteset(round, vote_type))
    }

    /// A function to clean prevote count HashMap at the begining of a height.
    pub fn clear_prevote_count(&mut self) {
        self.prevote_count.clear();
    }
}

/// BFT vote set
// 1. sender's vote message  2. proposal's hash  3. count
#[derive(Clone, Debug)]
pub struct VoteSet {
    /// A HashMap that K is voter, V is proposal.
    pub votes_by_sender: HashMap<Address, Target>,
    /// A HashMap that K is proposal V is count of the proposal.
    pub votes_by_proposal: HashMap<Target, usize>,
    /// Count of vote set.
    pub count: usize,
}

impl VoteSet {
    /// A function to create a new vote set.
    pub fn new() -> Self {
        VoteSet {
            votes_by_sender: HashMap::new(),
            votes_by_proposal: HashMap::new(),
            count: 0,
        }
    }

    /// A function to add a vote to the vote set.
    pub fn add(&mut self, sender: Address, vote: Target) -> bool {
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
    pub fn abstract_polc(
        &self,
        height: usize,
        round: usize,
        vote_type: Step,
        proposal: &Target,
    ) -> Vec<Vote> {
        // abstract the votes for the polc proposal into a vec
        let mut polc = Vec::new();
        for (address, vote_proposal) in &self.votes_by_sender {
            if vote_proposal == proposal {
                polc.push(Vote {
                    vote_type,
                    height,
                    round,
                    proposal: proposal.clone(),
                    voter: address.clone(),
                });
            }
        }
        polc
    }
}

/// BFT round vote collector.
// round -> step collector
#[derive(Debug)]
pub struct RoundCollector {
    /// A LruCache to store step collect of a round.
    pub round_votes: LruCache<usize, StepCollector>,
}

impl RoundCollector {
    /// A function to create a new round collector.
    pub fn new() -> Self {
        RoundCollector {
            round_votes: LruCache::new(16),
        }
    }

    /// A function try to add a vote to a round collector.
    pub fn add(&mut self, round: usize, vote_type: Step, sender: Address, vote: Target) -> bool {
        if self.round_votes.contains_key(&round) {
            self.round_votes
                .get_mut(&round)
                .unwrap()
                .add(vote_type, sender, vote)
        } else {
            let mut step_votes = StepCollector::new();
            step_votes.add(vote_type, sender, vote);
            self.round_votes.insert(round, step_votes);
            true
        }
    }

    /// A functionto get the vote set of the round, and the vote type.
    pub fn get_voteset(&mut self, round: usize, vote_type: Step) -> Option<VoteSet> {
        self.round_votes
            .get_mut(&round)
            .and_then(|sc| sc.get_voteset(vote_type))
    }
}

/// BFT step collector.
// step -> voteset
#[derive(Debug, Default)]
pub struct StepCollector {
    /// A HashMap that K is step, V is the vote set
    pub step_votes: HashMap<Step, VoteSet>,
}

impl StepCollector {
    /// A function to create a new step collector.
    pub fn new() -> Self {
        StepCollector {
            step_votes: HashMap::new(),
        }
    }

    /// A function to add a vote to the step collector.
    pub fn add(&mut self, vote_type: Step, sender: Address, vote: Target) -> bool {
        self.step_votes
            .entry(vote_type)
            .or_insert_with(VoteSet::new)
            .add(sender, vote)
    }

    /// A function to get voteset of the vote type
    pub fn get_voteset(&self, vote_type: Step) -> Option<VoteSet> {
        self.step_votes.get(&vote_type).cloned()
    }
}
