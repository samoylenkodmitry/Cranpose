//! Apple live camera via `AVCaptureSession`, on iOS and on macOS.
//!
//! Registered as the platform camera (see
//! [`cranpose_services::set_platform_camera`]) by the iOS backend and by the
//! desktop shell on a Mac. A video data output delivers BGRA frames on a
//! background dispatch queue; each frame is converted to tightly-packed RGBA
//! and stored so the app's preview pump can poll it (matching the Android
//! live viewfinder).
//!
//! The two targets share the session, the delegates, the frame conversion and
//! the still capture, because AVFoundation is one stack. They differ in what
//! a device is: a phone carries a fixed set of built-in lenses behind a
//! position, and a Mac carries one built-in camera plus whatever a person
//! plugs in or points at it. The iOS-only calls (focus range, virtual
//! multi-camera switching, torch, flash, high-resolution stills) are absent
//! from the macOS half rather than guarded at runtime: several are declared
//! unavailable on macOS, so messaging them would be a wrong answer at best.
#![allow(unsafe_code)]

use std::{
    sync::{Arc, Mutex, OnceLock, mpsc},
    time::Duration,
};

use block2::RcBlock;
use cranpose_services::{
    Camera, CameraError, CameraFrame, CameraLens, CameraLenses, CameraState, CameraStill,
    FlashMode, FrameFormat, LensFacing, publish_camera_lenses, set_platform_camera,
};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::{
    AllocAnyThread, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, ProtocolObject},
};
// The device-type names are link-time symbols, so each one is imported only on
// the target whose AVFoundation defines it. An iOS-only name referenced from a
// Mac build does not fail to compile, it fails to link.
#[cfg(target_os = "ios")]
use objc2_av_foundation::{
    AVCaptureAutoFocusRangeRestriction, AVCaptureDeviceTypeBuiltInDualWideCamera,
    AVCaptureDeviceTypeBuiltInTelephotoCamera, AVCaptureDeviceTypeBuiltInTripleCamera,
    AVCaptureDeviceTypeBuiltInUltraWideCamera, AVCaptureFlashMode,
    AVCapturePrimaryConstituentDeviceRestrictedSwitchingBehaviorConditions,
    AVCapturePrimaryConstituentDeviceSwitchingBehavior, AVCaptureTorchMode,
};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession, AVCaptureDeviceInput,
    AVCaptureDevicePosition, AVCaptureDeviceType, AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureFocusMode, AVCaptureOutput, AVCapturePhoto, AVCapturePhotoCaptureDelegate,
    AVCapturePhotoOutput, AVCapturePhotoSettings, AVCaptureSession, AVCaptureSessionPresetPhoto,
    AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaType,
    AVMediaTypeVideo, AVVideoCodecKey, AVVideoCodecTypeJPEG,
};
#[cfg(target_os = "macos")]
use objc2_av_foundation::{
    AVCaptureDeviceTypeContinuityCamera, AVCaptureDeviceTypeDeskViewCamera,
    AVCaptureDeviceTypeExternal,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVBuffer, CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSString,
};

/// `'BGRA'` — 32-bit BGRA, the pixel format we request from the video output.
const PIXEL_FORMAT_32BGRA: u32 = 0x4247_5241;

/// Which frame this is in the session, so a consumer can tell a repeat from a
/// new one and count what it missed.
static FRAME_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Recycled RGBA buffers for [`frame_from_sample`]. The delegate publishes a
/// frame ~25×/s; a fresh multi-MB `Vec` per frame fragments the app
/// heap against any concurrently running inference's transient buffers
/// (measured on a phone: each SAM encode grew the process footprint ~500MB
/// while frames interleaved, straight into a jetsam kill — with frame
/// delivery paused the identical workload stayed flat). Replaced frames
/// park their allocation here; steady state allocates nothing.
fn buffer_pool() -> &'static Mutex<Vec<Vec<u8>>> {
    static POOL: OnceLock<Mutex<Vec<Vec<u8>>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Vec::new()))
}

