// CITA
// Copyright 2016-2017 Cryptape Technologies LLC.

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
use crate::*;
use rlp::{Decodable, DecoderError, Encodable, Prototype, Rlp, RlpStream};
use std::fmt::{Debug, Formatter, Result as FmtResult};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct Proposal {
    /// The height of proposal.
    pub(crate) height: Height,
    /// The round of proposal.
    pub(crate) round: Round,
    /// The block hash.
    pub(crate) block_hash: Hash,
    ///
    pub(crate) proof: Proof,
    /// A lock round of the proposal.
    pub(crate) lock_round: Option<Round>,
    /// The lock votes of the proposal.
    pub(crate) lock_votes: Vec<SignedVote>,
    /// The address of proposer.
    pub(crate) proposer: Address,
}

impl Debug for Proposal {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "Proposal {{ h: {}, r: {}, hash:{:?}, addr: {:?}}}",
            self.height, self.round, self.block_hash, self.proposer,
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

/// Something need to be consensus in a round.
/// A `Proposal` includes `height`, `round`, `content`, `lock_round`, `lock_votes`
/// and `proposer`. `lock_round` and `lock_votes` are `Option`, means the PoLC of
/// the proposal. Therefore, these must have same variant of `Option`.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct SignedProposal {
    /// The height of proposal.
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
    /// Prevote vote or precommit vote
    pub(crate) vote_type: VoteType,
    /// The height of vote
    pub(crate) height: Height,
    /// The round of vote
    pub(crate) round: Round,
    /// The vote proposal
    pub(crate) block_hash: Hash,
    /// The address of voter
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

/// A vote to a proposal.
#[derive(Clone, Eq, PartialEq, Hash)]
pub(crate) struct SignedVote {
    /// Prevote vote or precommit vote
    pub(crate) vote: Vote,
    ///
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
    /// The lock proposal
    pub(crate) block_hash: Hash,
    /// The lock round
    pub(crate) round: Round,
    /// The lock votes.
    pub(crate) votes: Vec<SignedVote>,
}

#[derive(Clone, Debug)]
pub(crate) struct AuthorityManage {
    ///
    pub(crate) authorities: Vec<Node>,
    ///
    pub(crate) authorities_old: Vec<Node>,
    ///
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
}

/// BFT step
#[derive(Debug, PartialEq, PartialOrd, Eq, Clone, Copy, Hash)]
pub(crate) enum Step {
    /// A step to determine proposer and proposer publish a proposal.
    Propose,
    /// A step to wait for proposal or feed.
    ProposeWait,
    /// A step to transmit prevote and check prevote count.
    Prevote,
    /// A step to wait for more prevote if none of them reach 2/3.
    PrevoteWait,
    /// A step to wait for proposal verify result.
    #[cfg(feature = "verify_req")]
    VerifyWait,
    /// A step to transmit precommit and check precommit count.
    Precommit,
    /// A step to wait for more prevote if none of them reach 2/3.
    PrecommitWait,
    /// A step to do commit.
    Commit,
    /// A step to wait for rich status.
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

/// Bft vote types.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) enum VoteType {
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

#[derive(Debug)]
pub(crate) enum LogType {
    Proposal,
    Vote,
    Status,
    Proof,
    Feed,
    #[cfg(feature = "verify_req")]
    VerifyResp,
    TimeOutInfo,
    Block,
}

impl From<u8> for LogType {
    fn from(s: u8) -> Self {
        match s {
            0 => LogType::Proposal,
            1 => LogType::Vote,
            2 => LogType::Status,
            3 => LogType::Proof,
            4 => LogType::Feed,
            #[cfg(feature = "verify_req")]
            5 => LogType::VerifyResp,
            6 => LogType::TimeOutInfo,
            7 => LogType::Block,
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
            #[cfg(feature = "verify_req")]
            LogType::VerifyResp => 5,
            LogType::TimeOutInfo => 6,
            LogType::Block => 7,
        }
    }
}

pub(crate) enum PrecommitRes {
    Above,
    Below,
    Nil,
    Proposal,
}
