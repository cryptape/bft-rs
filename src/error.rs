
pub type BftResult<T> = ::std::result::Result<T, BftError>;
/// Error for Bft actuator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BftError {
    /// Send message error.
    SendMsgErr(String),
    /// Receive message error.
    RecvMsgErr(String),

    DecodeErr(String),

    ObsoleteMsg(String),

    HigherMsg(String),

    SaveWalErr(String),

    ShouldNotHappen(String),

    InvalidSender(String),

    CheckBlockFailed(String),

    CheckTxFailed(String),

    CheckSigFailed(String),

    SignFailed(String),

    CommitFailed(String),

    CheckProofFailed(String),

    CheckLockVotesFailed(String),

    RecvMsgAgain(String),

    NotReady(String),

    ObsoleteTimer(String),
}

pub(crate) fn handle_error<T>(result: BftResult<T>) {
    if let Err(error) = result {
        match error {
            BftError::NotReady(_)
            | BftError::ObsoleteMsg(_)
            | BftError::HigherMsg(_)
            | BftError::RecvMsgAgain(_) => info!("Bft encounters {:?}", error),

            BftError::CheckProofFailed(_)
            | BftError::CheckBlockFailed(_)
            | BftError::CheckLockVotesFailed(_)
            | BftError::CheckSigFailed(_)
            | BftError::CheckTxFailed(_)
            | BftError::DecodeErr(_)
            | BftError::InvalidSender(_) => warn!("Bft encounters {:?}", error),

            BftError::ShouldNotHappen(_)
            | BftError::SendMsgErr(_)
            | BftError::RecvMsgErr(_)
            | BftError::CommitFailed(_)
            | BftError::SaveWalErr(_)
            | BftError::SignFailed(_) => error!("Bft encounters {:?}", error),

            BftError::ObsoleteTimer(_) => {}
        }
    }
}
