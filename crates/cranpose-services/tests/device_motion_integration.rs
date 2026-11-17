use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_services::{
    DeviceMotion, DeviceMotionError, DeviceMotionPermission, DeviceMotionSensor,
    DeviceMotionStatus, MotionClock, MotionConfig, MotionRate, MotionReference, Quaternion,
    clear_platform_device_motion, device_motion_available, device_motion_interrupted,
    device_motion_now_ns, device_motion_permission, device_motion_status, latest_device_motion,
    observe_device_motion_status, publish_device_motion, publish_device_motion_permission,
    rememberDeviceMotion, request_device_motion_permission, set_platform_device_motion,
    start_device_motion,
};

use crate::composition_support::pump_until;

fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn one_sensor_at_a_time() -> MutexGuard<'static, ()> {
    static SENSOR: Mutex<()> = Mutex::new(());
    SENSOR.lock().unwrap_or_else(PoisonError::into_inner)
}

// A sensor that records what was asked of it.
struct Fake {
    permission: Mutex<DeviceMotionPermission>,
    requests: AtomicUsize,
    starts: Mutex<Vec<MotionConfig>>,
    stops: AtomicUsize,
    running: AtomicBool,
    now: AtomicU64,
}

impl Fake {
    fn with(permission: DeviceMotionPermission) -> Arc<Self> {
        Arc::new(Self {
            permission: Mutex::new(permission),
            requests: AtomicUsize::new(0),
            starts: Mutex::new(Vec::new()),
            stops: AtomicUsize::new(0),
            running: AtomicBool::new(false),
            now: AtomicU64::new(0),
        })
    }

    fn granted() -> Arc<Self> {
        Self::with(DeviceMotionPermission::Granted)
    }

    fn starts(&self) -> Vec<MotionConfig> {
        held(&self.starts).clone()
    }
}

impl DeviceMotionSensor for Fake {
    fn clock(&self) -> MotionClock {
        MotionClock::PerformanceNow
    }
    fn now_ns(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
    fn available(&self) -> bool {
        true
    }
    fn permission(&self) -> DeviceMotionPermission {
        *held(&self.permission)
    }
    fn request_permission(&self) {
        self.requests.fetch_add(1, Ordering::SeqCst);
    }
    fn start(&self, config: MotionConfig) -> Result<(), DeviceMotionError> {
        held(&self.starts).push(config);
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
    }
}

const SOME_ROTATION: Quaternion = Quaternion::new(0.0, 0.0, 0.6, 0.8);

fn period(millis: u64) -> MotionConfig {
    MotionConfig {
        reference: MotionReference::ArbitraryHeading,
        rate: MotionRate::Period(Duration::from_millis(millis)),
    }
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1.0e-5)
}

#[test]
fn without_a_sensor_nothing_is_available_and_nothing_starts() {
    let _one = one_sensor_at_a_time();
    clear_platform_device_motion();

    assert!(!device_motion_available());
    assert_eq!(device_motion_status(), DeviceMotionStatus::Unavailable);
    assert_eq!(device_motion_now_ns(), None);
    assert_eq!(device_motion_permission(), DeviceMotionPermission::Denied);
    let lease = start_device_motion(MotionConfig::default());
    publish_device_motion(SOME_ROTATION, 7, MotionReference::ArbitraryHeading);
    assert_eq!(latest_device_motion(), None);
    assert_eq!(device_motion_status(), DeviceMotionStatus::Unavailable);
    drop(lease);
}

