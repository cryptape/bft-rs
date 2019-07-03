//! An efficent and stable Rust library of BFT protocol for distributed system.
use crate::{
    algorithm::Bft,
    error::{BftError, BftResult},
    objects::{Vote, VoteType},
    utils::{get_total_weight, get_votes_weight},
};

use crate::utils::extract_two;
use crossbeam::crossbeam_channel::{unbounded, Sender};
use hex_fmt::HexFmt;
#[allow(unused_imports)]
use log::{debug, error, info, log, trace};
use rlp::{Decodable, DecoderError, Encodable, Prototype, Rlp, RlpStream};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::hash::{Hash as Hashable, Hasher};
use std::ops::Deref;
use std::sync::Arc;

/// Define the core functions of the BFT state machine.
pub mod algorithm;
/// Define simple byzantine behaviors.
pub mod byzantine;
/// Define collectors of blocks, signed_proposals and signed_votes.
pub mod collectors;
/// Define errors.
pub mod error;
/// Define structures only for this crate, including Proposal, Vote, Step.
pub mod objects;
/// Define params including time interval and local address.
pub mod params;
/// Define a timeout structure and the timer process.
pub mod timer;
/// Define utils of the BFT state machine.
pub mod utils;
/// Define wal support.
pub mod wal;

/// Define the structure of the node address.
#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Address(Vec<u8>);
/// Define the structure of the hash.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Hash(Vec<u8>);
/// Define the structure of the signature.
#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Signature(Vec<u8>);
/// Define the structure of the block.
/// It is the consensus content, which should be serialized and wrapped.
#[derive(Clone, Eq, PartialEq)]
pub struct Block(Vec<u8>);

macro_rules! impl_traits_for_vecu8_wraper {
    ($name: ident) => {
        impl $name {
            pub fn to_vec(&self) -> Vec<u8> {
                self.0.clone()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                $name(vec![])
            }
        }

        impl Deref for $name {
            type Target = Vec<u8>;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl Encodable for $name {
            fn rlp_append(&self, s: &mut RlpStream) {
                s.begin_list(1).append(&self.0);
            }
        }

        impl Decodable for $name {
            fn decode(r: &Rlp) -> Result<Self, DecoderError> {
                match r.prototype()? {
                    Prototype::List(1) => {
                        let v: Vec<u8> = r.val_at(0)?;
                        Ok($name(v))
                    }
                    _ => Err(DecoderError::RlpInconsistentLengthAndData),
                }
            }
        }

        impl From<Vec<u8>> for $name {
            fn from(v: Vec<u8>) -> Self {
                $name(v)
            }
        }

        impl From<&[u8]> for $name {
            fn from(v: &[u8]) -> Self {
                $name(v.to_vec())
            }
        }

        impl Debug for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                write!(f, "{:?}", &format!("{:<10}", HexFmt(&self.0)),)
            }
        }
    };
}

impl_traits_for_vecu8_wraper!(Address);
impl_traits_for_vecu8_wraper!(Hash);
impl_traits_for_vecu8_wraper!(Signature);
impl_traits_for_vecu8_wraper!(Block);

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
    Proposal(Vec<u8>),
    Vote(Vec<u8>),
    Status(Status),
    VerifyResp(VerifyResp),
    Feed(Feed),

    Pause,
    Start,
    Clear(Proof),

    Kill,
    Corrupt,
}

#[cfg(feature = "verify_req")]
#[derive(Clone, Eq, PartialEq)]
pub enum VerifyResult {
    Approved,
    Failed,
    Undetermined,
}

/// A reaching consensus result of a giving height.
/// It will send outside for block execution, and the current proof should be persisted for sync process.
#[derive(Clone, PartialEq, Eq)]
pub struct Commit {
    /// the commit height
    pub height: Height,
    /// the reaching-consensus block content, which contains a proof of previous height.
    pub block: Block,
    /// the proof of current height
    pub proof: Proof,
    /// the proposer address
    pub address: Address,
}

impl Debug for Commit {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "Commit {{ h: {}, addr: {:?}}}",
            self.height, self.address,
        )
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
/// It should be served from outside after block execution.
/// After receiving status of a specified height, the consensus of next height will start immediately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// the height of status
    pub height: Height,
    /// time interval of next height. If it is none, maintain the old interval
    pub interval: Option<u64>,
    /// a new authority list for next height
    pub authority_list: Vec<Node>,
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
/// It should be served from outside and supply as consensus content.
#[derive(Clone, PartialEq, Eq)]
pub struct Feed {
    /// the height of the block
    pub height: Height,
    /// the content of the block
    pub block: Block,
    /// the hash of the block
    pub block_hash: Hash,
}

impl Debug for Feed {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Feed {{ h: {}}}", self.height)
    }
}

impl Encodable for Feed {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(3)
            .append(&self.height)
            .append(&self.block)
            .append(&self.block_hash);
    }
}

