//! The program that hosts the session: an Android activity, or a desktop process.

#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod desktop;

#[cfg(target_os = "android")]
pub use android::run;
#[cfg(target_os = "android")]
pub(crate) use android::{Host, SystemKeyboard};
#[cfg(not(target_os = "android"))]
pub use desktop::run;
#[cfg(not(target_os = "android"))]
pub(crate) use desktop::{Host, SystemKeyboard};
