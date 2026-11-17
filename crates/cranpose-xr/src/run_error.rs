//! What a failed desktop run reports.

use cranpose::LaunchError;

/// Why [`run`](crate::run) failed.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The machine has a headset, and its session failed.
    #[error("the headset session failed: {0}")]
    Headset(String),
    /// The flat window failed to open.
    #[error("the flat window failed: {0}")]
    Window(#[from] LaunchError),
}
