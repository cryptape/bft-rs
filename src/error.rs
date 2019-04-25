
pub type BftResult<T> = ::std::result::Result<T, BftError>;
/// Error for Bft actuator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BftError {
    /// Send message error.
    SendMsgErr(String),
    /// Receive message error.
    RecvMsgErr(String),
    /// Message type error.
    MsgTypeErr(String),
    /// Unreachable error.
    Unreachable(String),

    DecodeErr(String),

    ObsoleteMsg(String),

    HigherMsg(String),

    SaveWalErr(String),

    ShouldNotHappen(String),

    EmptyAuthManage(String),

    InvalidSender(String),

    CheckBlockFailed(String),

    CheckTxFailed(String),

    NoConsensusPower(String),

    CheckSigFailed(String),

    SignFailed(String),

    CommitFailed(String),

    RepeatLockVote(String),

    NotEnoughVotes(String),

    CheckProofFailed(String),

    CheckLockVotesFailed(String),

    VoteError(String),

    RecvMsgAgain(String),

    NotReady(String),
}

pub(crate) fn handle_error<T>(result: BftResult<T>) {
    if let Err(error) = result {
        match error {
            BftError::NotReady(_) => info!("{:?}", error),
            BftError::CheckProofFailed(_) => warn!("Bft encounters {:?}", error),
            BftError::Unreachable(_) => error!("Bft encounters {:?}", error),
            _ => {}
        }
    }
}
