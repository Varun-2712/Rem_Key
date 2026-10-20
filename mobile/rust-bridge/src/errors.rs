#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum RelayError {
    #[error("invalid desktop address: {message}")]
    InvalidAddress { message: String },
    #[error("invalid certificate fingerprint: {message}")]
    InvalidFingerprint { message: String },
    #[error("already connected or connecting")]
    AlreadyConnected,
    #[error("not connected")]
    NotConnected,
    /// The send queue is full. Only returned for commands that must not be dropped;
    /// pointer and scroll deltas are discarded instead.
    #[error("send queue is full")]
    QueueFull,
}
