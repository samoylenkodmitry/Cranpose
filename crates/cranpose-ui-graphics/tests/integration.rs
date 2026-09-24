//! Every integration test of this crate, linked as one binary: one link
//! instead of one per file, while nextest still runs each test in its own
//! process.

mod record_preparation;
mod vector_path_fill;
