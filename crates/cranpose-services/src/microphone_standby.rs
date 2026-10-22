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
//! The standby shows a notification. [`set_microphone_standby_notice`] says
//! in the application's language what it does now ("Waiting for the watch",
//! "Recording") and names the action that ends it; when the person takes
//! that action the standby stops and [`on_microphone_standby_stop`] tells the
//! application, which then drops its leases.
//!
//! An Android application turns it on with
//! `cranpose { services.add("microphone-standby") }`. Elsewhere a lease holds
//! nothing and [`microphone_standby_available`] is false.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::registry::ServiceRegistry;

/// What the standby's notification says, in the application's language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandbyNotice {
    /// What the application does now, for example "Waiting for the watch"
    /// or "Recording".
    pub text: String,
    /// The label of the action that ends the standby, for example "Stop".
    pub stop: String,
}

/// The platform's standby, installed by the platform backend.
pub trait MicrophoneStandby: Send + Sync {
    /// Starts the standby when `held`, stops it otherwise.
    fn set_held(&self, held: bool);

    /// Shows `notice` on the standby's notification, at once when it shows
    /// and from its next start.
    fn set_notice(&self, notice: &StandbyNotice);
}

static PLATFORM: ServiceRegistry<dyn MicrophoneStandby> = ServiceRegistry::new();
static HOLDERS: AtomicUsize = AtomicUsize::new(0);
static STOPPED: ServiceRegistry<dyn Fn() + Send + Sync> = ServiceRegistry::new();

/// Installs the platform's standby, or removes it with `None`; leases then
/// hold nothing. Platform backends call it.
pub fn set_platform_microphone_standby(standby: Option<Arc<dyn MicrophoneStandby>>) {
    match standby {
        Some(standby) => PLATFORM.set(standby),
        None => PLATFORM.clear(),
    }
}

/// Says what the standby's notification shows and how its action is named.
pub fn set_microphone_standby_notice(notice: StandbyNotice) {
    if let Some(standby) = PLATFORM.get() {
        standby.set_notice(&notice);
    }
}

/// Runs `stopped` when the person ends the standby from its notification.
/// The standby has stopped then; the application drops its leases and
/// shows that it no longer waits.
pub fn on_microphone_standby_stop(stopped: impl Fn() + Send + Sync + 'static) {
    STOPPED.set(Arc::new(stopped));
}

/// The person ended the standby from its notification. Platform backends
/// call it.
pub fn microphone_standby_stopped() {
    if let Some(stopped) = STOPPED.get() {
        stopped();
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