/// Holds the running session alive. Its AVFoundation objects are only touched
/// from `start`/`stop` (the app's single preview-pump thread) and the frame
/// delegate (its own dispatch queue, which only writes [`latest`]); marking it
/// `Send` lets it live behind the session mutex.
struct SessionHolder {
    session: Retained<AVCaptureSession>,
    photo_output: Retained<AVCapturePhotoOutput>,
    /// The capture device, kept for mid-session reconfiguration (torch).
    device: Retained<AVCaptureDevice>,
    _delegate: Retained<FrameDelegate>,
    _queue: DispatchRetained<DispatchQueue>,
}
unsafe impl Send for SessionHolder {}

fn session_slot() -> &'static Mutex<Option<SessionHolder>> {
    static SLOT: OnceLock<Mutex<Option<SessionHolder>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// What the flash does on the next still, kept across a lens switch.
fn flash_slot() -> &'static Mutex<FlashMode> {
    static SLOT: OnceLock<Mutex<FlashMode>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(FlashMode::Off))
}

/// The lens the app picked, or `None` while the default device is in use.
fn lens_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Installs the AVFoundation camera as the platform camera.
pub(crate) fn register() {
    set_platform_camera(Arc::new(AppleCamera));
}

struct AppleCamera;

impl Camera for AppleCamera {
    fn start(&self) -> Result<(), CameraError> {
        if session_slot().lock().map(|s| s.is_some()).unwrap_or(false) {
            cranpose_services::publish_camera_state(CameraState::Running {
                device: "camera".to_string(),
            });
            return Ok(());
        }
        let device = start_session()?;
        cranpose_services::publish_camera_state(CameraState::Running { device });
        publish_lenses(self.lens());
        Ok(())
    }

    /// Asks the photo output for a still.
    ///
    /// The exposure and encode take as long as they take, so the request runs
    /// on a thread of its own and the picture arrives through the framework's
    /// still stream rather than by blocking whoever asked.
    fn request_still(&self) -> Result<(), CameraError> {
        if session_slot().lock().map(|s| s.is_none()).unwrap_or(true) {
            return Err(CameraError::NotRunning);
        }
        std::thread::Builder::new()
            .name("cranpose-camera-still".to_string())
            .spawn(|| {
                cranpose_services::publish_camera_still(capture_photo().ok_or_else(|| {
                    CameraError::Failed("the still capture produced no image".to_string())
                }));
            })
            .map_err(|error| CameraError::Failed(error.to_string()))?;
        Ok(())
    }

    #[cfg(target_os = "ios")]
    fn set_torch(&self, on: bool) -> bool {
        let Ok(slot) = session_slot().lock() else {
            return false;
        };
        let Some(holder) = slot.as_ref() else {
            return false;
        };
        let device = &holder.device;
        let mode = if on {
            AVCaptureTorchMode::On
        } else {
            AVCaptureTorchMode::Off
        };
        if !unsafe { device.hasTorch() } || !unsafe { device.isTorchModeSupported(mode) } {
            return false;
        }
        if unsafe { device.lockForConfiguration() }.is_err() {
            return false;
        }
        unsafe { device.setTorchMode(mode) };
        unsafe { device.unlockForConfiguration() };
        true
    }

    /// `torchMode` is declared unavailable on macOS, and no Mac camera carries
    /// a lamp, so the answer is no rather than a message the runtime may not
    /// answer.
    #[cfg(target_os = "macos")]
    fn set_torch(&self, _on: bool) -> bool {
        false
    }

    fn lenses(&self) -> Vec<CameraLens> {
        all_lenses()
    }

    fn lens(&self) -> Option<String> {
        if let Ok(slot) = session_slot().lock()
            && let Some(holder) = slot.as_ref()
        {
            return Some(unsafe { holder.device.uniqueID() }.to_string());
        }
        lens_slot().lock().ok().and_then(|slot| slot.clone())
    }

