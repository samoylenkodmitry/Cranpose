//! # Getting started
//!
//! Copy [`apps/isolated-demo`] rather than starting from scratch: it is a
//! complete project that already carries the desktop, Android and web entry
//! points, and it depends only on published crates.
//!
//! ```toml
//! [dependencies]
//! cranpose = { version = "0.1", features = ["desktop", "renderer-wgpu"] }
//! ```
//!
//! Composables are CamelCase functions, which is why every file starts by
//! allowing the non-snake-case name:
//!
//! ```no_run
//! #![allow(non_snake_case)]
//! use cranpose::prelude::*;
//! # use cranpose::{
//! #     __branch_group_scope_deferred, branch_location_key,
//! #     cached_branch_location_key, cached_composable_definition_key,
//! #     caller_location_key,
//! #     composable_definition_key, composable_identity_key, debug_label_current_scope,
//! #     location_key,
//! #     with_current_composer, CallbackHolder, Composer, Key, ParamState, ReturnSlot,
//! # };
//!
//! #[composable]
//! fn Hello() {
//!     Text("Hello", Modifier::empty(), TextStyle::default());
//! }
//!
//! fn main() {
//!     AppLauncher::new()
//!         .with_title("Hello")
//!         .with_size(320, 200)
//!         .try_run(Hello)
//!         .expect("launch the app");
//! }
//! ```
//!
//! Cranpose is pre-alpha: versions are not compatible with each other and APIs
//! change without deprecation cycles. Pin an exact version.
//!
//! [`apps/isolated-demo`]: https://github.com/samoylenkodmitry/cranpose/tree/main/apps/isolated-demo
