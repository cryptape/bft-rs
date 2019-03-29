//! An efficent and stable Rust library of BFT protocol for distributed system.
//!
//!

#![deny(missing_docs)]

extern crate rlp;
#[macro_use]
extern crate crossbeam;
#[macro_use]
extern crate log;
extern crate lru_cache;
extern crate min_max_heap;
#[macro_use]
extern crate serde_derive;

use crate::error::BftError;

use rlp::{Encodable, RlpStream};

/// Bft actuator.
pub mod actuator;
/// BFT state machine.
pub mod algorithm;
/// BFT error.
pub mod error;
/// BFT params include time interval and local address.
pub mod params;
/// BFT timer.
pub mod timer;
/// BFT vote set.
pub mod voteset;

/// Type for node address.
pub type Address = Vec<u8>;
/// Type for proposal.
pub type Target = Vec<u8>;

/// BFT input message.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BftMsg {
    /// Proposal message.
    Proposal(Proposal),
    /// Vote message.
    Vote(Vote),
    /// Status message, rich status.
    Status(Status),
    /// Commit message.
    Commit(Commit),
    /// Pause BFT state machine.
    Pause,
    /// Start running BFT state machine.
    Start,
}

impl Into<u8> for BftMsg {
    fn into(self) -> u8 {
        match self {
            BftMsg::Proposal(_) => 0,
            BftMsg::Vote(_) => 1,
            BftMsg::Commit(_) => 2,
            _ => panic!(""),
        }
    }
}

/// Bft vote types.
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub enum VoteType {
    /// Vote type prevote.
    Prevote,
    /// Vote type precommit.
    Precommit,
}

impl From<u8> for VoteType {
    fn from(s: u8) -> Self {
        match s {
            0 => VoteType::Prevote,
            1 => VoteType::Precommit,
            _ => panic!("Invalid vote type!"),
        }
    }
}

impl Into<u8> for VoteType {
    fn into(self) -> u8 {
        match self {
            VoteType::Prevote => 0,
            VoteType::Precommit => 1,
        }
    }
}

impl Encodable for VoteType {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.append(self);
    }
}

/// A Bft proposal
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// The height of proposal.
    pub height: u64,
    /// The round of proposal.
    pub round: u64,
    /// The proposal content.
    pub content: Target,
    /// A lock round of the proposal.
    pub lock_round: Option<u64>,
    /// The lock votes of the proposal.
    pub lock_votes: Vec<Vote>,
    /// The address of proposer.
    pub proposer: Address,
}

impl Encodable for Proposal {
    fn rlp_append(&self, s: &mut RlpStream) {
        let votes = self.lock_votes.clone();
        s.append(&self.height)
            .append(&self.round)
            .append(&self.content)
            .append(&self.lock_round);
        for vote in votes.into_iter() {
            s.append(&vote);
        }
        s.append(&self.proposer);
    }
}

/// A PoLC.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LockStatus {
    /// The lock proposal
    pub proposal: Target,
    /// The lock round
    pub round: u64,
    /// The lock votes.
    pub votes: Vec<Vote>,
}

/// A vote to a proposal.
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Vote {
    /// Prevote vote or precommit vote
    pub vote_type: VoteType,
    /// The height of vote
    pub height: u64,
    /// The round of vote
    pub round: u64,
    /// The vote proposal
    pub proposal: Target,
    /// The address of voter
    pub voter: Address,
}

impl Encodable for Vote {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.append(&self.vote_type)
            .append(&self.height)
            .append(&self.round)
            .append(&self.proposal)
            .append(&self.voter);
    }
}

/// A result of a height.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// The height of result.
    pub height: u64,
    /// The round of result.
    pub round: u64,
    /// Consensus result
    pub proposal: Target,
    /// Votes for generate proof.
    pub lock_votes: Vec<Vote>,
    /// The node address.
    pub address: Address,
}

/// Necessary messages for a height.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// The height of rich status.
    pub height: u64,
    /// The time interval of next height. If it is none, maintain the old interval.
    pub interval: Option<u64>,
    /// A new authority list for next height.
    pub authority_list: Vec<Address>,
}

/// A signed proposal.
pub struct SignProposal<T: Crypto> {
    /// Bft proposal.
    pub proposal: Proposal,
    /// Proposal signature.
    pub signature: T::Signature,
}

/// A signed vote.
pub struct SignVote<T> {
    /// Bft Vote.
    pub vote: Vote,
    /// Vote signature.
    pub signature: T,
}


///
pub trait BftSupport {
    /// A function to check signature.
    fn verify_proposal(&self, proposal: Proposal) -> Result<bool, BftError>;
    /// A function to pack proposal.
    fn package_proposal(&self, height: u64) -> Result<Proposal, BftError>;
    /// A function to update rich status.
    fn update_status(&self, height: u64) -> Result<Status, BftError>;
    /// A funciton to transmit messages.
    fn transmit(&self, msg: BftMsg) -> Result<(), BftError>;

    /// A function to get verify result.
    #[cfg(feature = "verify_req")]
    fn verify_transcation(&self, p: Target) -> Result<bool, BftError>;
}

///
pub trait Crypto {
    /// Hash type
    type Hash;
    /// Signature types
    type Signature: Crypto;
    /// A function to get signature.
    fn get_signature(&self) -> Self::Signature;
    /// A function to encrypt hash.
    fn hash(&self, msg: Vec<u8>) -> Self::Hash;
    /// A function to check signature
    fn check_signature(&self, hash: &Self::Hash, sig: &Self::Signature) -> Result<(), BftError>;
    /// A function to signature the message hash.
    fn signature(&self, hash: &Self::Hash, privkey: &[u8]) -> Self::Signature;
}