    fn use_lens(&self, id: &str) -> bool {
        if !all_lenses().iter().any(|lens| lens.id == id) {
            return false;
        }
        let running = session_slot().lock().map(|s| s.is_some()).unwrap_or(false);
        match lens_slot().lock() {
            Ok(mut slot) => *slot = Some(id.to_string()),
            Err(_) => return false,
        }
        if !running {
            publish_lenses(Some(id.to_string()));
            return true;
        }
        // The device changes without a Stopped in between, so the viewfinder
        // keeps the last frame instead of blanking while the new one opens.
        self.stop();
        match start_session() {
            Ok(device) => {
                cranpose_services::publish_camera_state(CameraState::Running { device });
                publish_lenses(Some(id.to_string()));
                true
            }
            Err(error) => {
                // The old device is already closed, so a screen that heard
                // Running must hear that there is nothing running now.
                cranpose_services::publish_camera_state(CameraState::Failed(error));
                false
            }
        }
    }

    #[cfg(target_os = "ios")]
    fn has_flash(&self) -> bool {
        if let Ok(slot) = session_slot().lock()
            && let Some(holder) = slot.as_ref()
        {
            return unsafe { holder.device.hasFlash() };
        }
        back_lenses()
            .first()
            .map(|device| unsafe { device.hasFlash() })
            .unwrap_or(false)
    }

    /// Same reading as the torch: `hasFlash` is declared unavailable on macOS.
    #[cfg(target_os = "macos")]
    fn has_flash(&self) -> bool {
        false
    }

    fn set_flash(&self, mode: FlashMode) -> bool {
        match flash_slot().lock() {
            Ok(mut slot) => *slot = mode,
            Err(_) => return false,
        }
        self.has_flash()
    }

    fn stop(&self) {
        if let Ok(mut slot) = session_slot().lock()
            && let Some(holder) = slot.take()
        {
            unsafe { holder.session.stopRunning() };
        }
        FRAME_SEQUENCE.store(0, std::sync::atomic::Ordering::Release);
    }
}

/// The back cameras a phone carries, widest field of view first.
///
/// The virtual triple/dual devices are left out: they are what
/// [`select_camera_device`] opens when the app picks no lens, and listing them
/// beside their own constituents would offer the same picture twice.
#[cfg(target_os = "ios")]
fn back_lenses() -> Vec<Retained<AVCaptureDevice>> {
    let types: [&AVCaptureDeviceType; 3] = unsafe {
        [
            AVCaptureDeviceTypeBuiltInUltraWideCamera,
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeBuiltInTelephotoCamera,
        ]
    };
    let mut found = discover_devices(&types, AVCaptureDevicePosition::Back);
    found.sort_by_key(|device| lens_order(device));
    found
}

/// The front camera, when the device has one.
#[cfg(target_os = "ios")]
fn front_lenses() -> Vec<Retained<AVCaptureDevice>> {
    let types: [&AVCaptureDeviceType; 1] = unsafe { [AVCaptureDeviceTypeBuiltInWideAngleCamera] };
    discover_devices(&types, AVCaptureDevicePosition::Front)
}

/// Every camera a Mac can open: the built-in one, an iPhone acting as a
/// continuity camera, the desk view that camera also offers, and anything
/// plugged in.
///
/// The position is left unspecified because a Mac answers it inconsistently.
/// The built-in camera reports front, a continuity camera reports the position
/// of the lens the phone is using, and a USB camera reports nothing at all, so
/// asking a discovery session for one position would drop whole devices.
#[cfg(target_os = "macos")]
fn mac_devices() -> Vec<Retained<AVCaptureDevice>> {
    let types: [&AVCaptureDeviceType; 4] = unsafe {
        [
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeExternal,
            AVCaptureDeviceTypeContinuityCamera,
            AVCaptureDeviceTypeDeskViewCamera,
        ]
    };
    discover_devices(&types, AVCaptureDevicePosition::Unspecified)
}

fn discover_devices(
    types: &[&AVCaptureDeviceType],
    position: AVCaptureDevicePosition,
) -> Vec<Retained<AVCaptureDevice>> {
    let Some(media_type) = (unsafe { AVMediaTypeVideo }) else {
        return Vec::new();
    };
    let wanted = NSArray::from_slice(types);
    let session = unsafe {
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &wanted,
            Some(media_type),
            position,
        )
    };
    unsafe { session.devices() }.to_vec()
}

