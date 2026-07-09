//! Live camera capture: an in-app viewfinder frame source.
//!
//! Unlike the one-shot [`image_picker`](crate::image_picker) (which presents the
//! system camera UI), this exposes a running capture session whose latest frame
//! the app polls to render its own viewfinder and run per-frame detection —
//! matching the Android/desktop live-preview experience.
//!
//! The platform backend installs an implementation via [`set_platform_camera`]
//! (iOS `AVCaptureSession`, desktop `nokhwa`, …). No default: [`camera`] returns
//! `None` where live capture is unsupported, so the app can fall back to the
//! image picker.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

/// A single captured frame as tightly-packed RGBA8 (`width * height * 4` bytes,
/// row-major, no padding).
#[derive(Clone)]
pub struct CameraFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    /// No live camera backend on this platform.
    #[error("live camera capture is not supported here")]
    Unsupported,
    /// The user denied camera access.
    #[error("camera permission denied")]
    PermissionDenied,
    /// Any other failure (no device, configuration error, …).
    #[error("{0}")]
    Failed(String),
}

/// A running (or startable) live camera. Implementations are `Send + Sync` so a
/// background preview pump can start/stop and poll frames off the UI thread.
pub trait Camera: Send + Sync {
    /// Start the capture session, returning a human-readable device name. Safe
    /// to call again while already running (idempotent).
    fn start(&self) -> Result<String, CameraError>;
    /// The most recent frame, or `None` if none has arrived yet.
    fn latest_frame(&self) -> Option<CameraFrame>;
    /// Stop the session and release the device.
    fn stop(&self);
}

pub type CameraRef = Arc<dyn Camera>;

fn slot() -> &'static Mutex<Option<CameraRef>> {
    static SLOT: OnceLock<Mutex<Option<CameraRef>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Installs the platform live camera, replacing any previous one.
pub fn set_platform_camera(camera: CameraRef) {
    if let Ok(mut s) = slot().lock() {
        *s = Some(camera);
    }
}

/// Removes any registered platform camera (tests/teardown).
pub fn clear_platform_camera() {
    if let Ok(mut s) = slot().lock() {
        *s = None;
    }
}

/// The registered live camera, or `None` where live capture is unsupported.
pub fn camera() -> Option<CameraRef> {
    slot().lock().ok().and_then(|s| s.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_round_trips() {
        clear_platform_camera();
        assert!(camera().is_none());
        struct Fake;
        impl Camera for Fake {
            fn start(&self) -> Result<String, CameraError> {
                Ok("fake".into())
            }
            fn latest_frame(&self) -> Option<CameraFrame> {
                Some(CameraFrame {
                    width: 1,
                    height: 1,
                    rgba: vec![0, 0, 0, 255],
                })
            }
            fn stop(&self) {}
        }
        set_platform_camera(Arc::new(Fake));
        let cam = camera().expect("registered");
        assert_eq!(cam.start().unwrap(), "fake");
        assert_eq!(cam.latest_frame().unwrap().width, 1);
        clear_platform_camera();
    }
}
