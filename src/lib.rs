//! An efficent and stable Rust library of BFT protocol for distributed system.
//!
//!

//#![deny(missing_docs)]

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
    error::BftError,
};

use rlp::{Encodable, Decodable, DecoderError, Prototype, Rlp, RlpStream};
use std::collections::HashMap;
use std::hash::{Hash as Hashable, Hasher};

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

pub type Signature = Vec<u8>;

pub type BftResult<T> = ::std::result::Result<T, BftError>;

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
}

/// Bft vote types.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
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

#[cfg(feature = "verify_req")]
enum CheckResp {
    PartialPass,
    CompletePass,
    NoPass,
}

#[derive(Debug)]
pub enum LogType {
    Proposal,
    Vote,
    Status,
    Feed,
    VerifyResp,
    Commit,
    Pause,
    Start,
}

impl From<u8> for LogType {
    fn from(s: u8) -> Self {
        match s {
            0 => LogType::Proposal,
            1 => LogType::Vote,
            2 => LogType::Status,
            3 => LogType::Feed,
            4 => LogType::VerifyResp,
            5 => LogType::Commit,
            6 => LogType::Pause,
            7 => LogType::Start,
            _ => panic!("Invalid vote type!"),
        }
    }
}

impl Into<u8> for LogType {
    fn into(self) -> u8 {
        match self {
            LogType::Proposal => 0,
            LogType::Vote => 1,
            LogType::Status => 2,
            LogType::Feed => 3,
            LogType::VerifyResp => 4,
            LogType::Commit => 5,
            LogType::Pause => 6,
            LogType::Start => 7,
        }
    }
}


#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    pub lock_votes: Vec<SignedVote>,
    /// The address of proposer.
    pub proposer: Address,
}

impl Encodable for Proposal {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(7)
            .append(&self.height)
            .append(&self.round)
            .append(&self.block)
            .append(&self.proof)
            .append(&self.lock_round)
            .append_list(&self.lock_votes)
            .append(&self.proposer);
    }
}

impl Decodable for Proposal {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(7) => {
                let height: u64 = r.val_at(0)?;
                let round: u64 = r.val_at(1)?;
                let block: Vec<u8> = r.val_at(2)?;
                let proof: Proof = r.val_at(3)?;
                let lock_round: Option<u64> = r.val_at(4)?;
                let lock_votes: Vec<SignedVote> = r.list_at(5)?;
                let proposer: Vec<u8> = r.val_at(6)?;
                Ok(Proposal{
                    height,
                    round,
                    block,
                    proof,
                    lock_round,
                    lock_votes,
                    proposer,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}

/// Something need to be consensus in a round.
/// A `Proposal` includes `height`, `round`, `content`, `lock_round`, `lock_votes`
/// and `proposer`. `lock_round` and `lock_votes` are `Option`, means the PoLC of
/// the proposal. Therefore, these must have same variant of `Option`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SignedProposal {
    /// The height of proposal.
    pub proposal: Proposal,

    pub signature: Signature,
}

impl Encodable for SignedProposal {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2)
            .append(&self.proposal)
            .append(&self.signature);
    }
}

impl Decodable for SignedProposal {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(2) => {
                let proposal: Proposal = r.val_at(0)?;
                let signature: Signature = r.val_at(1)?;
                Ok(SignedProposal{
                    proposal,
                    signature,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}

/// A vote to a proposal.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
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
}

impl Encodable for Vote {
    fn rlp_append(&self, s: &mut RlpStream) {
        let vote_type: u8 = self.vote_type.clone().into();
        s.begin_list(5).append(&vote_type)
            .append(&self.height)
            .append(&self.round)
            .append(&self.block_hash)
            .append(&self.voter);
    }
}

impl Decodable for Vote {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(5) => {
                let vote_type: u8 = r.val_at(0)?;
                let vote_type: VoteType = VoteType::from(vote_type);
                let height: u64 = r.val_at(1)?;
                let round: u64 = r.val_at(2)?;
                let block_hash: Vec<u8> = r.val_at(3)?;
                let voter: Address = r.val_at(4)?;
                Ok(Vote{
                    vote_type,
                    height,
                    round,
                    block_hash,
                    voter,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData)
        }
    }
}

/// A vote to a proposal.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SignedVote {
    /// Prevote vote or precommit vote
    pub vote: Vote,
    ///
    pub signature: Signature,
}

impl Encodable for SignedVote {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2).append(&self.vote)
            .append(&self.signature);
    }
}

