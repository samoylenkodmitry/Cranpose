use serde::Serialize;

/// Application-configurable bounds on live documents and retained diagnostics.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Limits {
    /// Maximum number of component nodes in a committed document.
    pub nodes: usize,
    /// Maximum nesting of nodes or bound expressions.
    pub depth: usize,
    /// Maximum UTF-8 source file size accepted by the translator.
    pub source_bytes: usize,
    /// Maximum edits in a single transaction.
    pub edits: usize,
    /// Maximum retained native diagnostics before older messages are discarded.
    pub diagnostics: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            nodes: 10_000,
            depth: 128,
            source_bytes: 1_048_576,
            edits: 1_000,
            diagnostics: 32,
        }
    }
}