/// Every device the application may pick, back lenses first, widest first,
/// then the front one.
#[cfg(target_os = "ios")]
fn all_lenses() -> Vec<CameraLens> {
    let mut lenses: Vec<CameraLens> = back_lenses()
        .into_iter()
        .map(|device| CameraLens {
            id: unsafe { device.uniqueID() }.to_string(),
            name: lens_name(&device),
            facing: LensFacing::Back,
        })
        .collect();
    for (index, device) in front_lenses().into_iter().enumerate() {
        lenses.push(CameraLens {
            id: unsafe { device.uniqueID() }.to_string(),
            name: if index == 0 {
                "Front".to_string()
            } else {
                format!("Front {}", index + 1)
            },
            facing: LensFacing::Front,
        });
    }
    lenses
}

/// Every camera the application may pick, in the order the discovery session
/// gives them: the built-in one first, then anything attached.
///
/// A Mac names its cameras itself, and the names are better than anything this
/// code could infer — "FaceTime HD Camera", the phone's own name for a
/// continuity camera, the model name of a USB one.
#[cfg(target_os = "macos")]
fn all_lenses() -> Vec<CameraLens> {
    mac_devices()
        .into_iter()
        .map(|device| CameraLens {
            id: unsafe { device.uniqueID() }.to_string(),
            name: unsafe { device.localizedName() }.to_string(),
            facing: match unsafe { device.position() } {
                AVCaptureDevicePosition::Front => LensFacing::Front,
                AVCaptureDevicePosition::Back => LensFacing::Back,
                _ => LensFacing::External,
            },
        })
        .collect()
}

/// Publishes the lens list and the device in use, so a lens control observes
/// instead of paying a discovery session per recomposition.
fn publish_lenses(active: Option<String>) {
    publish_camera_lenses(CameraLenses {
        lenses: all_lenses(),
        active,
    });
}

#[cfg(target_os = "ios")]
fn lens_order(device: &AVCaptureDevice) -> u8 {
    let kind = unsafe { device.deviceType() };
    let ultra = unsafe { AVCaptureDeviceTypeBuiltInUltraWideCamera };
    let wide = unsafe { AVCaptureDeviceTypeBuiltInWideAngleCamera };
    if &*kind == ultra {
        0
    } else if &*kind == wide {
        1
    } else {
        2
    }
}

#[cfg(target_os = "ios")]
fn lens_name(device: &AVCaptureDevice) -> String {
    match lens_order(device) {
        0 => "Ultra wide".into(),
        1 => "Wide".into(),
        _ => "Tele".into(),
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposeCameraDelegate"]
    #[ivars = ()]
    struct FrameDelegate;

    unsafe impl NSObjectProtocol for FrameDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for FrameDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        unsafe fn did_output(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            if let Some(frame) = frame_from_sample(sample_buffer) {
                // The framework keeps the newest frame and hands it to whoever
                // is observing; the parked buffer here is only the recycling.
                if let Some(old) = cranpose_services::latest_camera_frame()
                    && let Ok(mut pool) = buffer_pool().lock()
                    && pool.len() < 3
                {
                    pool.push(old.bytes);
                }
                cranpose_services::publish_camera_frame(frame);
            }
        }
    }
);

impl FrameDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Resolves one pending still capture: the encoded JPEG, or `None` on failure.
type PhotoSender = mpsc::Sender<Option<Vec<u8>>>;

/// The pending still capture's result channel. One capture runs at a time
/// (guarded by [`capture_photo`]'s lock); the photo delegate resolves it.
fn photo_result_slot() -> &'static Mutex<Option<PhotoSender>> {
    static SLOT: OnceLock<Mutex<Option<PhotoSender>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposePhotoDelegate"]
    #[ivars = ()]
    struct PhotoDelegate;

    unsafe impl NSObjectProtocol for PhotoDelegate {}

    unsafe impl AVCapturePhotoCaptureDelegate for PhotoDelegate {
        #[unsafe(method(captureOutput:didFinishProcessingPhoto:error:))]
        unsafe fn did_finish_photo(
            &self,
            _output: &AVCapturePhotoOutput,
            photo: &AVCapturePhoto,
            error: Option<&NSError>,
        ) {
            let jpeg = if error.is_some() {
                None
            } else {
                unsafe { photo.fileDataRepresentation() }.map(|data| data.to_vec())
            };
            if let Ok(mut slot) = photo_result_slot().lock()
                && let Some(sender) = slot.take()
            {
                let _ = sender.send(jpeg);
            }
        }
    }
);

