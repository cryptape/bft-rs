use log::{error, trace, warn};

pub type BftResult<T> = ::std::result::Result<T, BftError>;
/// Error for Bft actuator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BftError {
    ShouldNotHappen(String),
    /// Send message error.
    SendMsgErr(String),
    /// Receive message error.
    RecvMsgErr(String),

    RecvMsgAgain(String),

    ObsoleteMsg(String),

    HigherMsg(String),

    DecodeErr(String),

    SaveWalErr(String),

    InvalidSender(String),

    MismatchingBlock(String),

    CheckBlockFailed(String),

    CheckTxFailed(String),

    CheckSigFailed(String),

    CheckProofFailed(String),

    CheckLockVotesFailed(String),

    SignFailed(String),

    CommitFailed(String),

    GetBlockFailed(String),

    NotReady(String),

    ObsoleteTimer(String),
}

pub(crate) fn handle_err<T>(result: BftResult<T>) {
    if let Err(e) = result {
        match e {
            BftError::NotReady(_)
            | BftError::ObsoleteMsg(_)
            | BftError::HigherMsg(_)
            | BftError::RecvMsgAgain(_) => trace!("Bft encounters {:?}", e),

            BftError::CheckProofFailed(_)
            | BftError::CheckBlockFailed(_)
            | BftError::CheckLockVotesFailed(_)
            | BftError::CheckSigFailed(_)
            | BftError::CheckTxFailed(_)
            | BftError::DecodeErr(_)
            | BftError::InvalidSender(_)
            | BftError::MismatchingBlock(_) => warn!("Bft encounters {:?}", e),

            BftError::ShouldNotHappen(_)
            | BftError::SendMsgErr(_)
            | BftError::RecvMsgErr(_)
            | BftError::CommitFailed(_)
            | BftError::SaveWalErr(_)
            | BftError::SignFailed(_)
            | BftError::GetBlockFailed(_) => error!("Bft encounters {:?}", e),

            BftError::ObsoleteTimer(_) => {}
        }
    }
}
