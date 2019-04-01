use crate::*;
use crate::{algorithm::Bft, error::BftError};

use crossbeam::crossbeam_channel::{unbounded, Sender};
use rlp::encode;

#[cfg(feature = "message_cache")]
use std::collections::HashMap;

/// Results of Bft actuator.
pub type Result<T> = ::std::result::Result<T, BftError>;

/// A Bft Actuator
#[cfg(not(feature = "message_cache"))]
pub struct BftActuator(Sender<BftMsg>);

/// A Bft Actuator
#[cfg(feature = "message_cache")]
pub struct BftActuator {
    sender: Sender<BftMsg>,
    height: u64,
    proposal_cache: HashMap<u64, Vec<Proposal>>,
    vote_cache: HashMap<u64, Vec<Vote>>,
}

impl BftActuator {
    /// A function to create a new Bft actuator.
    #[cfg(not(feature = "message_cache"))]
    pub fn new<T: BftSupport + Send + 'static>(support: T, address: Address) -> Self {
        let (sender, internal_receiver) = unbounded();
        Bft::start(internal_receiver, support, address);
        BftActuator(sender)
    }

    /// A function to create a new Bft actuator.
    #[cfg(feature = "message_cache")]
    pub fn new<T: BftSupport + Send + 'static>(support: T, address: Address) -> Self {
        let (sender, internal_receiver) = unbounded();
        Bft::start(internal_receiver, support, address);
        BftActuator {
            sender,
            height: 0,
            proposal_cache: HashMap::new(),
            vote_cache: HashMap::new(),
        }
    }

    /// A function to send proposal.
    #[cfg(not(feature = "message_cache"))]
    pub fn send_proposal<F: Crypto>(&self, signed_proposal: SignedProposal<F>) -> Result<()> {
        let sig = signed_proposal.signature;
        let proposal = signed_proposal.proposal;

        // check proposal signature
        sig.check_signature(&sig.hash(encode(&proposal)), &sig.get_signature())?;

        // check lock round and lock votes
        if proposal.lock_round.is_some() && proposal.lock_votes.is_empty() {
            return Err(BftError::ProposalIllegal(proposal.height, proposal.round));
        }
        self.0
            .send(BftMsg::Proposal(proposal))
            .map_err(|_| BftError::SendProposalErr)
    }

    /// A function to send proposal.
    #[cfg(feature = "message_cache")]
    pub fn send_proposal<F: Crypto>(&mut self, signed_proposal: SignedProposal<F>) -> Result<()> {
        let sig = signed_proposal.signature;
        let proposal = signed_proposal.proposal;

        // check proposal signature
        sig.check_signature(&sig.hash(encode(&proposal)), &sig.get_signature())?;

        // check lock round and lock votes
        if proposal.lock_round.is_some() && proposal.lock_votes.is_empty() {
            return Err(BftError::ProposalIllegal(proposal.height, proposal.round));
        }

        // check proposal height
        let p_height = proposal.height;
        if p_height > self.height {
            self.proposal_cache
                .entry(p_height)
                .or_insert(vec![])
                .push(proposal);
            return Ok(());
        }
        self.sender
            .send(BftMsg::Proposal(proposal))
            .map_err(|_| BftError::SendProposalErr)
    }

    /// A function to send vote.
    #[cfg(not(feature = "message_cache"))]
    pub fn send_vote<F: Crypto>(&self, signed_vote: SignedVote<F>) -> Result<()> {
        let sig = signed_vote.signature;
        let vote = signed_vote.vote;

        // check vote signature
        sig.check_signature(&sig.hash(encode(&vote)), &sig.get_signature())?;
        self.0
            .send(BftMsg::Vote(vote))
            .map_err(|_| BftError::SendVoteErr)
    }

    /// A function to send vote.
    #[cfg(feature = "message_cache")]
    pub fn send_vote<F: Crypto>(&mut self, signed_vote: SignedVote<F>) -> Result<()> {
        let sig = signed_vote.signature;
        let vote = signed_vote.vote;

        // check vote signature
        sig.check_signature(&sig.hash(encode(&vote)), &sig.get_signature())?;
        let v_height = vote.height;
        if v_height > self.height {
            self.vote_cache.entry(v_height).or_insert(vec![]).push(vote);
            return Ok(());
        }
        self.sender
            .send(BftMsg::Vote(vote))
            .map_err(|_| BftError::SendVoteErr)
    }

    /// A function to send status.
    #[cfg(not(feature = "message_cache"))]
    pub fn send_status(&self, status: Status) -> Result<()> {
        self.0
            .send(BftMsg::Status(status))
            .map_err(|_| BftError::SendVoteErr)
    }

    /// A function to send status.
    #[cfg(feature = "message_cache")]
    pub fn send_status(&mut self, status: Status) -> Result<()> {
        let s_height = status.height;
        if self.height > s_height {
            return Err(BftError::OutdateStatus(s_height));
        }

        if self.sender.send(BftMsg::Status(status)).is_ok() {
            // clean message cache
            self.clean_cache(self.height, s_height + 1);
            self.height = s_height + 1;
            self.send_cache_msg(self.height)?;
            return Ok(());
        }
        Err(BftError::SendStatusErr(s_height))
    }

    /// A function to send command
    #[cfg(not(feature = "message_cache"))]
    pub fn send_command(&self, cmd: BftMsg) -> Result<()> {
        match cmd {
            BftMsg::Pause => Ok(cmd),
            BftMsg::Start => Ok(cmd),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|cmd| self.0.send(cmd).map_err(|_| BftError::SendCmdErr))
    }

    /// A function to send command
    #[cfg(feature = "message_cache")]
    pub fn send_command(&self, cmd: BftMsg) -> Result<()> {
        match cmd {
            BftMsg::Pause => Ok(cmd),
            BftMsg::Start => Ok(cmd),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|cmd| self.sender.send(cmd).map_err(|_| BftError::SendCmdErr))
    }

    #[cfg(feature = "message_cache")]
    fn clean_cache(&mut self, h_begin: u64, h_end: u64) {
        for height in h_begin..h_end {
            let _ = self.proposal_cache.remove(&height);
            let _ = self.vote_cache.remove(&height);
        }
    }

    #[cfg(feature = "message_cache")]
    fn send_cache_msg(&self, height: u64) -> Result<()> {
        if let Some(c_proposal) = self.proposal_cache.get(&height) {
            self.send_cache_proposal(c_proposal.to_vec())?;
        }
        if let Some(c_vote) = self.vote_cache.get(&height) {
            self.send_cache_vote(c_vote.to_vec())?;
        }
        Ok(())
    }

    #[cfg(feature = "message_cache")]
    fn send_cache_proposal(&self, c_proposal: Vec<Proposal>) -> Result<()> {
        for cache_proposal in c_proposal.into_iter() {
            if self.sender.send(BftMsg::Proposal(cache_proposal)).is_err() {
                return Err(BftError::SendProposalErr);
            }
        }
        Ok(())
    }

    #[cfg(feature = "message_cache")]
    fn send_cache_vote(&self, c_vote: Vec<Vote>) -> Result<()> {
        for cache_vote in c_vote.into_iter() {
            if self.sender.send(BftMsg::Vote(cache_vote)).is_err() {
                return Err(BftError::SendVoteErr);
            }
        }
        Ok(())
    }
}
