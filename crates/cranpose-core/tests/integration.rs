//! Every integration test of this crate, linked as one binary: one link
//! instead of one per file, while nextest still runs each test in its own
//! process.

mod effects_and_frames;
mod lifetime_cancellation;
mod snapshot_runtime_thread_isolation;
mod state_hooks;
mod value_slot_handle_ui;