#[test]
fn the_sensor_runs_only_while_a_lease_lives_and_leases_share_it() {
    let _one = one_sensor_at_a_time();
    let sensor = Fake::granted();
    set_platform_device_motion(Arc::<Fake>::clone(&sensor));
    assert_eq!(device_motion_status(), DeviceMotionStatus::Idle);
    assert!(sensor.starts().is_empty(), "nothing reads: it stays off");

    let slow = start_device_motion(period(50));
    assert_eq!(sensor.starts(), vec![period(50)]);
    let fast = start_device_motion(MotionConfig {
        reference: MotionReference::MagneticNorth,
        rate: MotionRate::Fastest,
    });
    assert_eq!(
        sensor.starts().last(),
        Some(&MotionConfig {
            reference: MotionReference::ArbitraryHeading,
            rate: MotionRate::Fastest,
        }),
        "the rate of the fastest reader, the reference of the oldest"
    );
    drop(fast);
    assert_eq!(
        sensor.starts().last(),
        Some(&period(50)),
        "back to what the remaining reader asked"
    );
    assert!(sensor.running.load(Ordering::SeqCst));
    assert_eq!(sensor.stops.load(Ordering::SeqCst), 0);
    drop(slow);
    assert!(!sensor.running.load(Ordering::SeqCst));
    assert_eq!(sensor.stops.load(Ordering::SeqCst), 1);
    assert_eq!(device_motion_status(), DeviceMotionStatus::Idle);
    clear_platform_device_motion();
}

#[test]
fn the_newest_sample_carries_its_clock_and_a_running_count_and_dies_with_the_sensor() {
    let _one = one_sensor_at_a_time();
    let sensor = Fake::granted();
    set_platform_device_motion(Arc::<Fake>::clone(&sensor));
    sensor.now.store(1_000, Ordering::SeqCst);
    assert_eq!(device_motion_now_ns(), Some(1_000));

    let lease = start_device_motion(MotionConfig::default());
    assert_eq!(device_motion_status(), DeviceMotionStatus::Acquiring);
    assert_eq!(latest_device_motion(), None, "no sample before the first");
    publish_device_motion(SOME_ROTATION, 500, MotionReference::ArbitraryHeading);
    let first = latest_device_motion().expect("a sample");
    assert_eq!(first.rotation, SOME_ROTATION);
    assert_eq!(first.timestamp_ns, 500);
    assert_eq!(first.clock, MotionClock::PerformanceNow);
    assert_eq!(first.reference, MotionReference::ArbitraryHeading);
    assert_eq!(first.index, 1);
    assert_eq!(device_motion_status(), DeviceMotionStatus::Live);

    publish_device_motion(Quaternion::IDENTITY, 505, MotionReference::MagneticNorth);
    let second = latest_device_motion().expect("a sample");
    assert_eq!(second.index, 2);
    assert_eq!(second.reference, MotionReference::MagneticNorth);

    drop(lease);
    assert_eq!(
        latest_device_motion(),
        None,
        "a stopped sensor has no sample"
    );
    publish_device_motion(SOME_ROTATION, 510, MotionReference::ArbitraryHeading);
    assert_eq!(
        latest_device_motion(),
        None,
        "a late delivery does not bring it back"
    );

    let again = start_device_motion(MotionConfig::default());
    publish_device_motion(SOME_ROTATION, 900, MotionReference::ArbitraryHeading);
    assert_eq!(latest_device_motion().expect("a sample").index, 1);
    drop(again);
    clear_platform_device_motion();
}

#[test]
fn a_platform_that_stops_delivering_leaves_no_stale_sample() {
    let _one = one_sensor_at_a_time();
    set_platform_device_motion(Fake::granted());
    let lease = start_device_motion(MotionConfig::default());
    publish_device_motion(SOME_ROTATION, 1, MotionReference::ArbitraryHeading);
    assert_eq!(device_motion_status(), DeviceMotionStatus::Live);

    device_motion_interrupted();
    assert_eq!(latest_device_motion(), None);
    assert_eq!(device_motion_status(), DeviceMotionStatus::Acquiring);

    publish_device_motion(SOME_ROTATION, 2, MotionReference::ArbitraryHeading);
    assert_eq!(latest_device_motion().expect("a sample").timestamp_ns, 2);
    assert_eq!(device_motion_status(), DeviceMotionStatus::Live);
    drop(lease);
    clear_platform_device_motion();
}

