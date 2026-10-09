//! Keeps the microphone usable while the application is in the background.
//!
//! Android lets a background application open the microphone only from a
//! foreground service of type microphone that the application started while
//! it was in front. [`hold_microphone_standby`] starts that service; call it
//! from the screen, for example when the person turns on a "ready" mode.
//! While any [`MicrophoneStandbyLease`] is held, the application may open the
//! microphone later from the background, such as when a paired watch asks
//! with the phone in a pocket. Dropping the last lease stops the service. The
//! service records nothing itself; the system shows its microphone indicator
//! only while the application records.
//!
//! An Android application turns it on with
//! `cranpose { services.add("microphone-standby") }`. Elsewhere a lease holds
//! nothing and [`microphone_standby_available`] is false.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::registry::ServiceRegistry;

/// The platform's standby, installed by the platform backend.
pub trait MicrophoneStandby: Send + Sync {
    /// Starts the standby when `held`, stops it otherwise.
    fn set_held(&self, held: bool);
}

static PLATFORM: ServiceRegistry<dyn MicrophoneStandby> = ServiceRegistry::new();
static HOLDERS: AtomicUsize = AtomicUsize::new(0);

/// Installs the platform's standby, or removes it with `None`; leases then
/// hold nothing. Platform backends call it.
pub fn set_platform_microphone_standby(standby: Option<Arc<dyn MicrophoneStandby>>) {
    match standby {
        Some(standby) => PLATFORM.set(standby),
        None => PLATFORM.clear(),
    }
}

/// Whether this platform and build can keep the microphone ready.
pub fn microphone_standby_available() -> bool {
    PLATFORM.get().is_some()
}

/// A claim on the microphone standby; the standby stops when the last lease
/// is dropped.
pub struct MicrophoneStandbyLease {
    _private: (),
}

impl Drop for MicrophoneStandbyLease {
    fn drop(&mut self) {
        if HOLDERS.fetch_sub(1, Ordering::AcqRel) == 1
            && let Some(standby) = PLATFORM.get()
        {
            standby.set_held(false);
        }
    }
}

/// Starts the microphone standby, or joins it if another lease holds it.
/// Call it while the application is in front.
pub fn hold_microphone_standby() -> MicrophoneStandbyLease {
    if HOLDERS.fetch_add(1, Ordering::AcqRel) == 0
        && let Some(standby) = PLATFORM.get()
    {
        standby.set_held(true);
    }
    MicrophoneStandbyLease { _private: () }
}

#[cfg(test)]
#[path = "tests/microphone_standby_tests.rs"]
mod tests;
