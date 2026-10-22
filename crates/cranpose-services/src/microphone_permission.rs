//! Permission to record from the microphone.
//!
//! Stores refuse an application that records without asking, or that asks
//! without saying why, so the framework asks only when the application calls
//! [`request_microphone_permission`], and the person reads the reason the
//! build script gives `cranpose_capabilities::Use::microphone`. Reading the
//! permission never asks.
//!
//! A person who refused twice, or chose not to be asked again, can allow the
//! microphone only in the system settings of the application:
//! [`MicrophonePermission::Blocked`] says so and [`open_microphone_settings`]
//! goes there. [`rememberMicrophonePermission`] reads the permission again
//! each time the application comes back to the front, so an answer given in
//! the settings shows at once.
//!
//! Where no backend is installed (desktop systems, which ask by themselves
//! the first time an application records), the permission reads
//! [`MicrophonePermission::Granted`].

use std::sync::{Arc, Mutex, OnceLock, PoisonError, atomic::Ordering};

use cranpose_core::{State, rememberEventStream};
use portable_atomic::AtomicU64;

use crate::registry::ServiceRegistry;

/// Whether the application may record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MicrophonePermission {
    /// The person has not been asked.
    NotAsked,
    /// The person said yes.
    Granted,
    /// The person said no; asking again shows the prompt again.
    Denied,
    /// The person said no for good; only the system settings can allow it.
    Blocked,
}

/// A platform's microphone permission, installed by the platform backend.
pub trait MicrophoneAccess: Send + Sync {
    /// The permission as the platform reports it now.
    fn permission(&self) -> MicrophonePermission;
    /// Shows the system prompt. The backend calls
    /// [`publish_microphone_permission`] when the person answers.
    fn request(&self);
    /// Opens the system settings page of this application.
    fn open_settings(&self);
    /// Readies the platform's audio for recording, where opening an input
    /// stream is not enough: iOS and watchOS record only in a session whose
    /// category takes input.
    fn prepare_recording(&self) -> Result<(), String> {
        Ok(())
    }
    /// Releases what [`prepare_recording`](Self::prepare_recording) readied,
    /// once no recording needs it: on iOS and watchOS other apps' audio
    /// resumes and a Bluetooth headset leaves its call mode.
    fn finish_recording(&self) {}
}

static PLATFORM: ServiceRegistry<dyn MicrophoneAccess> = ServiceRegistry::new();

/// Installs the platform's permission, or removes it with `None`. Platform
/// backends call it.
pub fn set_platform_microphone_access(access: Option<Arc<dyn MicrophoneAccess>>) {
    match access {
        Some(access) => PLATFORM.set(access),
        None => PLATFORM.clear(),
    }
    publish_microphone_permission();
}

/// Whether the application may record now.
pub fn microphone_permission() -> MicrophonePermission {
    PLATFORM
        .get()
        .map_or(MicrophonePermission::Granted, |access| access.permission())
}

/// Asks the person, when the platform can still show its prompt: a no-op
/// once the microphone is allowed or blocked.
pub fn request_microphone_permission() {
    if let Some(access) = PLATFORM.get()
        && matches!(
            access.permission(),
            MicrophonePermission::NotAsked | MicrophonePermission::Denied
        )
    {
        access.request();
    }
}

/// Opens the system settings page of this application, where a blocked
/// microphone can be allowed.
pub fn open_microphone_settings() {
    if let Some(access) = PLATFORM.get() {
        access.open_settings();
    }
}

/// The platform's audio readied for recording, from [`prepare_microphone`].
/// Keep it as long as the microphone stream: when the last one is dropped,
/// the platform releases its audio for other apps.
#[must_use = "dropping it releases the audio readied for recording"]
#[derive(Debug)]
pub struct PreparedMicrophone {
    _private: (),
}

/// The number of [`PreparedMicrophone`]s alive.
static PREPARED: Mutex<usize> = Mutex::new(0);

/// Readies the platform's audio for recording; call it before opening a
/// microphone stream. On iOS and watchOS it sets the audio session to record
/// and play and activates it; elsewhere it does nothing.
pub fn prepare_microphone() -> Result<PreparedMicrophone, String> {
    let mut prepared = PREPARED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(access) = PLATFORM.get() {
        access.prepare_recording()?;
    }
    *prepared += 1;
    Ok(PreparedMicrophone { _private: () })
}

impl Drop for PreparedMicrophone {
    fn drop(&mut self) {
        let mut prepared = PREPARED.lock().unwrap_or_else(PoisonError::into_inner);
        *prepared -= 1;
        if *prepared == 0
            && let Some(access) = PLATFORM.get()
        {
            access.finish_recording();
        }
    }
}

type Observer = Arc<dyn Fn(MicrophonePermission) + Send + Sync>;

static NEXT_OBSERVER: AtomicU64 = AtomicU64::new(1);

fn observers() -> &'static Mutex<Vec<(u64, Observer)>> {
    static OBSERVERS: OnceLock<Mutex<Vec<(u64, Observer)>>> = OnceLock::new();
    OBSERVERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Tells the observers the permission as it is now. Backends call this when
/// the person answers the prompt.
pub fn publish_microphone_permission() {
    let permission = microphone_permission();
    let listeners: Vec<Observer> = observers()
        .lock()
        .map(|observers| observers.iter().map(|(_, o)| Arc::clone(o)).collect())
        .unwrap_or_default();
    for observer in listeners {
        observer(permission);
    }
}

/// A registration that keeps an observer.
pub struct MicrophonePermissionObserver {
    id: u64,
}

impl Drop for MicrophonePermissionObserver {
    fn drop(&mut self) {
        if let Ok(mut observers) = observers().lock() {
            observers.retain(|(id, _)| *id != self.id);
        }
    }
}

/// Registers `observer` for answers, delivering the permission as it is now
/// at once.
pub fn observe_microphone_permission(
    observer: impl Fn(MicrophonePermission) + Send + Sync + 'static,
) -> MicrophonePermissionObserver {
    let id = NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
    let observer: Observer = Arc::new(observer);
    if let Ok(mut observers) = observers().lock() {
        observers.push((id, Arc::clone(&observer)));
    }
    observer(microphone_permission());
    MicrophonePermissionObserver { id }
}

/// The permission as observable state, read again each time the
/// application's lifecycle changes, such as on its return from the system
/// settings. This never asks.
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberMicrophonePermission() -> State<MicrophonePermission> {
    let lifecycle = crate::rememberLifecycleState().get();
    let updates = rememberEventStream(lifecycle, move |sender| {
        observe_microphone_permission(move |permission| sender.send(permission))
    });
    cranpose_core::collectAsState(updates, lifecycle, microphone_permission())
}

#[cfg(test)]
#[path = "tests/microphone_permission_tests.rs"]
mod tests;
