use cranpose_core::{
    LaunchedEffect, State, collectAsState, rememberEventStream, rememberMutableStateOf,
};

use super::{
    lease::start_device_motion,
    sample::{MotionConfig, MotionSample},
    sensor::latest_device_motion,
    status::{DeviceMotionStatus, device_motion_status, observe_device_motion_status},
};

/// The handle that [`rememberDeviceMotion`] returns. It is `Copy`, and it
/// stays on the composition's thread because its state does.
#[derive(Clone, Copy)]
pub struct DeviceMotion {
    status: State<DeviceMotionStatus>,
}

impl DeviceMotion {
    /// What the sensor is doing. It changes when the condition of the sensor
    /// changes and never once per sample.
    pub fn status(&self) -> State<DeviceMotionStatus> {
        self.status
    }

    /// The newest sample; see [`latest_device_motion`], which any thread can
    /// call without this handle.
    pub fn latest(&self) -> Option<MotionSample> {
        latest_device_motion()
    }
}

/// Runs the motion sensor for as long as this call stays in the composition.
///
/// The sensor runs, if the platform allows it. This never asks the person; see
/// [`request_device_motion_permission`](super::request_device_motion_permission).
/// Leaving the composition stops the sensor when no other reader needs it, so
/// a screen that reads the rotation only sometimes calls this under an `if`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberDeviceMotion(config: MotionConfig) -> DeviceMotion {
    let updates = rememberEventStream(config, move |sender| {
        let lease = start_device_motion(config);
        let observer = observe_device_motion_status(move |status| sender.send(status));
        (lease, observer)
    });
    DeviceMotion {
        status: collectAsState(updates, (), device_motion_status()),
    }
}

/// The newest sample as state, refreshed once per frame while the sensor is
/// live.
///
/// Reading the state recomposes its reader at the frame rate, and the
/// refresh keeps frames coming while the sensor runs. A view that only needs
/// the rotation as it draws reads [`DeviceMotion::latest`] there instead.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberDeviceMotionSample(motion: DeviceMotion) -> State<Option<MotionSample>> {
    let sample = rememberMutableStateOf(|| None::<MotionSample>);
    let live = motion.status().get() == DeviceMotionStatus::Live;
    LaunchedEffect(live, async move |scope| {
        if !live {
            if sample.get_non_reactive().is_some() {
                sample.set(None);
            }
            return;
        }
        let clock = scope.runtime().frame_clock();
        loop {
            clock.next_frame().await;
            let latest = latest_device_motion();
            if latest.map(|s| s.index) != sample.get_non_reactive().map(|s| s.index) {
                sample.set(latest);
            }
        }
    });
    sample.as_state()
}
