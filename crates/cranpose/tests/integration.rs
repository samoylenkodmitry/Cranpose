//! Every integration test of this crate, linked as one binary: one link
//! instead of one per file, while nextest still runs each test in its own
//! process.

mod host_effects;
mod ios_scene_static;
mod platform_scheduling_static;
mod prelude_surface;
