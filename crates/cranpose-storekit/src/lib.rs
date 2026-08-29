#![deny(missing_docs)]

//! StoreKit 2 backend for [`cranpose_services::purchases`](mod@cranpose_services::purchases).
//!
//! StoreKit 2 has no Objective-C surface — `Product`, `Transaction` and the
//! on-device JWS verification are Swift-only — so this crate compiles a small
//! Swift shim (`swift/storekit.swift`) in its build script and links it
//! statically. Apple recommends StoreKit 2 for anything that can require
//! iOS 15; StoreKit 1 (`SKPaymentQueue`) is the Objective-C fallback and is
//! deliberately not used here.
//!
//! On non-Apple targets the build script is a no-op and every entry point
//! compiles away, so this crate can sit in an Android/Linux/web dependency
//! graph unnoticed.
//!
//! ```no_run
//! # #[cfg(any(target_os = "ios", target_os = "macos"))]
//! # fn demo() {
//! cranpose_storekit::register();
//! cranpose_services::purchases::configure(&["com.example.app.pro"]);
//! # }
//! ```
//!
//! # Threading
//!
//! StoreKit answers on Swift concurrency executors, so the Swift shim calls
//! back from arbitrary threads. All state therefore lives behind a
//! [`std::sync::Mutex`] here, and [`cranpose_services::purchases::Purchases`]
//! reads a clone of it from the UI thread.

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;

#[cfg(any(target_os = "ios", target_os = "macos"))]
pub use apple::{StoreKitPurchases, register};

/// Installs the StoreKit backend — a no-op on targets without an App Store.
///
/// Call it before
/// [`configure`](cranpose_services::purchases::configure); apps that also run
/// on Android or desktop can call it unconditionally.
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
pub fn register() {}
