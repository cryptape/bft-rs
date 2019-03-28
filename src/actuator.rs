use crate::*;
use crate::{algorithm::Bft, error::BftError};

use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use rlp::encode;

/// Results of Bft actuator.
pub type Result<T> = ::std::result::Result<T, BftError>;

/// A Bft Actuator
pub struct BftActuator {
    sender: Sender<BftMsg>,
    receiver: Receiver<BftMsg>,
}

impl BftActuator {
    /// A function to create a new Bft actuator.
    pub fn new(address: Address) -> Self {
        let (sender, internal_receiver) = unbounded();
        let (internal_sender, receiver) = unbounded();
        Bft::start(internal_sender, internal_receiver, address);
        Self { sender, receiver }
    }

    /// A function to send proposal.
    pub fn send_proposal<T: Crypto>(&self, sp: SignProposal<T>) -> Result<()> {
        let sig = sp.signature;
        let proposal = sp.proposal;

        // check proposal signature
        sig.check_signature(&sig.hash(encode(&proposal)), &sig.get_signature())?;

        // check lock round and lock votes
        if proposal.lock_round.is_some() && proposal.lock_votes.is_empty() {
            return Err(BftError::ProposalIllegal(proposal.height, proposal.round));
        }
        self.sender
            .send(BftMsg::Proposal(proposal))
            .map_err(|_| BftError::SendProposalErr)
    }

    /// A function to send vote.
    pub fn send_vote<T: Crypto>(&self, sv: SignVote<T>) -> Result<()> {
        let sig = sv.signature;
        let vote = sv.vote;

        // check vote signature
        sig.check_signature(&sig.hash(encode(&vote)), &sig.get_signature())?;
        self.sender
            .send(BftMsg::Vote(vote))
            .map_err(|_| BftError::SendVoteErr)
    }

    ///
    pub fn send_commend(&self, cmd: BftMsg) -> Result<()> {
        match cmd {
            BftMsg::Pause => Ok(cmd),
            BftMsg::Start => Ok(cmd),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|cmd| self.sender.send(cmd).map_err(|_| BftError::SendCmdErr))
    }
}
