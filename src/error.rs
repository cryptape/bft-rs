/// Error for Bft actuator.
pub enum BftError {
    /// Send message error.
    SendMsgErr,
    /// Receive message error.
    RecvMsgErr,
    /// Message type error.
    MsgTypeErr,
    /// Unreachable error.
    Unreachable,
}
