//! An efficent and stable Rust library of BFT protocol for distributed system.
//!
//!

#![deny(missing_docs)]

extern crate bincode;
#[macro_use]
extern crate crossbeam;
#[macro_use]
extern crate log;
extern crate lru_cache;
extern crate min_max_heap;
#[macro_use]
extern crate serde_derive;

use crate::error::BftError;

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
    /// Feed messge, this is the proposal of the height.
    Feed(Feed),
    /// Verify response
    #[cfg(feature = "verify_req")]
    VerifyResp(VerifyResp),
    /// Status message, rich status.
    Status(Status),
    /// Commit message.
    Commit(Commit),
    /// Pause BFT state machine.
    Pause,
    /// Start running BFT state machine.
    Start,
}

/// Bft vote types.
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub enum VoteType {
    /// Vote type prevote.
    Prevote,
    /// Vote type precommit.
    Precommit,
}

/// Something need to be consensus in a round.
/// A `Proposal` includes `height`, `round`, `content`, `lock_round`, `lock_votes`
/// and `proposer`. `lock_round` and `lock_votes` are `Option`, means the PoLC of
/// the proposal. Therefore, these must have same variant of `Option`.
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
    pub lock_votes: Option<Vec<Vote>>,
    /// The address of proposer.
    pub proposer: Address,
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

/// A proposal for a height.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    /// The height of the proposal.
    pub height: u64,
    /// A proposal.
    pub proposal: Target,
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

/// A verify result of a proposal.
#[cfg(feature = "verify_req")]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct VerifyResp {
    /// The Response of proposal verify
    pub is_pass: bool,
    /// The verify proposal
    pub proposal: Target,
}

/// A signed proposal.
pub struct SignProposal<T> {
    /// Bft proposal.
    pub proposal: Proposal,
    /// Proposal signature.
    pub signature: T,
}

/// A signed vote.
pub struct SignVote<T> {
    /// Bft Vote.
    pub vote: Vote,
    /// Vote signature.
    pub signature: T,
}

///
pub trait SendMsg {
    /// A function to send proposal.
    fn send_proposal<T>() -> Result<(), BftError>;
    /// A function to send vote.
    fn send_vote<T>() -> Result<(), BftError>;
    /// A function to start of pause Bft machine.
    fn send_commend() -> Result<(), BftError>;
}

///
pub trait BftSupport {
    /// A function to check signature.
    fn verify_proposal(proposal: Proposal) -> Result<bool, BftError>;
    /// A function to pack proposal.
    fn package_proposal(height: u64) -> Result<Proposal, BftError>;
    /// A function to update rich status.
    fn update_status(height: u64) -> Result<Status, BftError>;
    /// A funciton to transmit messages.
    fn transmit(msg: BftMsg) -> Result<(), BftError>;

    /// A function to get verify result.
    #[cfg(feature = "verify_req")]
    fn verify_proposal(p: Proposal) -> Result<bool, BftError>;
}

///
pub trait Crypto {
    /// Hash type
    type Hash;
    /// Signature type
    type Signature;
    /// A function to encrypt hash.
    fn hash() -> Self::Hash;
    /// A function to check signature
    fn check_signature(hash: &Self::Hash, sig: &Self::Signature) -> bool;
    /// A function to signature the message hash.
    fn signature(hash: &Self::Hash, privkey: &[u8]) -> Self::Signature;
}
