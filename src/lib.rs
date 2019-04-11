//! An efficent and stable Rust library of BFT protocol for distributed system.
//!
//!

//#![deny(missing_docs)]

#[macro_use]
pub extern crate crossbeam;
#[macro_use]
extern crate log;
extern crate lru_cache;
extern crate min_max_heap;
extern crate rand;
extern crate rand_core;
extern crate rand_pcg;
extern crate rlp;

use crate::{
    algorithm::Bft,
    error::BftError,
    objects::{Vote, VoteType},
};

use rlp::{Encodable, Decodable, DecoderError, Prototype, Rlp, RlpStream};
use std::collections::HashMap;
use std::hash::{Hash as Hashable, Hasher};

/// BFT state machine.
pub mod algorithm;
/// BFT vote set.
pub mod collectors;
///
pub mod error;

pub mod objects;
/// BFT params include time interval and local address.
pub mod params;

pub mod random;
/// BFT timer.
pub mod timer;

pub mod utils;

pub mod wal;

/// Type for node address.
pub type Address = Vec<u8>;
/// Type for block hash.
pub type Hash = Vec<u8>;

pub type Signature = Vec<u8>;

pub type BftResult<T> = ::std::result::Result<T, BftError>;

pub type BftActuator<T> = Bft<T>;

#[derive(Debug)]
pub enum BftMsg {
    Proposal(Vec<u8>),
    Vote(Vec<u8>),
    Status(Status),
    #[cfg(feature = "verify_req")]
    VerifyResp(VerifyResp),
    Feed(Feed),
    Pause,
    Start,
    Snapshot(Snapshot),
}

#[cfg(feature = "verify_req")]
#[derive(Clone, Eq, PartialEq)]
pub enum VerifyResult {
    Approved,
    Failed,
    Undetermined,
}

/// A result of a height.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    ///
    pub height: u64,
    ///
    pub block: Vec<u8>,
    ///
    pub proof: Proof,
    ///
    pub address: Address,
}

impl Default for Commit {
    fn default() -> Self {
        Commit {
            height: 0u64,
            block: vec![],
            proof: Proof::default(),
            address: vec![],
        }
    }
}

impl Encodable for Commit {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(4).append(&self.height)
            .append(&self.block)
            .append(&self.proof)
            .append(&self.address);
    }
}

impl Decodable for Commit {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(4) => {
                let height: u64 = r.val_at(0)?;
                let block: Vec<u8> = r.val_at(1)?;
                let proof: Proof = r.val_at(2)?;
                let address: Address = r.val_at(3)?;
                Ok(Commit{
                    height,
                    block,
                    proof,
                    address,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}

/// Necessary messages for a height.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// The height of rich status.
    pub height: u64,
    /// The time interval of next height. If it is none, maintain the old interval.
    pub interval: Option<u64>,
    /// A new authority list for next height.
    pub authority_list: Vec<Node>,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            height: 0u64,
            interval: None,
            authority_list: vec![],
        }
    }
}

impl Encodable for Status {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(3).append(&self.height)
            .append(&self.interval)
            .append_list(&self.authority_list);
    }
}

impl Decodable for Status {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(3) => {
                let height: u64 = r.val_at(0)?;
                let interval: Option<u64> = r.val_at(1)?;
                let authority_list: Vec<Node> = r.list_at(2)?;
                Ok(Status{
                    height,
                    interval,
                    authority_list,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}

/// A proposal for a height.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    /// The height of the proposal.
    pub height: u64,
    /// A proposal.
    pub block: Vec<u8>,
}

impl Default for Feed {
    fn default() -> Self {
        Feed {
            height: 0u64,
            block: vec![],
        }
    }
}

impl Encodable for Feed {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2).append(&self.height)
            .append(&self.block);
    }
}

impl Decodable for Feed {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(2) => {
                let height: u64 = r.val_at(0)?;
                let block: Vec<u8> = r.val_at(1)?;
                Ok(Feed{
                    height,
                    block,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}

/// A verify result of a proposal.
#[cfg(feature = "verify_req")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyResp {
    /// The Response of proposal verify
    pub is_pass: bool,
    /// The verify proposal
    pub block_hash: Hash,
}

#[cfg(feature = "verify_req")]
impl Default for VerifyResp {
    fn default() -> Self {
        VerifyResp {
            is_pass: false,
            block_hash: vec![],
        }
    }
}

#[cfg(feature = "verify_req")]
impl Encodable for VerifyResp {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2).append(&self.is_pass)
            .append(&self.block_hash);
    }
}

