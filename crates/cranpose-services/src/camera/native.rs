//! Desktop live camera via `nokhwa` (AVFoundation on macOS, Media Foundation
//! on Windows, V4L2 on Linux).
//!
//! nokhwa hands frames over on request, and the framework contract is that
//! frames are published; a capture thread bridges the two. The thread opens
//! the device, publishes what happened, and pushes every decoded frame until
//! the session stops. Nothing here blocks the caller for the length of a
//! capture: `start` returns once the thread exists, and the session's
//! progress arrives as [`CameraState`].

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

use nokhwa::{
    Camera as CaptureDevice,
    pixel_format::RgbFormat,
    utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType},
};

use super::{
    Camera, CameraError, CameraFrame, CameraLens, CameraLenses, CameraState, FrameFormat,
    LensFacing, publish_camera_frame, publish_camera_lenses, publish_camera_state,
    record_dropped_camera_frame, set_platform_camera,
};

/// Installs the built-in desktop camera as the platform camera.
pub fn install_native_camera() {
    set_platform_camera(Arc::new(NativeCamera::default()));
}

/// The devices nokhwa can open, as lenses an application can pick between.
///
/// A webcam does not face a screen the way a phone lens does, so every device
/// reports [`LensFacing::External`].
fn list_lenses() -> Vec<CameraLens> {
    match nokhwa::query(ApiBackend::Auto) {
        Ok(devices) => devices
            .into_iter()
            .filter_map(|info| match info.index() {
                CameraIndex::Index(index) => Some(CameraLens {
                    id: index.to_string(),
                    name: info.human_name(),
                    facing: LensFacing::External,
                }),
                CameraIndex::String(_) => None,
            })
            .collect(),
        Err(error) => {
            log::warn!("camera device list failed: {error}");
            Vec::new()
        }
    }
}

struct Session {
    running: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

#[derive(Default)]
struct NativeCamera {
    session: Mutex<Option<Session>>,
    /// Capture threads told to end, waited for by the next [`NativeCamera::open`]
    /// rather than by whoever stopped the session.
    retiring: Mutex<Vec<JoinHandle<()>>>,
    /// The device the application picked, kept across stop and start.
    chosen: Mutex<Option<u32>>,
    /// The device the open session uses, written by the capture thread.
    active: Arc<Mutex<Option<u32>>>,
}

impl NativeCamera {
    fn open(&self) -> Result<(), CameraError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| CameraError::Failed("the camera session lock is poisoned".into()))?;
        if session.is_some() {
            return Ok(());
        }
        // The previous thread holds the device until it returns, so a new one
        // waits for it here rather than racing the platform for the camera.
        if let Ok(mut retiring) = self.retiring.lock() {
            for thread in retiring.drain(..) {
                let _ = thread.join();
            }
        }
        let chosen = self.chosen.lock().ok().and_then(|chosen| *chosen);
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = Arc::clone(&running);
        let active = Arc::clone(&self.active);
        let thread = std::thread::Builder::new()
            .name("cranpose-camera".into())
            .spawn(move || capture_loop(chosen, thread_running, active))
            .map_err(|error| CameraError::Failed(error.to_string()))?;
        *session = Some(Session { running, thread });
        Ok(())
    }

    /// Tells the capture thread to end and returns.
    ///
    /// The thread is left to finish its frame and release the device on its
    /// own: it is blocked inside a platform read for as long as that read
    /// takes, and a stop that waited for it would hold up whoever asked —
    /// which is the screen being left.
    fn close(&self) {
        let taken = self
            .session
            .lock()
            .ok()
            .and_then(|mut session| session.take());
        if let Some(session) = taken {
            session.running.store(false, Ordering::Relaxed);
            if let Ok(mut retiring) = self.retiring.lock() {
                retiring.push(session.thread);
            }
        }
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }
    }
}

impl Camera for NativeCamera {
    fn start(&self) -> Result<(), CameraError> {
        self.open()
    }

    fn stop(&self) {
        self.close();
    }

    fn lenses(&self) -> Vec<CameraLens> {
        list_lenses()
    }

