//! An efficent and stable Rust library of BFT protocol for distributed system.
//!
//!

#![deny(missing_docs)]

#[macro_use]
extern crate crossbeam;
#[macro_use]
extern crate log;
extern crate lru_cache;
extern crate min_max_heap;
extern crate rand;
extern crate rand_core;
extern crate rand_pcg;
extern crate rlp;
#[macro_use]
extern crate serde_derive;

use crate::{
    algorithm::Step,
    error::BftError,
};
use crypto::{pubkey_to_address, Sign, Signature};
use rlp::{Encodable, RlpStream};
use std::collections::HashMap;
use std::usize::MAX;

/// Bft actuator.
pub mod actuator;
/// BFT state machine.
pub mod algorithm;
///
pub mod error;
/// BFT params include time interval and local address.
pub mod params;

pub mod random;
/// BFT timer.
pub mod timer;
/// BFT vote set.
pub mod voteset;

pub mod wal;

/// Type for node address.
pub type Address = Vec<u8>;
/// Type for block hash.
pub type Hash = Vec<u8>;

#[derive(Debug)]
pub enum BftInput {
    ///
    BftMsg(BftMsg),
    ///
    Status(Status),
    ///
    VerifyResp(VerifyResp),
    ///
    Feed(Feed),
    ///
    Pause,
    ///
    Start,
}

/// BFT input message.
#[derive(Debug)]
pub enum BftMsg {
    /// Proposal message.
    Proposal(Vec<u8>),
    /// Vote message.
    Vote(Vec<u8>),
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
    pub block: Vec<u8>,
    ///
    pub proof: Proof,
    /// A lock round of the proposal.
    pub lock_round: Option<u64>,
    /// The lock votes of the proposal.
    pub lock_votes: Vec<Vote>,
    /// The address of proposer.
    pub proposer: Address,

    pub signature: Signature,
}

impl Encodable for Proposal {
    fn rlp_append(&self, s: &mut RlpStream) {
        let votes = self.lock_votes.clone();
        s.append(&self.height)
            .append(&self.round)
            .append(&self.block)
            .append(&self.proof)
            .append(&self.lock_round);
        for vote in votes.into_iter() {
            s.append(&vote);
        }
        s.append(&self.proposer)
            .append(&self.signature);
    }
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
    pub block_hash: Hash,
    /// The address of voter
    pub voter: Address,
    ///
    pub signature: Signature,
}

impl Encodable for Vote {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.append(&self.vote_type)
            .append(&self.height)
            .append(&self.round)
            .append(&self.block_hash)
            .append(&self.voter)
            .append(&self.signature);
    }
}

/// A proposal for a height.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    /// The height of the proposal.
    pub height: u64,
    /// A proposal.
    pub block: Vec<u8>,
}

/// A result of a height.

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Commit {
    ///
    pub height: u64,
    ///
    pub block: Vec<u8>,
    ///
    pub pre_hash: Hash,
    ///
    pub proof: Proof,
    ///
    pub address: Address,
}

/// Necessary messages for a height.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// The height of rich status.
    pub height: u64,
    ///
    pub pre_hash: Hash,
    /// The time interval of next height. If it is none, maintain the old interval.
    pub interval: Option<u64>,
    /// A new authority list for next height.
    pub authority_list: Vec<Node>,
}

/// A verify result of a proposal.
#[cfg(feature = "verify_req")]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct VerifyResp {
    /// The Response of proposal verify
    pub is_pass: bool,
    /// The verify proposal
    pub block_hash: Hash,
}

#[derive(Serialize, Deserialize, Clone, Debug PartialEq, Eq)]
pub struct Node {
    ///
    pub address: Address,
    ///
    pub proposal_weight: u32,
    ///
    pub vote_weight: u32,
}

impl Node {
    pub fn new(address: Address, proposal_weight: u32, vote_weight: u32) -> Self {
        let mut node = Node {
            address,
            proposal_weight,
            vote_weight,
        };
        authority_manage
    }

    pub fn set_address(address: Address) -> Self {
        Self::new(address, 1, 1)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuthorityManage {
    ///
    pub authorities: Vec<Node>,
    ///
    pub authorities_old: Vec<Node>,
    ///
    pub authority_h_old: u64,
}

impl Default for AuthorityManage {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthorityManage {
    pub fn new() -> Self {
        let mut authority_manage = AuthorityManage {
            authorities: Vec::new(),
            authorities_old: Vec::new(),
            authority_h_old: 0,
        };
        authority_manage
    }

    pub fn receive_authorities_list(
        &mut self,
        height: u64,
        authorities: &[Node],
    ) {
        if self.authorities != authorities {
            self.authorities_old.clear();
            self.authorities_old.extend_from_slice(&self.authorities);
            self.authority_h_old = height;

            self.authorities.clear();
            self.authorities.extend_from_slice(&authorities);
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Proof {
    ///
    pub height: u64,
    ///
    pub round: u64,
    ///
    pub block_hash: Hash,
    ///
    pub precommit_votes: HashMap<Address, Signature>,
}

impl Proof {
    pub fn new(
        height: usize,
        round: usize,
        block_hash: Hash,
        precommit_votes: HashMap<Address, Signature>,
    ) -> BftProof {
        BftProof {
            height,
            round,
            block_hash,
            precommit_votes,
        }
    }

    // Check proof
    pub fn check(&self, height: u64, authorities: &[Node]) -> bool {
        if h == 0 {
            return true;
        }
        if h != self.height {
            return false;
        }
        // TODO: 加上权重
        if 2 * authorities.len() >= 3 * self.precommit_votes.len() {
            return false;
        }
        self.commits.iter().all(|(sender, sig)| {
            if authorities.into().any(|node| node.address == sender) {
                let msg = rlp::encode(
                    &(
                        height,
                        self.round,
                        Step::Precommit,
                        sender,
                        Some(self.block_hash.clone()),
                    )
                );
                let signature = Signature(sig.0.into());
                if let Ok(pubkey) = signature.recover(&msg.crypt_hash().into()) {
                    return pubkey_to_address(&pubkey) == sender.clone().into();
                }
            }
            false
        })
    }
}

/// A PoLC.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LockStatus {
    /// The lock proposal
    pub proposal: Hash,
    /// The lock round
    pub round: u64,
    /// The lock votes.
    pub votes: Vec<Vote>,
}


pub trait BftSupport {
    /// A function to check signature.
    fn check_block(&self, block: &[u8]) -> Result<bool, BftError>;
    /// A funciton to transmit messages.
    fn transmit(&self, msg: BftMsg) -> Result<(), BftError>;
    /// A function to commit the proposal.
    fn commit(&self, commit: Commit) -> Result<(), BftError>;
}

