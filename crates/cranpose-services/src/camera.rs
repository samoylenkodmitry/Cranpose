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

use crate::registry::ServiceRegistry;
use std::sync::Arc;

/// A single captured frame as tightly-packed RGBA8 (`width * height * 4` bytes,
/// row-major, no padding).
#[derive(Clone)]
pub struct CameraFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A full-resolution still photograph as an encoded image.
///
/// Unlike [`CameraFrame`] (a viewfinder-resolution stream frame), a still is
/// captured through the platform's dedicated photo pipeline at full sensor
/// resolution — on iOS that is `AVCapturePhotoOutput`, roughly 12 MP versus the
/// 720p-class viewfinder. The bytes are an encoded JPEG whose EXIF orientation
/// tag reflects the device rotation; decode with an orientation-aware decoder.
#[derive(Clone)]
pub struct CameraStill {
    pub jpeg: Vec<u8>,
}

/// One capture device the app may pick, as reported by [`Camera::lenses`].
///
/// `id` is the platform's own handle for the device (an `AVCaptureDevice`
/// uniqueID on iOS, a camera2 id on Android); pass it back to
/// [`Camera::use_lens`]. `name` is for a button label: "Ultra wide", "Wide",
/// "Tele".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraLens {
    pub id: String,
    pub name: String,
}

/// What the light does when a still is captured.
///
/// `Auto` leaves the choice to the device's exposure metering. A backend with
/// no flash reports `false` from [`Camera::set_flash`] and the app hides the
/// control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlashMode {
    #[default]
    Off,
    Auto,
    On,
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
    /// Capture a full-resolution still through the platform photo pipeline.
    ///
    /// Blocks up to a few seconds while the device exposes and encodes.
    /// Returns `None` where the backend has no dedicated photo path (callers
    /// should fall back to [`latest_frame`](Self::latest_frame)) or when the
    /// capture fails.
    fn capture_still(&self) -> Option<CameraStill> {
        None
    }
    /// Toggle the capture device's torch (flashlight) while the session runs.
    /// Scanner-style apps light dim scenes instead of trying to analyze
    /// photon-starved frames. Returns `false` where the device has no torch
    /// or the backend has none wired; the torch dies with the session.
    fn set_torch(&self, _on: bool) -> bool {
        false
    }
    /// The capture devices the app may pick between, back cameras first and in
    /// field-of-view order (widest first). An empty list means the app shows no
    /// lens control: either the platform has one camera or the backend does not
    /// list them.
    fn lenses(&self) -> Vec<CameraLens> {
        Vec::new()
    }

    /// The id of the device the session uses, or `None` when nothing is open
    /// and the backend has no stored choice.
    fn lens(&self) -> Option<String> {
        None
    }

    /// Open `id` instead of the current device, keeping the session running.
    /// Returns `false` when the id is unknown or the backend cannot switch.
    fn use_lens(&self, _id: &str) -> bool {
        false
    }

    /// Whether the current device has a flash for stills.
    fn has_flash(&self) -> bool {
        false
    }

    /// What the flash does on the next [`capture_still`](Self::capture_still).
    /// Returns `false` where the device has no flash or the backend has none
    /// wired; the mode dies with the session.
    fn set_flash(&self, _mode: FlashMode) -> bool {
        false
    }

    /// Stop the session and release the device.
    fn stop(&self);
}

pub type CameraRef = Arc<dyn Camera>;

static PLATFORM_CAMERA: ServiceRegistry<dyn Camera> = ServiceRegistry::new();

/// Installs the platform live camera, replacing any previous one.
pub fn set_platform_camera(camera: CameraRef) {
    PLATFORM_CAMERA.set(camera);
}

/// Removes any registered platform camera (tests/teardown).
pub fn clear_platform_camera() {
    PLATFORM_CAMERA.clear();
}

/// The registered live camera, or `None` where live capture is unsupported.
pub fn camera() -> Option<CameraRef> {
    PLATFORM_CAMERA.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_round_trips() {
        let _guard = crate::registry::test_service_guard();
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

    #[test]
    fn a_backend_that_lists_no_lens_and_no_flash_says_so() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        struct Bare;
        impl Camera for Bare {
            fn start(&self) -> Result<String, CameraError> {
                Ok("bare".into())
            }
            fn latest_frame(&self) -> Option<CameraFrame> {
                None
            }
            fn stop(&self) {}
        }
        set_platform_camera(Arc::new(Bare));
        let cam = camera().expect("registered");
        assert!(cam.lenses().is_empty());
        assert_eq!(cam.lens(), None);
        assert!(!cam.use_lens("0"));
        assert!(!cam.has_flash());
        assert!(!cam.set_flash(FlashMode::On));
        clear_platform_camera();
    }

    #[test]
    fn a_backend_that_lists_two_lenses_hands_them_over_in_order() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        struct Two;
        impl Camera for Two {
            fn start(&self) -> Result<String, CameraError> {
                Ok("two".into())
            }
            fn latest_frame(&self) -> Option<CameraFrame> {
                None
            }
            fn lenses(&self) -> Vec<CameraLens> {
                vec![
                    CameraLens {
                        id: "u".into(),
                        name: "Ultra wide".into(),
                    },
                    CameraLens {
                        id: "w".into(),
                        name: "Wide".into(),
                    },
                ]
            }
            fn lens(&self) -> Option<String> {
                Some("w".into())
            }
            fn use_lens(&self, id: &str) -> bool {
                id == "u" || id == "w"
            }
            fn stop(&self) {}
        }
        set_platform_camera(Arc::new(Two));
        let cam = camera().expect("registered");
        let lenses = cam.lenses();
        assert_eq!(lenses.len(), 2);
        assert_eq!(lenses[0].name, "Ultra wide");
        assert_eq!(cam.lens().as_deref(), Some("w"));
        assert!(cam.use_lens("u"));
        assert!(!cam.use_lens("tele"));
        clear_platform_camera();
    }
}
