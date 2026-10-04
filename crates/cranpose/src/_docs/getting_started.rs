//! # Start an app
//!
//! Use the [project template](https://github.com/samoylenkodmitry/cranpose-showcase)
//! for screens, view models and platform hosts. [`apps/isolated-demo`] supplies
//! a smaller example with published dependencies.
//!
//! For a desktop app with one source file:
//!
//! ```sh
//! cargo new hello
//! cd hello
//! cargo add cranpose --features desktop,renderer-wgpu
//! ```
//!
//! Put this code in `src/main.rs`, then run `cargo run`:
//!
//! ```no_run
//! use cranpose::*;
//!
//! #[composable]
//! fn Hello() {
//!     Text("Hello", Modifier::empty(), TextStyle::default());
//! }
//!
//! fn main() -> Result<(), cranpose::LaunchError> {
//!     AppLauncher::new()
//!         .with_title("Hello")
//!         .with_size(320, 200)
//!         .try_run(Hello)
//! }
//! ```
//!
//! `#[composable]` retains the function's state and subscriptions. The launcher
//! owns the window and event loop. Reuse the same composable from mobile and
//! web hosts through their platform entry points.
//!
//! Continue with the [state guide](super::state) for a control with callbacks,
//! or the [platform guide](super::platforms) for another target.
//!
//! [`apps/isolated-demo`]: https://github.com/samoylenkodmitry/cranpose/tree/main/apps/isolated-demo
