//! Error types for stream wrappers.

/// Error returned by `BroadcastStream` when the receiver lags behind.
#[derive(Debug)]
pub enum BroadcastStreamRecvError {
    /// The receiver lagged behind and missed `n` messages.
    Lagged(u64),
}

impl std::fmt::Display for BroadcastStreamRecvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lagged(n) => write!(f, "receiver lagged by {n} messages"),
        }
    }
}

impl std::error::Error for BroadcastStreamRecvError {}