    fn lens(&self) -> Option<String> {
        let active = self.active.lock().ok().and_then(|active| *active);
        active
            .or_else(|| self.chosen.lock().ok().and_then(|chosen| *chosen))
            .map(|index| index.to_string())
    }

    fn use_lens(&self, id: &str) -> bool {
        let Ok(index) = id.parse::<u32>() else {
            return false;
        };
        if !list_lenses().iter().any(|lens| lens.id == id) {
            return false;
        }
        match self.chosen.lock() {
            Ok(mut chosen) => *chosen = Some(index),
            Err(_) => return false,
        }
        let running = self
            .session
            .lock()
            .map(|session| session.is_some())
            .unwrap_or(false);
        if !running {
            publish_camera_lenses(CameraLenses {
                lenses: list_lenses(),
                active: Some(id.to_string()),
            });
            return true;
        }
        // The device changes without a Stopped in between, so the viewfinder
        // keeps the last frame instead of blanking while the new one opens.
        self.close();
        match self.open() {
            Ok(()) => true,
            Err(error) => {
                // The old device is already released, so a screen that heard
                // Running must hear that there is nothing running now.
                publish_camera_state(CameraState::Failed(error));
                false
            }
        }
    }
}

/// Opens the device and pushes what it produces until `running` clears.
///
/// Runs on its own thread because nokhwa blocks for the length of every
/// frame. What happens is published rather than returned: the caller has
/// already moved on, exactly as with the phone backends.
fn capture_loop(chosen: Option<u32>, running: Arc<AtomicBool>, active: Arc<Mutex<Option<u32>>>) {
    let index = match chosen.or_else(|| {
        list_lenses()
            .first()
            .and_then(|lens| lens.id.parse::<u32>().ok())
    }) {
        Some(index) => index,
        None => {
            publish_camera_state(CameraState::Failed(CameraError::Failed(
                "this machine has no camera".into(),
            )));
            return;
        }
    };
    let requested =
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
    let mut device = match CaptureDevice::new(CameraIndex::Index(index), requested) {
        Ok(device) => device,
        Err(error) => {
            publish_camera_state(CameraState::Failed(CameraError::Failed(format!(
                "opening camera {index}: {error}"
            ))));
            return;
        }
    };
    let name = device.info().human_name();
    if let Err(error) = device.open_stream() {
        publish_camera_state(CameraState::Failed(CameraError::Failed(format!(
            "starting the camera stream: {error}"
        ))));
        return;
    }
    if let Ok(mut slot) = active.lock() {
        *slot = Some(index);
    }
    publish_camera_state(CameraState::Running { device: name });
    publish_camera_lenses(CameraLenses {
        lenses: list_lenses(),
        active: Some(index.to_string()),
    });

    let mut sequence = 0u64;
    while running.load(Ordering::Relaxed) {
        match device.frame() {
            Ok(buffer) => match buffer.decode_image::<RgbFormat>() {
                Ok(decoded) => {
                    let (width, height) = (decoded.width(), decoded.height());
                    match CameraFrame::new(
                        width,
                        height,
                        FrameFormat::Rgb8,
                        0,
                        sequence,
                        decoded.into_raw(),
                    ) {
                        Some(frame) => {
                            sequence += 1;
                            publish_camera_frame(frame);
                        }
                        None => record_dropped_camera_frame(),
                    }
                }
                Err(error) => {
                    log::warn!("camera frame decode failed: {error}");
                    record_dropped_camera_frame();
                }
            },
            Err(error) => {
                // The device is briefly out of frames or was yanked; back off
                // rather than spinning a core against the error.
                log::warn!("camera frame read failed: {error}");
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    let _ = device.stop_stream();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{camera_supported, clear_platform_camera};

    /// Installing registers the backend; nothing touches the hardware until a
    /// session is asked for.
    #[test]
    fn installing_the_native_camera_registers_it() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        assert!(!camera_supported());
        install_native_camera();
        assert!(camera_supported());
        clear_platform_camera();
    }

    #[test]
    fn a_lens_id_that_is_not_a_device_index_is_refused() {
        let camera = NativeCamera::default();
        assert!(!camera.use_lens("front"));
        assert!(!camera.use_lens(""));
    }
}
