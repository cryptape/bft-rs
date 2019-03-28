/// Error for Bft actuator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BftError {
    /// Send proposal error.
    SendProposalErr,
    /// Send proposal error.
    SendVoteErr,
    /// Receive message error.
    RecvMsgErr,
    /// Message type error.
    MsgTypeErr,
    /// Unreachable error.
    Unreachable,
    /// The lock round of the Proposal is `Some`, however, lock vote is empty.
    ProposalIllegal(u64, u64),
}