#[test]
fn a_sensor_that_needs_permission_starts_only_after_the_person_agrees() {
    let _one = one_sensor_at_a_time();
    let sensor = Fake::with(DeviceMotionPermission::NotAsked);
    set_platform_device_motion(Arc::<Fake>::clone(&sensor));
    assert_eq!(device_motion_status(), DeviceMotionStatus::NeedsPermission);

    let lease = start_device_motion(MotionConfig::default());
    assert!(sensor.starts().is_empty(), "reading never asks or starts");
    assert_eq!(sensor.requests.load(Ordering::SeqCst), 0);
    assert_eq!(device_motion_status(), DeviceMotionStatus::NeedsPermission);

    request_device_motion_permission();
    assert_eq!(sensor.requests.load(Ordering::SeqCst), 1);
    *held(&sensor.permission) = DeviceMotionPermission::Granted;
    publish_device_motion_permission(true);
    assert!(
        sensor.running.load(Ordering::SeqCst),
        "the reader starts it"
    );
    assert_eq!(device_motion_status(), DeviceMotionStatus::Acquiring);
    request_device_motion_permission();
    assert_eq!(
        sensor.requests.load(Ordering::SeqCst),
        1,
        "allowed: nobody is asked again"
    );
    drop(lease);
    clear_platform_device_motion();
}

#[test]
fn a_refusal_is_reported_and_the_sensor_stays_off() {
    let _one = one_sensor_at_a_time();
    let sensor = Fake::with(DeviceMotionPermission::NotAsked);
    set_platform_device_motion(Arc::<Fake>::clone(&sensor));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let observer = observe_device_motion_status(move |status| held(&log).push(status));
    let lease = start_device_motion(MotionConfig::default());

    *held(&sensor.permission) = DeviceMotionPermission::Denied;
    publish_device_motion_permission(false);
    assert_eq!(device_motion_status(), DeviceMotionStatus::Denied);
    assert!(sensor.starts().is_empty());
    assert_eq!(
        held(&seen).as_slice(),
        [
            DeviceMotionStatus::NeedsPermission,
            DeviceMotionStatus::Denied
        ]
    );
    drop(observer);
    drop(lease);
    clear_platform_device_motion();
}

#[test]
fn a_reader_on_another_thread_never_sees_a_torn_sample() {
    let _one = one_sensor_at_a_time();
    set_platform_device_motion(Fake::granted());
    let lease = start_device_motion(MotionConfig::default());
    const SAMPLES: u32 = 300_000;
    let writer = std::thread::spawn(|| {
        for k in 1..=SAMPLES {
            let value = k as f32;
            publish_device_motion(
                Quaternion::new(value, value, value, value),
                u64::from(k),
                MotionReference::ArbitraryHeading,
            );
        }
    });
    let mut last_index = 0;
    let mut reads = 0u64;
    while !writer.is_finished() {
        if let Some(sample) = latest_device_motion() {
            let Quaternion { x, y, z, w } = sample.rotation;
            assert!(
                x == y && y == z && z == w && x as u64 == sample.timestamp_ns,
                "a torn sample: {sample:?}"
            );
            assert_eq!(sample.index, sample.timestamp_ns, "index of another sample");
            assert!(sample.index >= last_index, "the newest sample went back");
            last_index = sample.index;
            reads += 1;
        }
    }
    writer.join().expect("the writer finishes");
    assert!(reads > 0, "the reader saw samples");
    assert_eq!(
        latest_device_motion().expect("a sample").index,
        u64::from(SAMPLES)
    );
    drop(lease);
    clear_platform_device_motion();
}

#[test]
fn a_composition_runs_the_sensor_while_it_holds_the_call() {
    let _one = one_sensor_at_a_time();
    let sensor = Fake::granted();
    set_platform_device_motion(Arc::<Fake>::clone(&sensor));
    let handle: Rc<RefCell<Option<DeviceMotion>>> = Rc::new(RefCell::new(None));
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    {
        let handle = Rc::clone(&handle);
        composition
            .render(key, move || {
                *handle.borrow_mut() = Some(rememberDeviceMotion(MotionConfig::default()));
            })
            .expect("render succeeds");
    }
    let motion = handle.borrow().expect("the call ran");
    assert!(sensor.running.load(Ordering::SeqCst));
    assert!(pump_until(&mut composition, || {
        motion.status().get_non_reactive() == DeviceMotionStatus::Acquiring
    }));

    publish_device_motion(SOME_ROTATION, 42, MotionReference::ArbitraryHeading);
    assert!(pump_until(&mut composition, || {
        motion.status().get_non_reactive() == DeviceMotionStatus::Live
    }));
    assert_eq!(motion.latest().expect("a sample").timestamp_ns, 42);

    composition
        .render(key, || {})
        .expect("the call leaves the composition");
    assert!(
        !sensor.running.load(Ordering::SeqCst),
        "the call left: the sensor stops"
    );
    assert_eq!(latest_device_motion(), None);
    clear_platform_device_motion();
}

