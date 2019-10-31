use crate::*;
use rlp::{Decodable, DecoderError, Encodable, Prototype, Rlp, RlpStream};
use std::fmt::{Debug, Formatter, Result as FmtResult};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct Proposal {
    /// the height of proposal
    pub(crate) height: Height,
    /// the round of proposal
    pub(crate) round: Round,
    /// block hash
    pub(crate) block_hash: Hash,
    /// the proof of previous height
    pub(crate) proof: Proof,
    /// the lock round of the proposal
    pub(crate) lock_round: Option<Round>,
    /// the lock votes of the proposal
    pub(crate) lock_votes: Vec<SignedVote>,
    /// proposer address
    pub(crate) proposer: Address,
}

impl Debug for Proposal {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "Proposal {{ h: {}, r: {}, hash:{:?}, addr: {:?}, proof: {:?}, lock_round: {:?}, proposer: {:?}}}",
            self.height, self.round, self.block_hash, self.proposer, self.proof, self.lock_round, self.proposer
        )
    }
}

impl Encodable for Proposal {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(7)
            .append(&self.height)
            .append(&self.round)
            .append(&self.block_hash)
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
                let height: Height = r.val_at(0)?;
                let round: Round = r.val_at(1)?;
                let block_hash: Hash = r.val_at(2)?;
                let proof: Proof = r.val_at(3)?;
                let lock_round: Option<Round> = r.val_at(4)?;
                let lock_votes: Vec<SignedVote> = r.list_at(5)?;
                let proposer: Address = r.val_at(6)?;
                Ok(Proposal {
                    height,
                    round,
                    block_hash,
                    proof,
                    lock_round,
                    lock_votes,
                    proposer,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct SignedProposal {
    pub(crate) proposal: Proposal,
    pub(crate) signature: Signature,
}

impl Debug for SignedProposal {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "SignedProposal {{ proposal: {:?}, sig: {:?}}}",
            self.proposal, self.signature,
        )
    }
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
                Ok(SignedProposal {
                    proposal,
                    signature,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

/// A vote to a proposal.
#[derive(Clone, Eq, PartialEq, Hash)]
pub(crate) struct Vote {
    /// Prevote or precommit
    pub(crate) vote_type: VoteType,
    /// the height of vote
    pub(crate) height: Height,
    /// the round of vote
    pub(crate) round: Round,
    /// the content vote for
    pub(crate) block_hash: Hash,
    /// voter address
    pub(crate) voter: Address,
}

impl Debug for Vote {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "{:?} {{ h: {}, r: {}, hash: {:?}, addr: {:?}}}",
            self.vote_type, self.height, self.round, self.block_hash, self.voter,
        )
    }
}

impl Encodable for Vote {
    fn rlp_append(&self, s: &mut RlpStream) {
        let vote_type: u8 = self.vote_type.clone().into();
        s.begin_list(5)
            .append(&vote_type)
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
                let height: Height = r.val_at(1)?;
                let round: Round = r.val_at(2)?;
                let block_hash: Hash = r.val_at(3)?;
                let voter: Address = r.val_at(4)?;
                Ok(Vote {
                    vote_type,
                    height,
                    round,
                    block_hash,
                    voter,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub(crate) struct SignedVote {
    pub(crate) vote: Vote,
    pub(crate) signature: Signature,
}

impl Debug for SignedVote {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "SignedVote {{ vote: {:?}, sig: {:?}}}",
            self.vote, self.signature,
        )
    }
}

impl Encodable for SignedVote {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2).append(&self.vote).append(&self.signature);
    }
}

impl Decodable for SignedVote {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(2) => {
                let vote: Vote = r.val_at(0)?;
                let signature: Signature = r.val_at(1)?;
                Ok(SignedVote { vote, signature })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

/// A PoLC.
#[derive(Clone, Debug)]
pub(crate) struct LockStatus {
    pub(crate) block_hash: Hash,
    pub(crate) round: Round,
    pub(crate) votes: Vec<SignedVote>,
}

#[derive(Clone, Debug)]
pub(crate) struct AuthorityManage {
    pub(crate) authorities: Vec<Node>,
    pub(crate) authorities_old: Vec<Node>,
    pub(crate) authority_h_old: Height,
}

impl AuthorityManage {
    pub(crate) fn new() -> Self {
        AuthorityManage {
            authorities: Vec::new(),
            authorities_old: Vec::new(),
            authority_h_old: 0,
        }
    }

    pub(crate) fn receive_authorities_list(&mut self, height: Height, mut authorities: Vec<Node>) {
        authorities.sort();

        if self.authorities != authorities {
            self.authorities_old.clear();
            self.authorities_old.extend_from_slice(&self.authorities);
            self.authority_h_old = height;

            self.authorities.clear();
            self.authorities.extend_from_slice(&authorities);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.authorities.is_empty()
    }
}

impl Encodable for AuthorityManage {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(3)
            .append_list(&self.authorities)
            .append_list(&self.authorities_old)
            .append(&self.authority_h_old);
    }
}

impl Decodable for AuthorityManage {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(3) => {
                let authorities = r.list_at(0)?;
                let authorities_old = r.list_at(1)?;
                let authority_h_old = r.val_at(2)?;
                Ok(AuthorityManage {
                    authorities,
                    authorities_old,
                    authority_h_old,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Clone, Copy, Hash)]
pub(crate) enum Step {
    Propose,
    ProposeWait,
    Prevote,
    PrevoteWait,
    #[cfg(feature = "verify_req")]
    VerifyWait,
    Precommit,
    PrecommitWait,
    Commit,
    CommitWait,
}

impl Default for Step {
    fn default() -> Step {
        Step::Propose
    }
}

impl From<u8> for Step {
    fn from(s: u8) -> Self {
        match s {
            0 => Step::Propose,
            1 => Step::ProposeWait,
            2 => Step::Prevote,
            3 => Step::PrevoteWait,
            #[cfg(feature = "verify_req")]
            4 => Step::VerifyWait,
            5 => Step::Precommit,
            6 => Step::PrecommitWait,
            7 => Step::Commit,
            8 => Step::CommitWait,
            _ => panic!("Invalid vote type!"),
        }
    }
}

impl Into<u8> for Step {
    fn into(self) -> u8 {
        match self {
            Step::Propose => 0,
            Step::ProposeWait => 1,
            Step::Prevote => 2,
            Step::PrevoteWait => 3,
            #[cfg(feature = "verify_req")]
            Step::VerifyWait => 4,
            Step::Precommit => 5,
            Step::PrecommitWait => 6,
            Step::Commit => 7,
            Step::CommitWait => 8,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) enum VoteType {
    Prevote,
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

#[derive(Debug)]
pub(crate) enum LogType {
    Proposal,
    Vote,
    Status,
    Proof,
    Feed,
    VerifyResp,
    TimeOutInfo,
    Block,
    Authorities,
}

impl From<u8> for LogType {
    fn from(s: u8) -> Self {
        match s {
            0 => LogType::Proposal,
            1 => LogType::Vote,
            2 => LogType::Status,
            3 => LogType::Proof,
            4 => LogType::Feed,
            5 => LogType::VerifyResp,
            6 => LogType::TimeOutInfo,
            7 => LogType::Block,
            8 => LogType::Authorities,
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
            LogType::Proof => 3,
            LogType::Feed => 4,
            LogType::VerifyResp => 5,
            LogType::TimeOutInfo => 6,
            LogType::Block => 7,
            LogType::Authorities => 8,
        }
    }
}

#[derive(Debug)]
pub(crate) enum PrecommitRes {
    Above,
    Below,
    Nil,
    Proposal,
}
