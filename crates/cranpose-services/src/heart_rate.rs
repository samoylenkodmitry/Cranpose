//! The wearer's heart rate, from the device's own sensor.
//!
//! Two things about a heart-rate sensor are the framework's to get right,
//! because an application that gets them wrong is one a store can refuse.
//!
//! **Nothing happens unless the application asks.** The permission is in the
//! build only when the build script declares
//! `cranpose_capabilities::Use::heart_rate`, and the person is asked only
//! when the application calls [`request_heart_rate_permission`]. Observing
//! the reading never prompts: an application that has not asked, or was told
//! no, sees [`HeartRateStatus::NeedsPermission`] or
//! [`HeartRateStatus::Denied`] and decides for itself what to say.
//!
//! **The sensor runs only while something reads it.** [`rememberHeartRate`]
//! starts it when it enters the composition active and stops it when it
//! leaves or turns inactive, so a screen that is not showing the pulse is
//! not paying the battery for it.
//!
//! A reading is beats per minute as the sensor reports it. A sensor that is
//! not touching skin reports nothing usable; that is
//! [`HeartRateStatus::OffBody`], not a number.

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use cranpose_core::{State, rememberEventStream};

use crate::registry::ServiceRegistry;

/// Whether the application may read the sensor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HeartRatePermission {
    /// The person has not been asked, or the answer is not known yet.
    #[default]
    NotAsked,
    Granted,
    Denied,
}

/// What the heart-rate sensor is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HeartRateStatus {
    /// This platform has no heart-rate sensor, or the build did not declare
    /// the service.
    #[default]
    Unavailable,
    /// The sensor is there, and the application has not been allowed to
    /// read it yet.
    NeedsPermission,
    /// The person said no.
    Denied,
    /// Allowed, and not running.
    Idle,
    /// Running, without a reading yet.
    Acquiring,
    /// Running, and [`HeartRate::bpm`] is the latest reading.
    Live,
    /// Running, and the sensor is not on the skin.
    OffBody,
}

/// The latest the sensor said.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeartRate {
    pub status: HeartRateStatus,
    /// Beats per minute, while [`status`](Self::status) is
    /// [`HeartRateStatus::Live`].
    pub bpm: Option<f32>,
}

impl HeartRate {
    /// The reading, when there is a live one.
    pub fn live_bpm(&self) -> Option<f32> {
        match self.status {
            HeartRateStatus::Live => self.bpm,
            _ => None,
        }
    }
}

/// Why the sensor could not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeartRateError {
    Unavailable,
    NotPermitted,
    Failed(String),
}

/// A platform's heart-rate sensor.
pub trait HeartRateMonitor: Send + Sync {
    /// Whether the device has the sensor and the build declared the service.
    fn available(&self) -> bool;
    /// Whether the application may read it.
    fn permission(&self) -> HeartRatePermission;
    /// Asks the person, once per call; the answer arrives through
    /// [`publish_heart_rate_permission`].
    fn request_permission(&self);
    /// Starts reading. Readings arrive through [`publish_heart_rate`].
    fn start(&self) -> Result<(), HeartRateError>;
    fn stop(&self);
}

pub type HeartRateMonitorRef = Arc<dyn HeartRateMonitor>;

static PLATFORM_HEART_RATE: ServiceRegistry<dyn HeartRateMonitor> = ServiceRegistry::new();

/// Installs the platform sensor, replacing any previous one.
pub fn set_platform_heart_rate(monitor: HeartRateMonitorRef) {
    PLATFORM_HEART_RATE.set(monitor);
    publish_heart_rate(resting_reading());
}

/// Removes the platform sensor (tests and teardown).
pub fn clear_platform_heart_rate() {
    PLATFORM_HEART_RATE.clear();
    READERS.store(0, Ordering::Release);
    publish_heart_rate(HeartRate::default());
}

fn monitor() -> Option<HeartRateMonitorRef> {
    PLATFORM_HEART_RATE.get()
}

/// Whether this device can read a heart rate for this application at all.
pub fn heart_rate_available() -> bool {
    monitor().is_some_and(|monitor| monitor.available())
}

/// Whether the application may read the sensor.
pub fn heart_rate_permission() -> HeartRatePermission {
    monitor()
        .filter(|monitor| monitor.available())
        .map_or(HeartRatePermission::Denied, |monitor| monitor.permission())
}

/// Asks the person for the sensor. The only call in the framework that does;
/// a no-op where there is no sensor or the build did not declare it.
pub fn request_heart_rate_permission() {
    if let Some(monitor) = monitor().filter(|monitor| monitor.available())
        && monitor.permission() != HeartRatePermission::Granted
    {
        monitor.request_permission();
    }
}

