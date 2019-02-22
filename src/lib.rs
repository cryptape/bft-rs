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

//! An efficent and stable Rust library of BFT protocol for distributed system.
//!
//!

#![deny(missing_docs)]
#![allow(unused_imports)]
#![allow(unused_results)]

#[macro_use]
extern crate bincode;
#[macro_use]
extern crate crossbeam;
#[macro_use]
extern crate log;
extern crate log4rs;
extern crate lru_cache;
extern crate min_max_heap;
#[macro_use]
extern crate serde_derive;

use algorithm::Step;

/// BFT state machine.
pub mod algorithm;
/// BFT params include time interval and local address.
pub mod params;
/// BFT timer.
pub mod timer;
/// BFT vote set.
pub mod voteset;

/// The type for node address.
pub type Address = Vec<u8>;
/// The type for proposal.
pub type Target = Vec<u8>;

/// BFT message.
#[derive(Clone, Debug)]
pub enum BftMsg {
    /// Proposal message.
    Proposal(Proposal),
    /// Vote message.
    Vote(Vote),
    /// Feed messge, this is the proposal of the height.
    Feed(Feed),
    /// Status message, rich status.
    Status(Status),
    /// Commit message.
    Commit(Commit),
}

/// Something need to be consensus in a round.
/// A `Proposal` includes `height`, `round`, `content`, `lock_round`, `lock_votes`
/// and `proposer`. `lock_round` and `lock_votes` are `Option`, means the PoLC of
/// the proposal. Therefore, these must have same variant of `Option`.
#[derive(Clone, Debug)]
pub struct Proposal {
    height: usize,
    round: usize,
    content: Target,
    lock_round: Option<usize>,
    lock_votes: Option<Vec<Vote>>,
    proposer: Address,
}

/// A PoLC.
#[derive(Clone, Debug)]
pub struct LockStatus {
    proposal: Target,
    round: usize,
    votes: Vec<Vote>,
}

/// A vote to a proposal.
#[derive(Clone, Debug)]
pub struct Vote {
    vote_type: Step,
    height: usize,
    round: usize,
    proposal: Target,
    voter: Address,
}

/// A proposal for a height.
#[derive(Clone, Debug)]
pub struct Feed {
    /// The height of the proposal.
    pub height: usize,
    /// A proposal.
    pub proposal: Target,
}

/// A result of a height.
#[derive(Clone, Debug)]
pub struct Commit {
    /// The height of result.
    pub height: usize,
    /// Consensus result
    pub proposal: Target,
    /// Vote for generate proof.
    pub lock_votes: Vec<Vote>,
}

/// Necessary messages for a height.
#[derive(Clone, Debug)]
pub struct Status {
    /// The height of rich status.
    pub height: usize,
    /// The time interval of next height. If it is none, maintain the old interval.
    pub interval: Option<u64>,
    /// A new authority list for next height.
    pub authority_list: Vec<Address>,
}
