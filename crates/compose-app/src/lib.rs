#![deny(missing_docs)]

//! High level utilities for running Compose applications with minimal boilerplate.

#[cfg(not(any(feature = "desktop", feature = "android")))]
compile_error!("compose-app must be built with at least one of `desktop` or `android` features.");

#[cfg(not(any(feature = "renderer-pixels", feature = "renderer-wgpu")))]
compile_error!("compose-app requires either `renderer-pixels` or `renderer-wgpu` feature.");

mod launcher;
pub use launcher::{AppLauncher, AppSettings};

pub mod robot;

// Platform-specific runtime modules
#[cfg(all(feature = "android", feature = "renderer-wgpu"))]
pub mod android;

#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub mod desktop;