/// What the sensor is doing when nothing is reading it.
fn resting_reading() -> HeartRate {
    let status = match monitor().filter(|monitor| monitor.available()) {
        None => HeartRateStatus::Unavailable,
        Some(monitor) => match monitor.permission() {
            HeartRatePermission::Granted => HeartRateStatus::Idle,
            HeartRatePermission::Denied => HeartRateStatus::Denied,
            HeartRatePermission::NotAsked => HeartRateStatus::NeedsPermission,
        },
    };
    HeartRate { status, bpm: None }
}

type Observer = Arc<dyn Fn(HeartRate) + Send + Sync>;

static NEXT_OBSERVER: AtomicU64 = AtomicU64::new(1);
/// How many active readers want the sensor running.
static READERS: AtomicUsize = AtomicUsize::new(0);

fn slot() -> &'static Mutex<HeartRate> {
    static SLOT: OnceLock<Mutex<HeartRate>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HeartRate::default()))
}

fn observers() -> &'static Mutex<Vec<(u64, Observer)>> {
    static OBSERVERS: OnceLock<Mutex<Vec<(u64, Observer)>>> = OnceLock::new();
    OBSERVERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// The latest reading.
pub fn heart_rate() -> HeartRate {
    slot().lock().map(|reading| *reading).unwrap_or_default()
}

/// Publishes a reading. Backends call this from whichever thread the
/// platform delivers on.
pub fn publish_heart_rate(reading: HeartRate) {
    {
        let Ok(mut current) = slot().lock() else {
            return;
        };
        if *current == reading {
            return;
        }
        *current = reading;
    }
    let listeners: Vec<Observer> = observers()
        .lock()
        .map(|observers| observers.iter().map(|(_, o)| Arc::clone(o)).collect())
        .unwrap_or_default();
    for observer in listeners {
        observer(reading);
    }
}

/// Publishes the person's answer. Backends call this when the permission
/// prompt closes; a reader waiting on it starts the sensor.
pub fn publish_heart_rate_permission(granted: bool) {
    if granted && READERS.load(Ordering::Acquire) > 0 {
        start_for_readers();
    } else {
        publish_heart_rate(resting_reading());
    }
}

fn start_for_readers() {
    let Some(monitor) = monitor().filter(|monitor| monitor.available()) else {
        publish_heart_rate(HeartRate::default());
        return;
    };
    if monitor.permission() != HeartRatePermission::Granted {
        publish_heart_rate(resting_reading());
        return;
    }
    publish_heart_rate(HeartRate {
        status: HeartRateStatus::Acquiring,
        bpm: None,
    });
    if let Err(error) = monitor.start() {
        log::warn!("cranpose: the heart-rate sensor did not start: {error:?}");
        publish_heart_rate(resting_reading());
    }
}

/// A registration that keeps an observer, and when it reads, the sensor.
pub struct HeartRateReader {
    id: u64,
    reading: bool,
}

impl Drop for HeartRateReader {
    fn drop(&mut self) {
        if let Ok(mut observers) = observers().lock() {
            observers.retain(|(id, _)| *id != self.id);
        }
        if self.reading && READERS.fetch_sub(1, Ordering::AcqRel) == 1 {
            if let Some(monitor) = monitor() {
                monitor.stop();
            }
            publish_heart_rate(resting_reading());
        }
    }
}

/// Registers `observer` for readings, delivering the latest at once. With
/// `read` the sensor runs while this registration lives -- if the person
/// has allowed it; this never asks.
pub fn observe_heart_rate(
    read: bool,
    observer: impl Fn(HeartRate) + Send + Sync + 'static,
) -> HeartRateReader {
    let id = NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
    let observer: Observer = Arc::new(observer);
    if let Ok(mut observers) = observers().lock() {
        observers.push((id, Arc::clone(&observer)));
    }
    if read && READERS.fetch_add(1, Ordering::AcqRel) == 0 {
        start_for_readers();
    }
    observer(heart_rate());
    HeartRateReader { id, reading: read }
}

/// The wearer's heart rate, observed for as long as this call stays in the
/// composition. While `active` the sensor runs, if the person has allowed
/// it; this never asks -- call [`request_heart_rate_permission`] for that.
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberHeartRate(active: bool) -> State<HeartRate> {
    let updates = rememberEventStream(active, move |sender| {
        observe_heart_rate(active, move |reading| sender.send(reading))
    });
    cranpose_core::collectAsState(updates, (), heart_rate())
}

#[cfg(test)]
#[path = "tests/heart_rate_tests.rs"]
mod tests;
