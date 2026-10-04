use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

/// A sensor that records what was asked of it.
#[derive(Default)]
struct Sensor {
    available: bool,
    permission: Mutex<HeartRatePermission>,
    requests: AtomicUsize,
    starts: AtomicUsize,
    stops: AtomicUsize,
    running: AtomicBool,
}

impl Sensor {
    fn present(permission: HeartRatePermission) -> Arc<Self> {
        Arc::new(Self {
            available: true,
            permission: Mutex::new(permission),
            ..Self::default()
        })
    }
}

impl HeartRateMonitor for Sensor {
    fn available(&self) -> bool {
        self.available
    }
    fn permission(&self) -> HeartRatePermission {
        *self.permission.lock().unwrap()
    }
    fn request_permission(&self) {
        self.requests.fetch_add(1, Ordering::SeqCst);
    }
    fn start(&self) -> Result<(), HeartRateError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
    }
}

#[test]
fn without_a_sensor_nothing_is_read_and_nothing_is_asked() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_heart_rate();
    assert!(!heart_rate_available());
    assert_eq!(heart_rate_permission(), HeartRatePermission::Denied);
    request_heart_rate_permission();
    let reader = observe_heart_rate(true, |_| {});
    assert_eq!(heart_rate().status, HeartRateStatus::Unavailable);
    drop(reader);
}

#[test]
fn reading_never_asks_and_asking_is_explicit() {
    let _guard = crate::registry::test_service_guard();
    let sensor = Sensor::present(HeartRatePermission::NotAsked);
    set_platform_heart_rate(sensor.clone());
    let reader = observe_heart_rate(true, |_| {});
    assert_eq!(heart_rate().status, HeartRateStatus::NeedsPermission);
    assert_eq!(sensor.requests.load(Ordering::SeqCst), 0, "observing asked");
    assert_eq!(sensor.starts.load(Ordering::SeqCst), 0);

    request_heart_rate_permission();
    assert_eq!(sensor.requests.load(Ordering::SeqCst), 1);
    *sensor.permission.lock().unwrap() = HeartRatePermission::Granted;
    publish_heart_rate_permission(true);
    assert!(
        sensor.running.load(Ordering::SeqCst),
        "the waiting reader starts it"
    );
    assert_eq!(heart_rate().status, HeartRateStatus::Acquiring);

    publish_heart_rate(HeartRate {
        status: HeartRateStatus::Live,
        bpm: Some(72.0),
    });
    assert_eq!(heart_rate().live_bpm(), Some(72.0));
    request_heart_rate_permission();
    assert_eq!(
        sensor.requests.load(Ordering::SeqCst),
        1,
        "granted: not asked again"
    );

    drop(reader);
    assert!(
        !sensor.running.load(Ordering::SeqCst),
        "nobody reads: it stops"
    );
    assert_eq!(heart_rate().status, HeartRateStatus::Idle);
    clear_platform_heart_rate();
}

#[test]
fn the_sensor_runs_only_while_something_reads_it() {
    let _guard = crate::registry::test_service_guard();
    let sensor = Sensor::present(HeartRatePermission::Granted);
    set_platform_heart_rate(sensor.clone());
    let watcher = observe_heart_rate(false, |_| {});
    assert_eq!(
        sensor.starts.load(Ordering::SeqCst),
        0,
        "watching is not reading"
    );
    let first = observe_heart_rate(true, |_| {});
    let second = observe_heart_rate(true, |_| {});
    assert_eq!(
        sensor.starts.load(Ordering::SeqCst),
        1,
        "one start for two readers"
    );
    drop(first);
    assert!(sensor.running.load(Ordering::SeqCst));
    drop(second);
    assert_eq!(sensor.stops.load(Ordering::SeqCst), 1);
    drop(watcher);
    clear_platform_heart_rate();
}

#[test]
fn a_refusal_is_reported_and_the_sensor_stays_off() {
    let _guard = crate::registry::test_service_guard();
    let sensor = Sensor::present(HeartRatePermission::Denied);
    set_platform_heart_rate(sensor.clone());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let reader = observe_heart_rate(true, move |reading| {
        log.lock().unwrap().push(reading.status);
    });
    publish_heart_rate_permission(false);
    assert_eq!(heart_rate().status, HeartRateStatus::Denied);
    assert_eq!(sensor.starts.load(Ordering::SeqCst), 0);
    assert!(seen.lock().unwrap().contains(&HeartRateStatus::Denied));
    drop(reader);
    clear_platform_heart_rate();
}
