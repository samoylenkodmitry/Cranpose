#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

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
