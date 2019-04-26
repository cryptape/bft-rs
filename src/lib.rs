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
    error::{BftError, BftResult},
    objects::{Vote, VoteType},
};

use crossbeam::crossbeam_channel::{unbounded, Sender};
use rlp::{Decodable, DecoderError, Encodable, Prototype, Rlp, RlpStream};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::hash::{Hash as Hashable, Hasher};
use std::sync::Arc;

/// The core algorithm of the BFT state machine.
pub mod algorithm;
/// Collectors for proposals and signed_votes.
pub mod collectors;
/// Bft errors defined.
pub mod error;
/// Bft structures only for this crate.
pub mod objects;
/// BFT params include time interval and local address.
pub mod params;
/// Select a proposer Probabilistic according to the proposal_weight of Nodes.
pub mod random;
/// BFT timer.
pub mod timer;
/// The external functions of the BFT state machine.
pub mod utils;
/// Wal
pub mod wal;

/// Type for node address.
pub type Address = Vec<u8>;
/// Type for hash.
pub type Hash = Vec<u8>;
/// Type for signature.
pub type Signature = Vec<u8>;

pub type Block = Vec<u8>;

pub type Encode = Vec<u8>;

pub type Height = u64;

pub type Round = u64;

pub struct BftActuator(Sender<BftMsg>);

impl BftActuator {
    /// A function to create a new Bft actuator and start the BFT state machine.
    pub fn new<T: BftSupport + 'static>(support: Arc<T>, address: Address, wal_path: &str) -> Self {
        let (sender, internal_receiver) = unbounded();
        Bft::start(
            sender.clone(),
            internal_receiver,
            support,
            address,
            wal_path,
        );
        BftActuator(sender)
    }

    /// A function for sending msg to the BFT state machine.
    pub fn send(&self, msg: BftMsg) -> BftResult<()> {
        let info = format!("{:?} by BftActuator", &msg);
        self.0.send(msg).map_err(|_| BftError::SendMsgErr(info))
    }
}

#[derive(Debug, Clone)]
pub enum BftMsg {
    Proposal(Encode),
    Vote(Encode),
    Status(Status),
    #[cfg(feature = "verify_req")]
    VerifyResp(VerifyResp),
    Feed(Feed),
    Pause,
    Start,
    Clear(Proof),
}

#[cfg(feature = "verify_req")]
#[derive(Clone, Eq, PartialEq)]
pub enum VerifyResult {
    Approved,
    Failed,
    Undetermined,
}

/// A reaching consensus result of a giving height.
#[derive(Clone, PartialEq, Eq)]
pub struct Commit {
    /// the commit height
    pub height: Height,
    /// the reaching-consensus block content
    pub block: Block,
    /// a proof for the above block
    pub proof: Proof,
    /// the address of the reaching-consensus proposer
    pub address: Address,
}

impl Debug for Commit {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "Commit {{ height: {}, address: {:?}}}",
            self.height, self.address,
        )
    }
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
        s.begin_list(4)
            .append(&self.height)
            .append(&self.block)
            .append(&self.proof)
            .append(&self.address);
    }
}

