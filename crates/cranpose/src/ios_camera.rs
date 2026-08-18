//! iOS live camera via `AVCaptureSession`.
//!
//! Registered as the platform camera (see
//! [`cranpose_services::set_platform_camera`]) by the iOS backend. A video data
//! output delivers BGRA frames on a background dispatch queue; each frame is
//! converted to tightly-packed RGBA and stored so the app's preview pump can
//! poll it (matching the desktop/Android live viewfinder).
#![allow(unsafe_code)]

use block2::RcBlock;
use cranpose_services::{
    set_platform_camera, Camera, CameraError, CameraFrame, CameraLens, CameraStill, FlashMode,
};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread};
use objc2_av_foundation::{
    AVCaptureAutoFocusRangeRestriction, AVCaptureConnection, AVCaptureDevice,
    AVCaptureDeviceDiscoverySession, AVCaptureDeviceInput, AVCaptureDevicePosition,
    AVCaptureDeviceType, AVCaptureDeviceTypeBuiltInDualWideCamera,
    AVCaptureDeviceTypeBuiltInTelephotoCamera, AVCaptureDeviceTypeBuiltInTripleCamera,
    AVCaptureDeviceTypeBuiltInUltraWideCamera, AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureFlashMode, AVCaptureFocusMode, AVCaptureOutput, AVCapturePhoto,
    AVCapturePhotoCaptureDelegate, AVCapturePhotoOutput, AVCapturePhotoSettings,
    AVCapturePrimaryConstituentDeviceRestrictedSwitchingBehaviorConditions,
    AVCapturePrimaryConstituentDeviceSwitchingBehavior, AVCaptureSession,
    AVCaptureSessionPresetPhoto, AVCaptureTorchMode, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaType, AVMediaTypeVideo, AVVideoCodecKey,
    AVVideoCodecTypeJPEG,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    kCVPixelBufferPixelFormatTypeKey, CVBuffer, CVPixelBuffer, CVPixelBufferGetBaseAddress,
    CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSString,
};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

/// `'BGRA'` — 32-bit BGRA, the pixel format we request from the video output.
const PIXEL_FORMAT_32BGRA: u32 = 0x4247_5241;

fn latest() -> &'static Mutex<Option<CameraFrame>> {
    static SLOT: OnceLock<Mutex<Option<CameraFrame>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Recycled RGBA buffers for [`frame_from_sample`]. The delegate replaces
/// [`latest`] ~25×/s; a fresh multi-MB `Vec` per frame fragments the app
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

/// Installs the iOS camera as the platform camera.
pub(crate) fn register() {
    set_platform_camera(Arc::new(IosCamera));
}

struct IosCamera;

impl Camera for IosCamera {
    fn start(&self) -> Result<String, CameraError> {
        if session_slot().lock().map(|s| s.is_some()).unwrap_or(false) {
            return Ok("camera".into());
        }
        start_session()
    }

    fn latest_frame(&self) -> Option<CameraFrame> {
        latest().lock().ok().and_then(|f| f.clone())
    }

    fn capture_still(&self) -> Option<CameraStill> {
        capture_photo()
    }

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

    fn lenses(&self) -> Vec<CameraLens> {
        back_lenses()
            .into_iter()
            .map(|device| CameraLens {
                id: unsafe { device.uniqueID() }.to_string(),
                name: lens_name(&device),
            })
            .collect()
    }

    fn lens(&self) -> Option<String> {
        if let Ok(slot) = session_slot().lock() {
            if let Some(holder) = slot.as_ref() {
                return Some(unsafe { holder.device.uniqueID() }.to_string());
            }
        }
        lens_slot().lock().ok().and_then(|slot| slot.clone())
    }

    fn use_lens(&self, id: &str) -> bool {
        if !back_lenses()
            .iter()
            .any(|device| unsafe { device.uniqueID() }.to_string() == id)
        {
            return false;
        }
        let running = session_slot().lock().map(|s| s.is_some()).unwrap_or(false);
        match lens_slot().lock() {
            Ok(mut slot) => *slot = Some(id.to_string()),
            Err(_) => return false,
        }
        if !running {
            return true;
        }
        self.stop();
        start_session().is_ok()
    }

    fn has_flash(&self) -> bool {
        if let Ok(slot) = session_slot().lock() {
            if let Some(holder) = slot.as_ref() {
                return unsafe { holder.device.hasFlash() };
            }
        }
        back_lenses()
            .first()
            .map(|device| unsafe { device.hasFlash() })
            .unwrap_or(false)
    }