impl PhotoDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Capture one full-resolution JPEG still through `AVCapturePhotoOutput`.
///
/// Blocks the calling (worker) thread until the photo pipeline delivers the
/// encoded image or a timeout passes. The EXIF orientation in the JPEG carries
/// the sensor rotation, matching the assumption the portrait viewfinder makes.
fn capture_photo() -> Option<CameraStill> {
    // Serialize captures: the delegate resolves the single pending channel.
    static CAPTURE_GATE: Mutex<()> = Mutex::new(());
    let _gate = CAPTURE_GATE.lock().ok()?;

    let (sender, receiver) = mpsc::channel::<Option<Vec<u8>>>();
    // The delegate must outlive the async capture; hold it until the callback
    // (or timeout) resolves the channel below.
    let delegate = PhotoDelegate::new();
    {
        let slot = session_slot().lock().ok()?;
        let holder = slot.as_ref()?;
        if let Ok(mut result) = photo_result_slot().lock() {
            *result = Some(sender);
        }
        let codec_key: &NSString = unsafe { AVVideoCodecKey }?;
        let codec_value: &NSString = unsafe { AVVideoCodecTypeJPEG }?;
        let codec: &AnyObject = codec_value.as_ref();
        let format = NSDictionary::from_slices(&[codec_key], &[codec]);
        let settings = unsafe { AVCapturePhotoSettings::photoSettingsWithFormat(Some(&format)) };
        // The high-resolution switch and the flash are a phone's. macOS
        // declares both unavailable, and a Mac camera has no lamp to fire.
        #[cfg(target_os = "ios")]
        {
            // `maxPhotoDimensions` (iOS 16+) supersedes these, but the
            // deprecated switches still map onto it and keep the iOS 15 floor
            // working.
            #[allow(deprecated)]
            unsafe {
                settings.setHighResolutionPhotoEnabled(true);
            }
            let wanted = match *flash_slot().lock().ok()? {
                FlashMode::Off => AVCaptureFlashMode::Off,
                FlashMode::Auto => AVCaptureFlashMode::Auto,
                FlashMode::On => AVCaptureFlashMode::On,
            };
            let supported = unsafe { holder.photo_output.supportedFlashModes() }
                .iter()
                .any(|mode| mode.as_i64() == wanted.0 as i64);
            if supported {
                unsafe { settings.setFlashMode(wanted) };
            }
        }
        unsafe {
            holder
                .photo_output
                .capturePhotoWithSettings_delegate(&settings, ProtocolObject::from_ref(&*delegate));
        }
    }

    let jpeg = receiver.recv_timeout(Duration::from_secs(5)).ok().flatten();
    if jpeg.is_none() {
        // Timeout or failure: clear any unresolved channel so a later capture
        // starts clean.
        if let Ok(mut result) = photo_result_slot().lock() {
            *result = None;
        }
    }
    drop(delegate);
    jpeg.map(|jpeg| CameraStill { jpeg })
}

