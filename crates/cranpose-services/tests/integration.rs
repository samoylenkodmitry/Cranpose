//! Every integration test of this crate, linked as one binary: one link
//! instead of one per file, while nextest still runs each test in its own
//! process.

mod app_info_integration;
mod audio_haptics_integration;
mod capability_state_integration;
mod hook_caller_identity;
mod http_client_integration;
mod incoming_share_delivery;
mod launch_args_integration;
mod launcher_integration;
mod lifecycle_steps;
mod media_integration;
mod theme_integration;
mod uri_handler_integration;
mod windowless_command;