#[cfg(feature = "verify_req")]
impl Decodable for VerifyResp {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(2) => {
                let is_pass: bool = r.val_at(0)?;
                let block_hash: Hash = r.val_at(1)?;
                Ok(VerifyResp{
                    is_pass,
                    block_hash,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub proof: Proof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    ///
    pub address: Address,
    ///
    pub proposal_weight: u32,
    ///
    pub vote_weight: u32,
}

impl Encodable for Node {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(3).append(&self.address)
            .append(&self.proposal_weight)
            .append(&self.vote_weight);
    }
}

impl Decodable for Node {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(3) => {
                let address: Address = r.val_at(0)?;
                let proposal_weight: u32 = r.val_at(1)?;
                let vote_weight: u32 = r.val_at(2)?;
                Ok(Node{
                    address,
                    proposal_weight,
                    vote_weight,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}

impl Node {
    pub fn new(address: Address, proposal_weight: u32, vote_weight: u32) -> Self {
        let node = Node {
            address,
            proposal_weight,
            vote_weight,
        };
        node
    }

    pub fn set_address(address: Address) -> Self {
        Self::new(address, 1, 1)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

impl Default for Proof {
    fn default() -> Self {
        Proof {
            height: 0u64,
            round: 0u64,
            block_hash: vec![],
            precommit_votes: HashMap::new(),
        }
    }
}

impl Hashable for Proof {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.height.hash(state);
        self.round.hash(state);
        self.block_hash.hash(state);
        //TODO: Ignore precommit_votes maybe leaves flaws
    }
}

impl Encodable for Proof {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(5)
            .append(&self.height)
            .append(&self.round)
            .append(&self.block_hash);
        let mut key_list: Vec<Address> = vec![];
        let mut value_list: Vec<Vec<u8>> = vec![];
        self.precommit_votes.iter().for_each(|(address, sig)| {
            key_list.push(address.to_owned());
            value_list.push(sig.to_owned());
        });
        s.begin_list(key_list.len());
        for key in key_list {
            s.append(&key);
        }
        s.begin_list(value_list.len());
        for value in value_list {
            s.append(&value);
        }
    }
}

impl Decodable for Proof {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(5) => {
                let height: u64 = r.val_at(0)?;
                let round: u64 = r.val_at(1)?;
                let block_hash: Vec<u8> = r.val_at(2)?;
                let key_list: Vec<Address> = r.list_at(3)?;
                let value_list: Vec<Signature> = r.list_at(4)?;
                if key_list.len() != value_list.len() {
                    return Err(DecoderError::RlpIncorrectListLen);
                }
                let precommit_votes: HashMap<_, _> = key_list.into_iter().zip(value_list.into_iter()).collect();
                Ok(Proof{
                    height,
                    round,
                    block_hash,
                    precommit_votes,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}


pub trait BftSupport {

    fn start(&mut self);
    /// A function to check signature.
    fn check_block(&self, block: &[u8], height: u64) -> bool;
    /// A function to check signature.
    #[cfg(feature = "verify_req")]
    fn check_transaction(&mut self, block: &[u8], height: u64) -> bool;
    /// A funciton to transmit messages.
    fn transmit(&self, msg: BftMsg);
    /// A function to commit the proposal.
    fn commit(&mut self, commit: Commit);

    fn get_block(&self, height: u64) -> Option<Vec<u8>>;

    fn sign(&self, hash: &[u8]) -> Option<Signature>;

    fn check_sig(&self, signature: &[u8], hash: &[u8]) -> Option<Address>;

    fn crypt_hash(&self, msg: &[u8]) -> Vec<u8>;
}

pub fn check_proof(proof: &Proof, height: u64, authorities: &[Node],
                   crypt_hash: fn(msg: &[u8]) -> Vec<u8>,
                   check_sig: fn(signature: &[u8], hash: &[u8]) -> Option<Address>) -> bool {
    if height == 0 {
        return true;
    }
    if height != proof.height {
        return false;
    }

    let weight: Vec<u64> = authorities.iter().map(|node| node.vote_weight as u64).collect();
    let vote_addresses : Vec<Address> = proof.precommit_votes.iter().map(|(sender, _)| sender.clone()).collect();
    let votes_weight: Vec<u64> = authorities.iter()
        .filter(|node| vote_addresses.contains(&node.address))
        .map(|node| node.vote_weight as u64).collect();
    let weight_sum: u64 = weight.iter().sum();
    let vote_sum: u64 = votes_weight.iter().sum();
    if vote_sum * 3 > weight_sum * 2 {
        return false;
    }

    proof.precommit_votes.iter().all(|(voter, sig)| {
        if authorities.iter().any(|node| node.address == *voter) {
            let vote = Vote{
                vote_type: VoteType::Precommit,
                height,
                round: proof.round,
                block_hash: proof.block_hash.clone(),
                voter: voter.clone(),
            };
            let msg = rlp::encode(&vote);
            if let Some(address) = check_sig(sig, &crypt_hash(&msg)) {
                return address == *voter;
            }
        }
        false
    })
}