/// Convert one BGRA sample buffer to a tightly-packed RGBA frame.
fn frame_from_sample(sample: &CMSampleBuffer) -> Option<CameraFrame> {
    let image_buffer = unsafe { sample.image_buffer() }?;
    // CVImageBuffer and CVPixelBuffer are the same Core Video object.
    let pixel_buffer: &CVPixelBuffer =
        unsafe { &*((&*image_buffer as *const CVBuffer) as *const CVPixelBuffer) };

    let flags = CVPixelBufferLockFlags::ReadOnly;
    unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, flags) };

    let width = CVPixelBufferGetWidth(pixel_buffer);
    let height = CVPixelBufferGetHeight(pixel_buffer);
    let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
    let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;

    // Recycle a parked buffer when one fits (clear keeps capacity, so after
    // the first few frames this allocates nothing at all).
    let mut rgba = buffer_pool()
        .lock()
        .ok()
        .and_then(|mut pool| pool.pop())
        .unwrap_or_default();
    rgba.clear();
    rgba.resize(width * height * 4, 0);
    if !base.is_null() && bytes_per_row >= width * 4 {
        for y in 0..height {
            let src_row = unsafe { base.add(y * bytes_per_row) };
            let out_row = &mut rgba[y * width * 4..(y + 1) * width * 4];
            for x in 0..width {
                let src = unsafe { src_row.add(x * 4) };
                let (b, g, r, a) = unsafe { (*src, *src.add(1), *src.add(2), *src.add(3)) };
                let dst = x * 4;
                out_row[dst] = r;
                out_row[dst + 1] = g;
                out_row[dst + 2] = b;
                out_row[dst + 3] = a;
            }
        }
    }

    unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, flags) };

    // On a phone the sensor delivers landscape buffers while the app runs in
    // portrait, so the frame carries a 90° clockwise turn. The turn is
    // metadata rather than a pixel pass here: `CameraFrame::upright_rgba8`
    // fuses it with whatever conversion the consumer does anyway, the same as
    // on Android. A Mac camera is already the way up its window is, so it
    // carries no turn.
    #[cfg(target_os = "ios")]
    let rotation = 90;
    #[cfg(target_os = "macos")]
    let rotation = 0;
    CameraFrame::new(
        width as u32,
        height as u32,
        FrameFormat::Rgba8,
        rotation,
        FRAME_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::AcqRel),
        rgba,
    )
}

/// Picks the device to open: the one the application asked for, or the
/// platform's default.
///
/// On iOS the default prefers an auto-switching virtual multi-camera (triple,
/// then dual-wide) so the system can drop to the ultra-wide constituent for
/// macro (close-up receipts, below the wide lens's minimum focus distance),
/// and falls back to the plain wide-angle camera on devices without a virtual
/// one. A Mac has no virtual device, so the default is whatever AVFoundation
/// names first, which is the built-in camera when there is one.
fn select_camera_device(media_type: &AVMediaType) -> Option<Retained<AVCaptureDevice>> {
    if let Some(id) = lens_slot().lock().ok().and_then(|slot| slot.clone()) {
        let wanted = NSString::from_str(&id);
        if let Some(device) = unsafe { AVCaptureDevice::deviceWithUniqueID(&wanted) } {
            return Some(device);
        }
    }
    #[cfg(target_os = "ios")]
    {
        let virtual_types: [&AVCaptureDeviceType; 2] = unsafe {
            [
                AVCaptureDeviceTypeBuiltInTripleCamera,
                AVCaptureDeviceTypeBuiltInDualWideCamera,
            ]
        };
        for device_type in virtual_types {
            if let Some(device) = unsafe {
                AVCaptureDevice::defaultDeviceWithDeviceType_mediaType_position(
                    device_type,
                    Some(media_type),
                    AVCaptureDevicePosition::Back,
                )
            } {
                return Some(device);
            }
        }
    }
    unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media_type) }
}