    fn set_flash(&self, mode: FlashMode) -> bool {
        match flash_slot().lock() {
            Ok(mut slot) => *slot = mode,
            Err(_) => return false,
        }
        self.has_flash()
    }

    fn stop(&self) {
        if let Ok(mut slot) = session_slot().lock() {
            if let Some(holder) = slot.take() {
                unsafe { holder.session.stopRunning() };
            }
        }
        if let Ok(mut f) = latest().lock() {
            *f = None;
        }
    }
}

/// The back cameras a phone carries, widest field of view first.
///
/// The virtual triple/dual devices are left out: they are what
/// [`select_camera_device`] opens when the app picks no lens, and listing them
/// beside their own constituents would offer the same picture twice.
fn back_lenses() -> Vec<Retained<AVCaptureDevice>> {
    let Some(media_type) = (unsafe { AVMediaTypeVideo }) else {
        return Vec::new();
    };
    let types: [&AVCaptureDeviceType; 3] = unsafe {
        [
            AVCaptureDeviceTypeBuiltInUltraWideCamera,
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeBuiltInTelephotoCamera,
        ]
    };
    let wanted = NSArray::from_slice(&types);
    let session = unsafe {
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &wanted,
            Some(media_type),
            AVCaptureDevicePosition::Back,
        )
    };
    let mut found: Vec<Retained<AVCaptureDevice>> = unsafe { session.devices() }.to_vec();
    found.sort_by_key(lens_order);
    found
}

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
                if let Ok(mut slot) = latest().lock() {
                    if let Some(old) = slot.replace(frame) {
                        if let Ok(mut pool) = buffer_pool().lock() {
                            if pool.len() < 3 {
                                pool.push(old.rgba);
                            }
                        }
                    }
                }
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
            if let Ok(mut slot) = photo_result_slot().lock() {
                if let Some(sender) = slot.take() {
                    let _ = sender.send(jpeg);
                }
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
        // `maxPhotoDimensions` (iOS 16+) supersedes these, but the deprecated
        // switches still map onto it and keep the iOS 15 floor working.
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

    // The sensor delivers landscape buffers; rotate 90° clockwise so the
    // in-app viewfinder is upright in portrait. Output is `height` x `width`.
    let out_w = height;
    let out_h = width;
    // Recycle a parked buffer when one fits (clear keeps capacity, so after
    // the first few frames this allocates nothing at all).
    let mut rgba = buffer_pool()
        .lock()
        .ok()
        .and_then(|mut pool| pool.pop())
        .unwrap_or_default();
    rgba.clear();
    rgba.resize(out_w * out_h * 4, 0);
    if !base.is_null() && bytes_per_row >= width * 4 {
        for sy in 0..height {
            let src_row = unsafe { base.add(sy * bytes_per_row) };
            let dx = height - 1 - sy;
            for sx in 0..width {
                let src = unsafe { src_row.add(sx * 4) };
                let (b, g, r, a) = unsafe { (*src, *src.add(1), *src.add(2), *src.add(3)) };
                // 90° clockwise: src(sx, sy) -> dst(height-1-sy, sx).
                let dst = (sx * out_w + dx) * 4;
                rgba[dst] = r;
                rgba[dst + 1] = g;
                rgba[dst + 2] = b;
                rgba[dst + 3] = a;
            }
        }
    }

    unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, flags) };

    Some(CameraFrame {
        width: out_w as u32,
        height: out_h as u32,
        rgba,
    })
}

/// Picks the back camera, preferring an auto-switching virtual multi-camera
/// (triple, then dual-wide) so the system can drop to the ultra-wide constituent
/// for macro (close-up receipts, below the wide lens's minimum focus distance).
/// Falls back to the plain wide-angle camera on devices without a virtual one.
fn select_camera_device(media_type: &AVMediaType) -> Option<Retained<AVCaptureDevice>> {
    if let Some(id) = lens_slot().lock().ok().and_then(|slot| slot.clone()) {
        let wanted = NSString::from_str(&id);
        if let Some(device) = unsafe { AVCaptureDevice::deviceWithUniqueID(&wanted) } {
            return Some(device);
        }
    }
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
        if unsafe { device.isAutoFocusRangeRestrictionSupported() } {
            unsafe {
                device.setAutoFocusRangeRestriction(AVCaptureAutoFocusRangeRestriction::Near)
            };
        }
        if unsafe { device.isFocusModeSupported(AVCaptureFocusMode::ContinuousAutoFocus) } {
            unsafe { device.setFocusMode(AVCaptureFocusMode::ContinuousAutoFocus) };
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
    // switch still maps onto it and keeps the iOS 15 floor working.
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
