use crate::algorithm::Bft;
#[cfg(feature = "verify_req")]
use crate::VerifyResp;
use crate::{Address, BftMsg, Commit, Feed, Proposal, Status, Vote};
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};

/// An simple BFT executor.
pub struct BftExecutor {
    sender: Sender<BftMsg>,
    receiver: Receiver<BftMsg>,
}

/// Messages to BFT engine via BftInput.
pub enum BftInput {
    /// Proposal message or Vote message.
    Message(Message),
    /// Feed messge, this is the proposal of the height.
    Feed(Feed),
    /// Status message, rich status.
    Status(Status),
    /// Verify response
    #[cfg(feature = "verify_req")]
    VerifyResp(VerifyResp),
}

/// Messages from BFT engine via BftInput.
pub enum BftOutput {
    /// Proposal message or Vote message.
    Message(Message),
    /// Commit message.
    Commit(Commit),
}

/// Error for BftExecutor.
#[derive(Debug)]
pub enum Error {
    /// Send message error.
    SendMessage,
    /// Recveive message error.
    RecvMessage,
    /// Unreachable error.
    Unreachable,
}

/// Result for BftExecutor.
pub type Result<T> = ::std::result::Result<T, Error>;

/// Proposal message or Vote message.
pub enum Message {
    /// Proposal message.
    Proposal(Proposal),
    /// Vote message.
    Vote(Vote),
}

impl BftExecutor {
    /// Create a new BftExecutor.
    pub fn new(address: Address) -> Self {
        let (sender, internal_receiver) = unbounded();
        let (internal_sender, receiver) = unbounded();
        Bft::start(internal_sender, internal_receiver, address);
        Self { sender, receiver }
    }

    /// Send a message to BFT engine via BftExecutor.
    pub fn send(&self, input: BftInput) -> Result<()> {
        match input {
            BftInput::Message(msg) => Ok(match msg {
                Message::Proposal(proposal) => BftMsg::Proposal(proposal),
                Message::Vote(vote) => BftMsg::Vote(vote),
            }),
            BftInput::Feed(feed) => Ok(BftMsg::Feed(feed)),
            BftInput::Status(status) => Ok(BftMsg::Status(status)),
            #[cfg(feature = "verify_req")]
            BftInput::VerifyResp(resp) => Ok(BftMsg::VerifyResp(resp)),
        }
        .and_then(|msg| self.sender.send(msg).map_err(|_| Error::SendMessage))
    }

    /// Receive a message to BFT engine via BftExecutor.
    pub fn recv(&self) -> Result<BftOutput> {
        self.receiver
            .recv()
            .map_err(|_| Error::RecvMessage)
            .and_then(|msg| match msg {
                BftMsg::Proposal(proposal) => Ok(BftOutput::Message(Message::Proposal(proposal))),
                BftMsg::Vote(vote) => Ok(BftOutput::Message(Message::Vote(vote))),
                BftMsg::Commit(commit) => Ok(BftOutput::Commit(commit)),
                _ => Err(Error::Unreachable),
            })
    }
}