fn start_session() -> Result<String, CameraError> {
    let media_type = unsafe { AVMediaTypeVideo }
        .ok_or_else(|| CameraError::Failed("AVMediaTypeVideo unavailable".into()))?;

    // Trigger the permission prompt (first launch); frames flow once granted.
    let handler = RcBlock::new(|_granted: Bool| {});
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler) };

    let device = select_camera_device(media_type).ok_or(CameraError::Unsupported)?;
    let name = unsafe { device.localizedName() }.to_string();

    // Enable continuous autofocus so the viewfinder keeps documents sharp as the
    // user moves the phone (the default is a fixed lens position -> blurry
    // preview). Restrict the scan range to "near" when supported, since receipts
    // and pages are held close. On a virtual multi-camera device, allow the
    // system to auto-switch to the ultra-wide constituent so very close subjects
    // (macro, below the wide lens's minimum focus distance) stay sharp. Any of
    // these calls throw if unsupported, so they are all guarded.
    if unsafe { device.lockForConfiguration() }.is_ok() {
        // The focus-range restriction and the constituent-device switching are
        // both a phone's, and macOS declares neither.
        #[cfg(target_os = "ios")]
        {
            if unsafe { device.isAutoFocusRangeRestrictionSupported() } {
                unsafe {
                    device.setAutoFocusRangeRestriction(AVCaptureAutoFocusRangeRestriction::Near)
                };
            }
            if unsafe { device.primaryConstituentDeviceSwitchingBehavior() }
                != AVCapturePrimaryConstituentDeviceSwitchingBehavior::Unsupported
            {
                unsafe {
                    device.setPrimaryConstituentDeviceSwitchingBehavior_restrictedSwitchingBehaviorConditions(
                        AVCapturePrimaryConstituentDeviceSwitchingBehavior::Auto,
                        AVCapturePrimaryConstituentDeviceRestrictedSwitchingBehaviorConditions(0),
                    )
                };
            }
        }
        if unsafe { device.isFocusModeSupported(AVCaptureFocusMode::ContinuousAutoFocus) } {
            unsafe { device.setFocusMode(AVCaptureFocusMode::ContinuousAutoFocus) };
        }
        unsafe { device.unlockForConfiguration() };
    }

    let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }
        .map_err(|_| CameraError::Failed("could not open camera input".into()))?;

    let session = unsafe { AVCaptureSession::new() };
    unsafe { session.beginConfiguration() };
    // The photo preset unlocks full-sensor stills through the photo output;
    // the video data output then streams preview-resolution 4:3 frames
    // (device-dependent, ~1440x1080) instead of 720p.
    let preset = unsafe { AVCaptureSessionPresetPhoto };
    if unsafe { session.canSetSessionPreset(preset) } {
        unsafe { session.setSessionPreset(preset) };
    }
    if !unsafe { session.canAddInput(&input) } {
        return Err(CameraError::Failed("cannot add camera input".into()));
    }
    unsafe { session.addInput(&input) };

    let output = unsafe { AVCaptureVideoDataOutput::new() };
    let key: &NSString =
        unsafe { &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString) };
    let value = NSNumber::numberWithUnsignedInt(PIXEL_FORMAT_32BGRA);
    let value_obj: &AnyObject = &value;
    let settings = NSDictionary::from_slices(&[key], &[value_obj]);
    unsafe { output.setVideoSettings(Some(&settings)) };
    unsafe { output.setAlwaysDiscardsLateVideoFrames(true) };

    let delegate = FrameDelegate::new();
    let queue = DispatchQueue::new("com.cranpose.camera", None);
    unsafe {
        output
            .setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*delegate)), Some(&queue))
    };
    if !unsafe { session.canAddOutput(&output) } {
        return Err(CameraError::Failed("cannot add camera output".into()));
    }
    unsafe { session.addOutput(&output) };

    // Dedicated photo pipeline for full-resolution stills (see
    // [`Camera::capture_still`]). High-resolution capture must be opted into
    // before the session starts.
    let photo_output = unsafe { AVCapturePhotoOutput::new() };
    if !unsafe { session.canAddOutput(&photo_output) } {
        return Err(CameraError::Failed("cannot add photo output".into()));
    }
    unsafe { session.addOutput(&photo_output) };
    // `maxPhotoDimensions` (iOS 16+) supersedes this, but the deprecated
    // switch still maps onto it and keeps the iOS 15 floor working. macOS
    // declares it unavailable and gives full sensor size without asking.
    #[cfg(target_os = "ios")]
    #[allow(deprecated)]
    unsafe {
        photo_output.setHighResolutionCaptureEnabled(true);
    }

    unsafe { session.commitConfiguration() };
    unsafe { session.startRunning() };

    if let Ok(mut slot) = session_slot().lock() {
        *slot = Some(SessionHolder {
            session,
            photo_output,
            device,
            _delegate: delegate,
            _queue: queue,
        });
    }
    Ok(name)
}
