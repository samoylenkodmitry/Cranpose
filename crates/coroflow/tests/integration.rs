//! Every integration test of this crate, linked as one binary: one link
//! instead of one per file, while nextest still runs each test in its own
//! process.

mod allocations;
mod blocking_dispatch;
mod buffering;
mod channels;
mod exclusion;
mod hot_flows;
mod more_operators;
mod operators;
mod scopes;
mod send_inference;
mod sharing;
mod structured;
