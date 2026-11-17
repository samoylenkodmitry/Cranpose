use cranpose_core::{rememberMutableStateOf, LaunchedEffect};
use cranpose_services::{
    device_motion_display_rotation, device_motion_now_ns, latest_device_motion,
    rememberDeviceMotion, rememberDeviceMotionSample, request_device_motion_permission,
    DeviceMotionStatus, MotionClock, MotionConfig, MotionSample,
};
use cranpose_ui::{
    composable, Box, BoxSpec, Button, ButtonSpec, Color, GraphicsLayer, Modifier, Spacer, Text,
    TextStyle,
};

// How long a window of samples is measured before its numbers are shown.
const WINDOW_NANOS: u64 = 1_000_000_000;

// What one window of samples measured.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Rate {
    hertz: f32,
    // The age of the newest sample at each frame of the window, in
    // milliseconds: now on the sample's clock, minus its timestamp.
    min_age_ms: f32,
    mean_age_ms: f32,
    max_age_ms: f32,
}

// The samples seen from one frame to the next, until `WINDOW_NANOS` of the
// sensor's own time have passed.
struct Window {
    first: MotionSample,
    last: MotionSample,
    frames: u32,
    age_sum_ns: i64,
    min_age_ns: i64,
    max_age_ns: i64,
}

impl Window {
    fn new(sample: MotionSample, age_ns: i64) -> Self {
        Self {
            first: sample,
            last: sample,
            frames: 1,
            age_sum_ns: age_ns,
            min_age_ns: age_ns,
            max_age_ns: age_ns,
        }
    }

    fn add(&mut self, sample: MotionSample, age_ns: i64) {
        self.last = sample;
        self.frames += 1;
        self.age_sum_ns += age_ns;
        self.min_age_ns = self.min_age_ns.min(age_ns);
        self.max_age_ns = self.max_age_ns.max(age_ns);
    }

    fn rate(&self) -> Option<Rate> {
        let span = self
            .last
            .timestamp_ns
            .checked_sub(self.first.timestamp_ns)?;
        if span < WINDOW_NANOS {
            return None;
        }
        let samples = self.last.index - self.first.index;
        let milliseconds = |nanos: i64| nanos as f32 * 1.0e-6;
        Some(Rate {
            hertz: samples as f32 / (span as f32 * 1.0e-9),
            min_age_ms: milliseconds(self.min_age_ns),
            mean_age_ms: milliseconds(self.age_sum_ns / i64::from(self.frames)),
            max_age_ms: milliseconds(self.max_age_ns),
        })
    }
}

