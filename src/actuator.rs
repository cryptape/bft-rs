use crate::*;
use crate::{algorithm::Bft, error::BftError};

use crossbeam::crossbeam_channel::{unbounded, Sender};
use rlp::encode;

/// Results of Bft actuator.
pub type Result<T> = ::std::result::Result<T, BftError>;

/// A Bft Actuator
pub struct BftActuator(Sender<BftMsg>);

impl BftActuator {
    /// A function to create a new Bft actuator.
    pub fn new<T: BftSupport + Send + 'static>(support: T, address: Address) -> Self {
        let (sender, internal_receiver) = unbounded();
        Bft::start(internal_receiver, support, address);
        BftActuator(sender)
    }

    /// A function to send proposal.
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

    /// A function to send vote.
    pub fn send_vote<F: Crypto>(&self, signed_vote: SignedVote<F>) -> Result<()> {
        let sig = signed_vote.signature;
        let vote = signed_vote.vote;

        // check vote signature
        sig.check_signature(&sig.hash(encode(&vote)), &sig.get_signature())?;
        self.0
            .send(BftMsg::Vote(vote))
            .map_err(|_| BftError::SendVoteErr)
    }

    /// A function to send status.
    pub fn send_status(&self, status: Status) -> Result<()> {
        self.0
            .send(BftMsg::Status(status))
            .map_err(|_| BftError::SendVoteErr)
    }

    /// A function to send command
    pub fn send_command(&self, cmd: BftMsg) -> Result<()> {
        match cmd {
            BftMsg::Pause => Ok(cmd),
            BftMsg::Start => Ok(cmd),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|cmd| self.0.send(cmd).map_err(|_| BftError::SendCmdErr))
    }
}