impl Decodable for Feed {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(3) => {
                let height: Height = r.val_at(0)?;
                let block: Block = r.val_at(1)?;
                let block_hash: Hash = r.val_at(2)?;
                Ok(Feed {
                    height,
                    block,
                    block_hash,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

/// The result of block verification.
#[derive(Clone, PartialEq, Eq)]
pub struct VerifyResp {
    /// the result of block verification.
    pub is_pass: bool,
    /// the round of proposal which contains the block
    pub round: Round,
    #[cfg(feature = "compact_block")]
    /// the block with complete transactions.
    pub complete_block: Block,
}

impl Debug for VerifyResp {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "VerifyResp {{ pass: {}, round: {}}}",
            self.is_pass, self.round
        )
    }
}

impl Encodable for VerifyResp {
    fn rlp_append(&self, s: &mut RlpStream) {
        #[cfg(not(feature = "compact_block"))]
        s.begin_list(2).append(&self.is_pass).append(&self.round);

        #[cfg(feature = "compact_block")]
        s.begin_list(3)
            .append(&self.is_pass)
            .append(&self.round)
            .append(&self.complete_block);
    }
}

impl Decodable for VerifyResp {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            #[cfg(not(feature = "compact_block"))]
            Prototype::List(2) => {
                let is_pass: bool = r.val_at(0)?;
                let round: Round = r.val_at(1)?;
                Ok(VerifyResp { is_pass, round })
            }
            #[cfg(feature = "compact_block")]
            Prototype::List(3) => {
                let is_pass: bool = r.val_at(0)?;
                let round: Round = r.val_at(1)?;
                let complete_block: Block = r.val_at(2)?;
                Ok(VerifyResp {
                    is_pass,
                    round,
                    complete_block,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

/// The bft node
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Node {
    /// the address of the node
    pub address: Address,
    /// the weight of being a proposer
    pub proposal_weight: u32,
    /// the weight of calculating vote
    pub vote_weight: u32,
}

impl Debug for Node {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "Node {{ addr: {:?}, w: {}/{}}}",
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
        Node {
            address,
            proposal_weight,
            vote_weight,
        }
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
    /// the reaching-consensus round
    pub round: Round,
    /// the reaching-consensus block hash
    pub block_hash: Hash,
    /// the voters and corresponding signatures
    pub precommit_votes: HashMap<Address, Signature>,
}

impl Debug for Proof {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "Proof {{ h: {}, r: {}, hash: {:?}}}",
            self.height, self.round, self.block_hash,
        )
    }
}

impl Default for Proof {
    fn default() -> Self {
        Proof {
            height: 0,
            round: 0,
            block_hash: Hash::default(),
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
        let mut key_values: Vec<(Address, Signature)> =
            self.precommit_votes.clone().into_iter().collect();
        key_values.sort();
        let mut key_list: Vec<Address> = vec![];
        let mut value_list: Vec<Signature> = vec![];
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
                        "Decode proof error, key_list_len {}, value_list_len{}",
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
                error!("Decode proof error, the prototype is {:?}", r.prototype());
                Err(DecoderError::RlpInconsistentLengthAndData)
            }
        }
    }
}

/// User-defined functions.
pub trait BftSupport: Sync + Send {
    type Error: ::std::fmt::Debug;
    /// A user-defined function for block validation.
    /// Every proposal bft received will call this function, even if the feed block.
    /// Users should validate block format, block headers, transactions here.
    /// The [`signed_proposal_hash`] is corresponding to the proposal of the [`block`].
    fn check_block(
        &self,
        block: &Block,
        block_hash: &Hash,
        signed_proposal_hash: &Hash,
        height_round: (Height, Round),
        is_lock: bool,
        proposer: &Address,
    ) -> Result<VerifyResp, Self::Error>;
    /// A user-defined function for transmitting signed_proposals and signed_votes.
    /// The signed_proposals and signed_votes have been serialized,
    /// users do not have to care about the structure of SignedProposal and SignedVote.
    fn transmit(&self, msg: BftMsg);
    /// A user-defined function for processing the reaching-consensus block.
    /// Users can execute the block and add it into chain.
    fn commit(&self, commit: Commit) -> Result<Status, Self::Error>;
    /// A user-defined function for feeding the bft consensus.
    /// The new block provided will feed for bft consensus of giving [`height`]
    fn get_block(&self, height: Height, proof: &Proof) -> Result<(Block, Hash), Self::Error>;
    /// A user-defined function for signing a [`hash`].
    fn sign(&self, hash: &Hash) -> Result<Signature, Self::Error>;
    /// A user-defined function for checking a [`signature`].
    fn check_sig(&self, signature: &Signature, hash: &Hash) -> Result<Address, Self::Error>;
    /// A user-defined function for hashing a [`msg`].
    fn crypt_hash(&self, msg: &[u8]) -> Hash;
}

/// A public function for proof validation.
/// The input [`height`] is the height of block containing the proof.
/// The input [`authorities`] is the authority_list for the proof check.
/// The fn [`crypt_hash`], [`check_sig`] are user-defined.
pub fn check_proof(
    proof: &Proof,
    height: Height,
    authorities: &[Node],
    crypt_hash: impl Fn(&[u8]) -> Hash,
    check_sig: impl Fn(&Signature, &Hash) -> Option<Address>,
) -> bool {
    if proof.height == 0 {
        return true;
    }
    if height != proof.height + 1 {
        return false;
    }

    let vote_addresses: Vec<Address> = proof
        .precommit_votes
        .iter()
        .map(|(sender, _)| sender.clone())
        .collect();

    if get_votes_weight(authorities, &vote_addresses) * 3 <= get_total_weight(authorities) * 2 {
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

/// A public function for get_proposal_hash from BftMsg::Proposal
pub fn get_proposal_hash(encode: &[u8], crypt_hash: impl Fn(&[u8]) -> Hash) -> Option<Hash> {
    if let Ok((signed_proposal_encode, _)) = extract_two(&encode) {
        Some(crypt_hash(signed_proposal_encode))
    } else {
        None
    }
}