// The device's rotation: a plate that turns with it, over the status, the
// sample rate, the age of the newest sample and the quaternion itself.
#[composable]
pub(super) fn DeviceMotionDemo() {
    let motion = rememberDeviceMotion(MotionConfig::default());
    let sample = rememberDeviceMotionSample(motion);
    let rate = rememberMutableStateOf(Rate::default);
    let status = motion.status().get();
    let live = status == DeviceMotionStatus::Live;
    LaunchedEffect(live, async move |scope| {
        if !live {
            return;
        }
        let mut window: Option<Window> = None;
        loop {
            scope.with_frame_nanos(|_| ()).await;
            let (Some(newest), Some(now)) = (latest_device_motion(), device_motion_now_ns()) else {
                continue;
            };
            let age_ns = now as i64 - newest.timestamp_ns as i64;
            let seen = window.get_or_insert_with(|| Window::new(newest, age_ns));
            seen.add(newest, age_ns);
            if let Some(measured) = seen.rate() {
                log::info!(
                    "device_motion: {:.1} Hz, age at frame min {:.2} mean {:.2} max {:.2} ms, \
                     {:?} clock, index {}",
                    measured.hertz,
                    measured.min_age_ms,
                    measured.mean_age_ms,
                    measured.max_age_ms,
                    newest.clock,
                    newest.index,
                );
                rate.set(measured);
                window = None;
            }
        }
    });

    Text(
        status_line(status),
        Modifier::empty()
            .background(Color(0.1, 0.18, 0.32, 0.6))
            .rounded_corners(14.0)
            .padding(10.0),
        TextStyle::default(),
    );
    if status == DeviceMotionStatus::NeedsPermission {
        Spacer(Modifier::empty().size_points(0.0, 12.0));
        Button(
            Modifier::empty()
                .rounded_corners(16.0)
                .background(Color(0.15, 0.35, 0.85, 1.0))
                .padding(12.0),
            ButtonSpec::default(),
            request_device_motion_permission,
            || {
                Text(
                    "Allow motion",
                    Modifier::empty().padding(6.0),
                    TextStyle::default(),
                );
            },
        );
    }
    Spacer(Modifier::empty().size_points(0.0, 12.0));
    let measured = rate.get();
    Text(
        format!(
            "{:.1} Hz, age {:.1} ms (min {:.1}, max {:.1})",
            measured.hertz, measured.mean_age_ms, measured.min_age_ms, measured.max_age_ms
        ),
        Modifier::empty().padding(6.0),
        TextStyle::default(),
    );
    Text(
        sample_line(sample.get()),
        Modifier::empty().padding(6.0),
        TextStyle::default(),
    );
    Spacer(Modifier::empty().size_points(0.0, 12.0));
    // The layer reads the sample as the scene is drawn, so the plate follows
    // the device without recomposing.
    Box(
        Modifier::empty()
            .size_points(140.0, 140.0)
            .graphics_layer(move || plate(sample.get()))
            .background(Color(0.15, 0.55, 0.45, 1.0))
            .rounded_corners(16.0),
        BoxSpec::default(),
        || {
            Text(
                "front",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

fn status_line(status: DeviceMotionStatus) -> &'static str {
    match status {
        DeviceMotionStatus::Unavailable => "This build has no rotation sensor to read",
        DeviceMotionStatus::NeedsPermission => "The rotation sensor needs permission",
        DeviceMotionStatus::Denied => "The rotation sensor was refused",
        DeviceMotionStatus::Idle => "The rotation sensor is off",
        DeviceMotionStatus::Acquiring => "The rotation sensor waits for a sample",
        DeviceMotionStatus::Live => "The rotation sensor reports",
    }
}

fn sample_line(sample: Option<MotionSample>) -> String {
    let Some(sample) = sample else {
        return "No sample".to_string();
    };
    let clock = match sample.clock {
        MotionClock::BootTime => "boot",
        MotionClock::SystemUptime => "uptime",
        MotionClock::PerformanceNow => "page",
    };
    let q = sample.rotation;
    let up = q.conjugate().rotate([0.0, 0.0, 1.0]);
    let screen = q
        .in_screen_frame(device_motion_display_rotation())
        .conjugate()
        .rotate([0.0, 0.0, 1.0]);
    format!(
        "q ({:+.3}, {:+.3}, {:+.3}, {:+.3}) t {:.3} s on {clock} clock, #{}\nup in the device ({:+.2}, {:+.2}, {:+.2})\nup in the screen ({:+.2}, {:+.2}, {:+.2})",
        q.x,
        q.y,
        q.z,
        q.w,
        sample.timestamp_ns as f64 * 1.0e-9,
        sample.index,
        up[0],
        up[1],
        up[2],
        screen[0],
        screen[1],
        screen[2],
    )
}

// The plate as the device would see a plate fixed to the world: tilted by the
// way the device leans and turned by its heading.
fn plate(sample: Option<MotionSample>) -> GraphicsLayer {
    let Some(sample) = sample else {
        return GraphicsLayer::default();
    };
    // The plate is drawn on the screen, so it follows the screen frame, which
    // differs from the device frame when the person turned the device.
    let rotation = sample
        .rotation
        .in_screen_frame(device_motion_display_rotation());
    let up = rotation.conjugate().rotate([0.0, 0.0, 1.0]);
    let top = rotation.rotate([0.0, 1.0, 0.0]);
    GraphicsLayer {
        rotation_x: up[1].atan2(up[2]).to_degrees(),
        rotation_y: (-up[0]).atan2(up[2]).to_degrees(),
        rotation_z: (-top[0]).atan2(top[1]).to_degrees(),
        ..GraphicsLayer::default()
    }
}
