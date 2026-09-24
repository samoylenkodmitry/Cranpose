//! Every integration test of this crate, linked as one binary: one link
//! instead of one per file, while nextest still runs each test in its own
//! process.

mod activation_semantics;
mod lazy_list_identity;
mod text_buffer_editing;