impl Decodable for SignedVote {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(2) => {
                let vote: Vote = r.val_at(0)?;
                let signature: Signature = r.val_at(1)?;
                Ok(SignedVote{
                    vote,
                    signature,
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

/// A PoLC.
#[derive(Clone, Debug)]
pub struct LockStatus {
    /// The lock proposal
    pub block_hash: Hash,
    /// The lock round
    pub round: u64,
    /// The lock votes.
    pub votes: Vec<SignedVote>,
}


#[derive(Clone, Debug)]
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
        let authority_manage = AuthorityManage {
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


pub trait BftSupport {
    /// A function to check signature.
    #[cfg(not(feature = "verify_req"))]
    fn check_block(&self, block: &[u8], height: u64) -> bool;
    /// A function to check signature.
    #[cfg(feature = "verify_req")]
    fn check_block(&self, block: &[u8], height: u64) -> CheckResp;
    /// A funciton to transmit messages.
    fn transmit(&self, msg: BftMsg);
    /// A function to commit the proposal.
    fn commit(&self, commit: Commit);

    fn get_block(&self, height: u64);

    fn sign(&self, hash: &[u8]) -> Option<Signature>;

    fn check_signature(&self, signature: &[u8], hash: &[u8]) -> Option<Address>;

    fn crypt_hash(&self, msg: &[u8]) -> Vec<u8>;
}

pub fn check_proof(proof: &Proof, height: u64, authorities: &[Node],
                   crypt_hash: fn(msg: &[u8]) -> Vec<u8>,
                   check_signature: fn(signature: &[u8], hash: &[u8]) -> Option<Address>) -> bool {
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
            if let Some(address) = check_signature(sig, &crypt_hash(&msg)) {
                return address == *voter;
            }
        }
        false
    })
}



#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_proof_rlp() {
        let address_1 = vec![87u8, 9u8, 17u8];
        let address_2 = vec![84u8, 91u8, 17u8];
        let signature_1 = vec![23u8, 32u8, 11u8, 21u8, 9u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,];
        let signature_2 = vec![23u8, 32u8, 11u8, 21u8, 9u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,];
        let mut precommit_votes: HashMap<Address, Signature> = HashMap::new();
        precommit_votes.entry(address_1).or_insert(signature_1);
        precommit_votes.entry(address_2).or_insert(signature_2);
        let proof = Proof{
            height: 1888787u64,
            round: 23u64,
            block_hash: vec![10u8, 90u8, 23u8, 65u8],
            precommit_votes,
        };
        let encode = rlp::encode(&proof);
        let decode: Proof = rlp::decode(&encode).unwrap();
        assert_eq!(proof, decode);
    }

    #[test]
    fn test_vote_rlp() {
        let vote = Vote{
            vote_type: VoteType::Prevote,
            height:10u64,
            round: 2u64,
            block_hash: vec![76u8, 8u8],
            voter: vec![76u8, 8u8],
        };

        let signed_vote = SignedVote{
            vote,
            signature: vec![23u8, 32u8, 11u8, 21u8, 9u8,
                            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,],
        };
        let encode = rlp::encode(&signed_vote);
        let decode: SignedVote = rlp::decode(&encode).unwrap();
        assert_eq!(signed_vote, decode);
    }