#[test]
fn a_device_lying_face_up_is_the_identity_and_upright_turns_y_onto_up() {
    // A web page reports a flat device with all three angles zero and an
    // upright one with beta of ninety degrees.
    let flat = Quaternion::from_device_orientation(0.0, 0.0, 0.0);
    assert!(close(flat.rotate([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]));
    let upright = Quaternion::from_device_orientation(0.0, 90.0, 0.0);
    assert!(close(upright.rotate([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
    assert!(
        close(upright.rotate([0.0, 0.0, 1.0]), [0.0, -1.0, 0.0]),
        "the screen faces the person: the device z points back along the reference -y"
    );

    // Android's rotation vector for a quarter turn about x, with and without
    // its fourth value.
    let turn = std::f32::consts::FRAC_PI_4;
    let with_w = Quaternion::from_rotation_vector(turn.sin(), 0.0, 0.0, Some(turn.cos()));
    let without_w = Quaternion::from_rotation_vector(turn.sin(), 0.0, 0.0, None);
    for rotation in [with_w, without_w] {
        assert!(close(rotation.rotate([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!((rotation.length() - 1.0).abs() < 1.0e-6);
    }
    assert!(close(
        Quaternion::IDENTITY.rotate([1.0, 2.0, 3.0]),
        [1.0, 2.0, 3.0]
    ));
}

#[test]
fn the_angles_of_a_web_page_compose_in_the_order_the_standard_names() {
    let (alpha, beta, gamma) = (30.0_f64, 50.0_f64, 20.0_f64);
    let about = |axis: [f32; 3], degrees: f64| {
        let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
        Quaternion::new(
            axis[0] * sin as f32,
            axis[1] * sin as f32,
            axis[2] * sin as f32,
            cos as f32,
        )
    };
    // z by alpha, then the turned x by beta, then the twice turned y by
    // gamma: each later turn is on the right of the product.
    let composed = about([0.0, 0.0, 1.0], alpha)
        * about([1.0, 0.0, 0.0], beta)
        * about([0.0, 1.0, 0.0], gamma);
    let reported = Quaternion::from_device_orientation(alpha, beta, gamma);
    for vector in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        assert!(close(composed.rotate(vector), reported.rotate(vector)));
    }
}

#[test]
fn core_motion_states_the_opposite_rotation_and_the_service_reverses_it() {
    // Apple states the attitude from the reference frame to the device. For
    // a device held upright the reference up axis lands on the device y
    // axis, so the stated rotation takes the reference z to the device y.
    let upright = Quaternion::from_device_orientation(0.0, 90.0, 0.0);
    let stated = upright.conjugate();
    assert!(close(stated.rotate([0.0, 0.0, 1.0]), [0.0, 1.0, 0.0]));
    let converted = Quaternion::from_core_motion(
        f64::from(stated.x),
        f64::from(stated.y),
        f64::from(stated.z),
        f64::from(stated.w),
    );
    assert!(close(converted.rotate([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
    // Gravity points down in the reference frame; in the device frame of an
    // upright device it points toward the bottom of the screen.
    assert!(close(
        converted.conjugate().rotate([0.0, 0.0, -1.0]),
        [0.0, -1.0, 0.0]
    ));
}

#[test]
fn a_quaternion_that_is_not_finite_falls_back_to_no_rotation() {
    assert_eq!(
        Quaternion::new(f32::NAN, 0.0, 0.0, 1.0).normalized(),
        Quaternion::IDENTITY
    );
    assert_eq!(
        Quaternion::new(0.0, 0.0, 0.0, 0.0).normalized(),
        Quaternion::IDENTITY
    );
}
