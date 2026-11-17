use parking_lot::Mutex;

use super::{
    sample::MotionConfig,
    sensor::{latest, platform},
    status::{DeviceMotionPermission, DeviceMotionStatus, set_status},
};

// The leases that want the sensor, and what the sensor runs as.
struct Session {
    next_id: u64,
    leases: Vec<(u64, MotionConfig)>,
    running: Option<MotionConfig>,
}

// The lock lasts across the call into the backend, because a start and a stop
// must reach the backend in the order the leases came and went. Backends
// return from `start` and `stop` without calling back into this module.
static SESSION: Mutex<Session> = Mutex::new(Session {
    next_id: 1,
    leases: Vec::new(),
    running: None,
});

/// A claim on the sensor. The sensor runs while at least one lease lives and
/// stops when the last one is dropped.
pub struct DeviceMotionLease {
    id: u64,
}

impl Drop for DeviceMotionLease {
    fn drop(&mut self) {
        let mut session = SESSION.lock();
        session.leases.retain(|(id, _)| *id != self.id);
        reconcile(&mut session);
    }
}

/// Claims the sensor for as long as the returned lease lives. It does not
/// ask the person for permission; see [`request_device_motion_permission`].
///
/// Leases share one sensor. While several live it runs at the fastest rate
/// any of them asks for, and with the reference of the oldest. A sample says
/// which reference it carries.
///
/// [`request_device_motion_permission`]: super::request_device_motion_permission
pub fn start_device_motion(config: MotionConfig) -> DeviceMotionLease {
    let mut session = SESSION.lock();
    let id = session.next_id;
    session.next_id += 1;
    session.leases.push((id, config));
    reconcile(&mut session);
    DeviceMotionLease { id }
}

fn wanted(leases: &[(u64, MotionConfig)]) -> Option<MotionConfig> {
    let (_, first) = leases.first()?;
    let rate = leases
        .iter()
        .fold(first.rate, |rate, (_, config)| rate.or_faster(config.rate));
    Some(MotionConfig {
        reference: first.reference,
        rate,
    })
}

// What the sensor is doing when no lease runs it.
fn resting_status() -> DeviceMotionStatus {
    match platform() {
        None => DeviceMotionStatus::Unavailable,
        Some(sensor) => match sensor.permission() {
            DeviceMotionPermission::Granted => DeviceMotionStatus::Idle,
            DeviceMotionPermission::Denied => DeviceMotionStatus::Denied,
            DeviceMotionPermission::NotAsked => DeviceMotionStatus::NeedsPermission,
        },
    }
}

pub(super) fn forget_running() {
    SESSION.lock().running = None;
}

pub(super) fn refresh() {
    let mut session = SESSION.lock();
    reconcile(&mut session);
    if session.running.is_none() {
        set_status(resting_status());
    }
}

// Brings the sensor to what the leases ask.
fn reconcile(session: &mut Session) {
    let wanted = wanted(&session.leases);
    if wanted == session.running {
        return;
    }
    let Some(sensor) = platform() else {
        session.running = None;
        set_status(DeviceMotionStatus::Unavailable);
        return;
    };
    let Some(config) = wanted else {
        sensor.stop();
        latest().end();
        session.running = None;
        set_status(resting_status());
        return;
    };
    if sensor.permission() != DeviceMotionPermission::Granted {
        set_status(resting_status());
        return;
    }
    if session.running.is_none() {
        latest().begin();
        set_status(DeviceMotionStatus::Acquiring);
    }
    match sensor.start(config) {
        Ok(()) => session.running = Some(config),
        Err(error) => {
            log::warn!("cranpose: the motion sensor did not start: {error:?}");
            latest().end();
            session.running = None;
            set_status(DeviceMotionStatus::Unavailable);
        }
    }
}