    #[test]
    fn test_proposal_rlp() {
        let address_1 = vec![87u8, 9u8, 17u8];
        let address_2 = vec![84u8, 91u8, 17u8];
        let signature_1 = vec![23u8, 32u8, 11u8, 21u8, 9u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,];
        let signature_2 = vec![23u8, 32u8, 11u8, 21u8, 9u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,];
        let mut precommit_votes: HashMap<Address, Signature> = HashMap::new();
        precommit_votes.entry(address_1).or_insert(signature_1);
        precommit_votes.entry(address_2).or_insert(signature_2);
        let proposal = Proposal{
            height: 787655u64,
            round: 2u64,
            block: vec![76u8, 9u8, 12u8],
            proof: Proof{
                height: 1888787u64,
                round: 23u64,
                block_hash: vec![10u8, 90u8, 23u8, 65u8],
                precommit_votes,
            },
            lock_round: Some(1u64),
            lock_votes: vec![SignedVote{
                vote:Vote{
                    vote_type: VoteType::Prevote,
                    height:10u64,
                    round: 2u64,
                    block_hash: vec![76u8, 8u8],
                    voter: vec![76u8, 8u8],
                },
                signature: vec![23u8, 32u8, 11u8, 21u8, 9u8,
                    23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                    23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                    23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                    23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                    23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,],
            }],
            proposer: vec![10u8, 90u8, 23u8, 65u8],
        };

        let signed_proposal = SignedProposal{
            proposal,
            signature: vec![23u8, 32u8, 11u8, 21u8, 9u8,
                23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,],
        };

        let encode = rlp::encode(&signed_proposal);
        let decode: SignedProposal = rlp::decode(&encode).unwrap();
        assert_eq!(signed_proposal, decode);
    }

    #[test]
    fn test_feed_rlp() {
        let feed = Feed{
            height: 8797888u64,
            block: vec![89u8, 12u8, 32u8],
        };
        let encode = rlp::encode(&feed);
        let decode: Feed = rlp::decode(&encode).unwrap();
        assert_eq!(feed, decode);
    }

    #[test]
    fn test_commit_rlp() {
        let address_1 = vec![87u8, 9u8, 17u8];
        let address_2 = vec![84u8, 91u8, 17u8];
        let signature_1 = vec![23u8, 32u8, 11u8, 21u8, 9u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,];
        let signature_2 = vec![23u8, 32u8, 11u8, 21u8, 9u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            23u8, 32u8, 11u8, 21u8, 9u8, 10u8,23u8, 32u8, 11u8, 21u8, 9u8, 10u8,];
        let mut precommit_votes: HashMap<Address, Signature> = HashMap::new();
        precommit_votes.entry(address_1).or_insert(signature_1);
        precommit_votes.entry(address_2).or_insert(signature_2);
        let commit = Commit{
            height: 878655u64,
            block: vec![87u8, 23u8],
            proof: Proof{
                height: 1888787u64,
                round: 23u64,
                block_hash: vec![10u8, 90u8, 23u8, 65u8],
                precommit_votes,
            },
            address: vec![87u8, 23u8],
        };
        let encode = rlp::encode(&commit);
        let decode: Commit = rlp::decode(&encode).unwrap();
        assert_eq!(commit, decode);
    }

    #[test]
    fn test_node_rlp() {
        let node = Node{
            address: vec![99u8, 12u8],
            proposal_weight: 43u32,
            vote_weight: 98u32,
        };
        let encode = rlp::encode(&node);
        let decode: Node = rlp::decode(&encode).unwrap();
        assert_eq!(node, decode);
    }

    #[test]
    fn test_status_rlp() {
        let node_1 = Node{
            address: vec![99u8, 12u8],
            proposal_weight: 43u32,
            vote_weight: 98u32,
        };
        let node_2 = Node{
            address: vec![99u8, 12u8],
            proposal_weight: 43u32,
            vote_weight: 98u32,
        };
        let status = Status{
            height: 7556u64,
            interval: Some(6677u64),
            authority_list: vec![node_1, node_2],
        };

        let encode = rlp::encode(&status);
        let decode: Status = rlp::decode(&encode).unwrap();
        assert_eq!(status, decode);
    }

    #[test]
    #[cfg(feature = "verify_req")]
    fn test_verify_resp_rlp() {
        let verify_resp = VerifyResp{
            is_pass: false,
            block_hash: vec![99u8, 12u8],
        };

        let encode = rlp::encode(&verify_resp);
        let decode: VerifyResp = rlp::decode(&encode).unwrap();
        assert_eq!(verify_resp, decode);
    }
}