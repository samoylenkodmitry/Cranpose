use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use cranpose_core::ProcessIds;
use parking_lot::Mutex;

/// Whether the application may read the sensor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DeviceMotionPermission {
    /// The person has not been asked, or the answer is not known yet.
    #[default]
    NotAsked,
    /// The sensor may be read.
    Granted,
    /// The person said no.
    Denied,
}

/// What the motion sensor is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DeviceMotionStatus {
    /// This platform has no rotation sensor, or the build did not declare the
    /// service.
    #[default]
    Unavailable,
    /// The sensor is there. The application has not been allowed to read it
    /// yet: call [`request_device_motion_permission`].
    ///
    /// [`request_device_motion_permission`]: super::request_device_motion_permission
    NeedsPermission,
    /// The person said no.
    Denied,
    /// Allowed, and not running because nothing reads it.
    Idle,
    /// Running, without a sample yet.
    Acquiring,
    /// Running, and [`latest_device_motion`](super::latest_device_motion)
    /// returns samples.
    Live,
}

impl DeviceMotionStatus {
    const fn code(self) -> u8 {
        match self {
            Self::Unavailable => 0,
            Self::NeedsPermission => 1,
            Self::Denied => 2,
            Self::Idle => 3,
            Self::Acquiring => 4,
            Self::Live => 5,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::NeedsPermission,
            2 => Self::Denied,
            3 => Self::Idle,
            4 => Self::Acquiring,
            5 => Self::Live,
            _ => Self::Unavailable,
        }
    }
}

/// Why the sensor could not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceMotionError {
    /// The device has no rotation sensor, or the build did not declare the
    /// service.
    Unavailable,
    /// The platform failed to start the sensor. The text describes the cause.
    Failed(String),
}

type Observer = Arc<dyn Fn(DeviceMotionStatus) + Send + Sync>;

static STATUS: AtomicU8 = AtomicU8::new(DeviceMotionStatus::Unavailable.code());
static NEXT_OBSERVER: ProcessIds = ProcessIds::starting_at(1);
static OBSERVERS: Mutex<Vec<(u64, Observer)>> = Mutex::new(Vec::new());

/// What the sensor is doing now.
pub fn device_motion_status() -> DeviceMotionStatus {
    DeviceMotionStatus::from_code(STATUS.load(Ordering::Acquire))
}

// Stores the status and says whether it changed. A caller that holds a lock
// announces the change after it lets go, so no observer runs under the lock.
pub(super) fn store_status(status: DeviceMotionStatus) -> bool {
    STATUS.swap(status.code(), Ordering::AcqRel) != status.code()
}

pub(super) fn announce(changed: bool) {
    if changed {
        notify(device_motion_status());
    }
}

pub(super) fn set_status(status: DeviceMotionStatus) {
    announce(store_status(status));
}

// The first sample of a run, which turns acquiring into live.
pub(super) fn mark_live() {
    let acquiring = DeviceMotionStatus::Acquiring.code();
    let live = DeviceMotionStatus::Live.code();
    if STATUS.load(Ordering::Relaxed) == acquiring
        && STATUS
            .compare_exchange(acquiring, live, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    {
        notify(DeviceMotionStatus::Live);
    }
}

// Back to acquiring when the platform stopped delivering during a run.
pub(super) fn mark_acquiring() {
    let live = DeviceMotionStatus::Live.code();
    let acquiring = DeviceMotionStatus::Acquiring.code();
    if STATUS
        .compare_exchange(live, acquiring, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        notify(DeviceMotionStatus::Acquiring);
    }
}

fn notify(status: DeviceMotionStatus) {
    // The observers are copied out so none of them runs under the lock.
    let listeners: Vec<Observer> = OBSERVERS
        .lock()
        .iter()
        .map(|(_, observer)| Arc::clone(observer))
        .collect();
    for observer in listeners {
        observer(status);
    }
}

/// A registration of a status observer. Dropping it removes the observer.
pub struct DeviceMotionStatusObserver {
    id: u64,
}

impl Drop for DeviceMotionStatusObserver {
    fn drop(&mut self) {
        OBSERVERS.lock().retain(|(id, _)| *id != self.id);
    }
}

/// Calls `observer` with the status now and with each change, from whichever
/// thread changes it.
pub fn observe_device_motion_status(
    observer: impl Fn(DeviceMotionStatus) + Send + Sync + 'static,
) -> DeviceMotionStatusObserver {
    let id = NEXT_OBSERVER.next();
    let observer: Observer = Arc::new(observer);
    OBSERVERS.lock().push((id, Arc::clone(&observer)));
    observer(device_motion_status());
    DeviceMotionStatusObserver { id }
}
