/// Error for Bft actuator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BftError {
    /// Send message error.
    SendMsgErr,
    /// Receive message error.
    RecvMsgErr,
    /// Message type error.
    MsgTypeErr,
    /// Unreachable error.
    Unreachable,

    DecodeErr,

    ObsoleteMsg,

    HigherMsg,

    SaveWalErr,

    ShouldNotHappen,

    EmptyAuthManage,

    InvalidProposer,

    InvalidVoter,

    CheckBlockFailed,

    NoConsensusPower,

    CheckSigFailed,

//    FailedToSign,

    MismatchingProposer,

    MismatchingVoter,

    MismatchingVote,

    RepeatLockVote,

    NotEnoughVotes,

    CheckProofFailed,

    VoteError,

}