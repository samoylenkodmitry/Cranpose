/// A rejected live program or native API call.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Source is outside the supported Rust subset.
    #[error("source: {0}")]
    Source(String),
    /// A program does not match the registered catalogue.
    #[error("invalid live program: {0}")]
    Invalid(String),
    /// A patch was based on an older document.
    #[error("stale revision: expected {expected}, received {received}")]
    Stale {
        /// Current document revision.
        expected: u64,
        /// Revision supplied by the client.
        received: u64,
    },
    /// A native argument could not be decoded.
    #[error(transparent)]
    Value(#[from] serde_json::Error),
}
