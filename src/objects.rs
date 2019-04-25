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
    /// The proposal content.
    pub(crate) block: Block,
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
        write!(f, "Proposal {{ height: {}, round: {}, proposer: {:?}}}",
               self.height,
               self.round,
               self.proposer,
        )
    }
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
                let height: Height = r.val_at(0)?;
                let round: Round = r.val_at(1)?;
                let block: Block = r.val_at(2)?;
                let proof: Proof = r.val_at(3)?;
                let lock_round: Option<Round> = r.val_at(4)?;
                let lock_votes: Vec<SignedVote> = r.list_at(5)?;
                let proposer: Address = r.val_at(6)?;
                Ok(Proposal {
                    height,
                    round,
                    block,
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
        write!(f, "SignedProposal {{ proposal: {:?}, signature: {:?}}}",
               self.proposal,
               self.signature,
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
        write!(f, "Vote {{ vote_type: {:?}, height: {}, round: {}, block_hash: {:?}, voter: {:?}}}",
               self.vote_type,
               self.height,
               self.round,
               self.block_hash,
               self.voter,
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
        write!(f, "SignedVote {{ vote: {:?}, signature: {:?}}}",
               self.vote,
               self.signature,
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
    pub(crate) authority_h_old: u64,
}

impl AuthorityManage {
    pub(crate) fn new() -> Self {
        let authority_manage = AuthorityManage {
            authorities: Vec::new(),
            authorities_old: Vec::new(),
            authority_h_old: 0,
        };
        authority_manage
    }

    pub(crate) fn receive_authorities_list(&mut self, height: u64, mut authorities: Vec<Node>) {
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
    Feed,
    #[cfg(feature = "verify_req")]
    VerifyResp,
}

impl From<u8> for LogType {
    fn from(s: u8) -> Self {
        match s {
            0 => LogType::Proposal,
            1 => LogType::Vote,
            2 => LogType::Status,
            3 => LogType::Feed,
            #[cfg(feature = "verify_req")]
            4 => LogType::VerifyResp,
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
            #[cfg(feature = "verify_req")]
            LogType::VerifyResp => 4,
        }
    }
}

pub(crate) enum PrecommitRes {
    Above,
    Below,
    Nil,
    Proposal,
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_proof_rlp() {
        let address_1 = vec![87u8, 9u8, 17u8];
        let address_2 = vec![84u8, 91u8, 17u8];
        let signature_1 = vec![
            23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8,
        ];
        let signature_2 = vec![
            23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8,
        ];
        let mut precommit_votes: HashMap<Address, Signature> = HashMap::new();
        precommit_votes.entry(address_1).or_insert(signature_1);
        precommit_votes.entry(address_2).or_insert(signature_2);
        let proof = Proof {
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
        let vote = Vote {
            vote_type: VoteType::Prevote,
            height: 10u64,
            round: 2u64,
            block_hash: vec![76u8, 8u8],
            voter: vec![76u8, 8u8],
        };

        let signed_vote = SignedVote {
            vote,
            signature: vec![
                23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8,
                21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8,
                10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
                32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8,
                21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            ],
        };
        let encode = rlp::encode(&signed_vote);
        let decode: SignedVote = rlp::decode(&encode).unwrap();
        assert_eq!(signed_vote, decode);
    }

    #[test]
    fn test_proposal_rlp() {
        let address_1 = vec![87u8, 9u8, 17u8];
        let address_2 = vec![84u8, 91u8, 17u8];
        let signature_1 = vec![
            23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8,
        ];
        let signature_2 = vec![
            23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8,
        ];
        let mut precommit_votes: HashMap<Address, Signature> = HashMap::new();
        precommit_votes.entry(address_1).or_insert(signature_1);
        precommit_votes.entry(address_2).or_insert(signature_2);
        let proposal = Proposal {
            height: 787655u64,
            round: 2u64,
            block: vec![76u8, 9u8, 12u8],
            proof: Proof {
                height: 1888787u64,
                round: 23u64,
                block_hash: vec![10u8, 90u8, 23u8, 65u8],
                precommit_votes,
            },
            lock_round: Some(1u64),
            lock_votes: vec![SignedVote {
                vote: Vote {
                    vote_type: VoteType::Prevote,
                    height: 10u64,
                    round: 2u64,
                    block_hash: vec![76u8, 8u8],
                    voter: vec![76u8, 8u8],
                },
                signature: vec![
                    23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8,
                    11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8,
                    21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
                    9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8,
                    10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
                ],
            }],
            proposer: vec![10u8, 90u8, 23u8, 65u8],
        };

        let signed_proposal = SignedProposal {
            proposal,
            signature: vec![
                23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8,
                21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8,
                10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
                32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8,
                21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8,
            ],
        };

        let encode = rlp::encode(&signed_proposal);
        let decode: SignedProposal = rlp::decode(&encode).unwrap();
        assert_eq!(signed_proposal, decode);
    }

    #[test]
    fn test_feed_rlp() {
        let feed = Feed {
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
        let signature_1 = vec![
            23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8,
        ];
        let signature_2 = vec![
            23u8, 32u8, 11u8, 21u8, 9u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8,
            9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8, 32u8, 11u8, 21u8, 9u8, 10u8, 23u8,
            32u8, 11u8, 21u8, 9u8, 10u8,
        ];
        let mut precommit_votes: HashMap<Address, Signature> = HashMap::new();
        precommit_votes.entry(address_1).or_insert(signature_1);
        precommit_votes.entry(address_2).or_insert(signature_2);
        let commit = Commit {
            height: 878655u64,
            block: vec![87u8, 23u8],
            proof: Proof {
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
        let node = Node {
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
        let node_1 = Node {
            address: vec![99u8, 12u8],
            proposal_weight: 43u32,
            vote_weight: 98u32,
        };
        let node_2 = Node {
            address: vec![99u8, 12u8],
            proposal_weight: 43u32,
            vote_weight: 98u32,
        };
        let status = Status {
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
        let verify_resp = VerifyResp {
            is_pass: false,
            block_hash: vec![99u8, 12u8],
        };

        let encode = rlp::encode(&verify_resp);
        let decode: VerifyResp = rlp::decode(&encode).unwrap();
        assert_eq!(verify_resp, decode);
    }
}