impl Decodable for Commit {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(4) => {
                let height: Height = r.val_at(0)?;
                let block: Block = r.val_at(1)?;
                let proof: Proof = r.val_at(2)?;
                let address: Address = r.val_at(3)?;
                Ok(Commit {
                    height,
                    block,
                    proof,
                    address,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

/// The status of a giving height.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// the height of status
    pub height: Height,
    /// time interval of next height. If it is none, maintain the old interval
    pub interval: Option<u64>,
    /// a new authority list for next height
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
        s.begin_list(3)
            .append(&self.height)
            .append(&self.interval)
            .append_list(&self.authority_list);
    }
}

impl Decodable for Status {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(3) => {
                let height: Height = r.val_at(0)?;
                let interval: Option<u64> = r.val_at(1)?;
                let authority_list: Vec<Node> = r.list_at(2)?;
                Ok(Status {
                    height,
                    interval,
                    authority_list,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

/// A feed block for a giving height.
#[derive(Clone, PartialEq, Eq)]
pub struct Feed {
    /// the height of the block
    pub height: Height,
    /// the content of the block
    pub block: Block,
}

impl Debug for Feed {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Feed {{ height: {}}}", self.height)
    }
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
        s.begin_list(2).append(&self.height).append(&self.block);
    }
}

impl Decodable for Feed {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(2) => {
                let height: Height = r.val_at(0)?;
                let block: Block = r.val_at(1)?;
                Ok(Feed { height, block })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

/// The verify result.
#[cfg(feature = "verify_req")]
#[derive(Clone, PartialEq, Eq)]
pub struct VerifyResp {
    /// the response of verification
    pub is_pass: bool,
    /// block hash
    pub round: Round,
}

#[cfg(feature = "verify_req")]
impl Debug for VerifyResp {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "VerifyResp {{ is_pass: {}, round: {:?}}}",
            self.is_pass, self.round,
        )
    }
}

#[cfg(feature = "verify_req")]
impl Default for VerifyResp {
    fn default() -> Self {
        VerifyResp {
            is_pass: false,
            round: 0u64,
        }
    }
}

#[cfg(feature = "verify_req")]
impl Encodable for VerifyResp {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2).append(&self.is_pass).append(&self.round);
    }
}

#[cfg(feature = "verify_req")]
impl Decodable for VerifyResp {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(2) => {
                let is_pass: bool = r.val_at(0)?;
                let round: Round = r.val_at(1)?;
                Ok(VerifyResp { is_pass, round })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

/// The bft Node
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Node {
    /// the address of node
    pub address: Address,
    /// the weight for being a proposer
    pub proposal_weight: u32,
    /// the weight for calculating vote
    pub vote_weight: u32,
}

impl Debug for Node {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "Node {{ address: {:?}, weight: {}/{}}}",
            self.address, self.proposal_weight, self.vote_weight,
        )
    }
}

impl Encodable for Node {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(3)
            .append(&self.address)
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
                Ok(Node {
                    address,
                    proposal_weight,
                    vote_weight,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
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

/// Proof
#[derive(Clone, Eq, PartialEq)]
pub struct Proof {
    /// proof height
    pub height: Height,
    /// the round reaching consensus
    pub round: Round,
    /// the block_hash reaching consensus
    pub block_hash: Hash,
    /// the voters and corresponding signatures
    pub precommit_votes: HashMap<Address, Signature>,
}

impl Debug for Proof {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "Proof {{ height: {}, round: {}, block_hash: {:?}}}",
            self.height, self.round, self.block_hash,
        )
    }
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
        let mut key_values: Vec<(Address, Vec<u8>)> =
            self.precommit_votes.clone().into_iter().collect();
        key_values.sort();
        let mut key_list: Vec<Address> = vec![];
        let mut value_list: Vec<Vec<u8>> = vec![];
        key_values.iter().for_each(|(address, sig)| {
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
                let height: Height = r.val_at(0)?;
                let round: Round = r.val_at(1)?;
                let block_hash: Hash = r.val_at(2)?;
                let key_list: Vec<Address> = r.list_at(3)?;
                let value_list: Vec<Signature> = r.list_at(4)?;
                if key_list.len() != value_list.len() {
                    error!(
                        "Bft decode proof error, key_list_len {}, value_list_len{}",
                        key_list.len(),
                        value_list.len()
                    );
                    return Err(DecoderError::RlpIncorrectListLen);
                }
                let precommit_votes: HashMap<_, _> =
                    key_list.into_iter().zip(value_list.into_iter()).collect();
                Ok(Proof {
                    height,
                    round,
                    block_hash,
                    precommit_votes,
                })
            }
            _ => {
                error!(
                    "Bft decode proof error, the prototype is {:?}",
                    r.prototype()
                );
                Err(DecoderError::RlpInconsistentLengthAndData)
            }
        }
    }
}

/// User-defined functions.
pub trait BftSupport: Sync + Send {
    type Error: ::std::fmt::Debug;
    /// A user-defined function for block validation.
    /// Every block bft received will call this function, even if the feed block.
    /// Users should validate block format, block headers here.
    fn check_block(&self, block: &[u8], height: u64) -> Result<(), Self::Error>;
    /// A user-defined function for transactions validation.
    /// Every block bft received will call this function, even if the feed block.
    /// Users should validate transactions here.
    /// The [`proposal_hash`] is corresponding to the proposal of the [`proposal_hash`].
    fn check_txs(
        &self,
        block: &[u8],
        signed_proposal_hash: &[u8],
        height: u64,
        round: u64,
    ) -> Result<(), Self::Error>;
    /// A user-defined function for transmitting signed_proposals and signed_votes.
    /// The signed_proposals and signed_votes have been serialized,
    /// users do not have to care about the structure of Proposal and Vote.
    fn transmit(&self, msg: BftMsg);
    /// A user-defined function for processing the reaching-consensus block.
    /// Users could execute the block inside and add it into chain.
    /// The height of proof inside the commit equals to block height.
    fn commit(&self, commit: Commit) -> Result<Status, Self::Error>;
    /// A user-defined function for feeding the bft consensus.
    /// The new block provided will feed for bft consensus of giving [`height`]
    fn get_block(&self, height: u64) -> Result<Vec<u8>, Self::Error>;
    /// A user-defined function for signing a [`hash`].
    fn sign(&self, hash: &[u8]) -> Result<Signature, Self::Error>;
    /// A user-defined function for checking a [`signature`].
    fn check_sig(&self, signature: &[u8], hash: &[u8]) -> Result<Address, Self::Error>;
    /// A user-defined function for hashing a [`msg`].
    fn crypt_hash(&self, msg: &[u8]) -> Vec<u8>;
}

/// A public function for proof validation.
/// The input [`height`] is the height of block involving the proof.
/// The input [`authorities`] is the authority_list for the proof.
/// The fn [`crypt_hash`], [`check_sig`] are user-defined.
/// signature of the functions:
/// - [`crypt_hash`]: `fn(msg: &[u8]) -> Vec<u8>`
/// - [`check_sig`]:  `fn(signature: &[u8], hash: &[u8]) -> Option<Address>`
pub fn check_proof(
    proof: &Proof,
    height: u64,
    authorities: &[Node],
    crypt_hash: impl Fn(&[u8]) -> Vec<u8>,
    check_sig: impl Fn(&[u8], &[u8]) -> Option<Address>,
) -> bool {
    if proof.height == 0 {
        return true;
    }
    if height != proof.height + 1 {
        return false;
    }

    let weight: Vec<u64> = authorities
        .iter()
        .map(|node| node.vote_weight as u64)
        .collect();
    let vote_addresses: Vec<Address> = proof
        .precommit_votes
        .iter()
        .map(|(sender, _)| sender.clone())
        .collect();
    let votes_weight: Vec<u64> = authorities
        .iter()
        .filter(|node| vote_addresses.contains(&node.address))
        .map(|node| node.vote_weight as u64)
        .collect();
    let weight_sum: u64 = weight.iter().sum();
    let vote_sum: u64 = votes_weight.iter().sum();
    if vote_sum * 3 <= weight_sum * 2 {
        return false;
    }

    proof.precommit_votes.iter().all(|(voter, sig)| {
        if authorities.iter().any(|node| node.address == *voter) {
            let vote = Vote {
                vote_type: VoteType::Precommit,
                height: proof.height,
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
