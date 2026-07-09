//! iOS live camera via `AVCaptureSession`.
//!
//! Registered as the platform camera (see
//! [`cranpose_services::set_platform_camera`]) by the iOS backend. A video data
//! output delivers BGRA frames on a background dispatch queue; each frame is
//! converted to tightly-packed RGBA and stored so the app's preview pump can
//! poll it (matching the desktop/Android live viewfinder).
#![allow(unsafe_code)]

use block2::RcBlock;
use cranpose_services::{set_platform_camera, Camera, CameraError, CameraFrame};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession,
    AVCaptureSessionPreset1280x720, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeVideo,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    kCVPixelBufferPixelFormatTypeKey, CVBuffer, CVPixelBuffer, CVPixelBufferGetBaseAddress,
    CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

/// `'BGRA'` — 32-bit BGRA, the pixel format we request from the video output.
const PIXEL_FORMAT_32BGRA: u32 = 0x4247_5241;

fn latest() -> &'static Mutex<Option<CameraFrame>> {
    static SLOT: OnceLock<Mutex<Option<CameraFrame>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Holds the running session alive. Its AVFoundation objects are only touched
/// from `start`/`stop` (the app's single preview-pump thread) and the frame
/// delegate (its own dispatch queue, which only writes [`latest`]); marking it
/// `Send` lets it live behind the session mutex.
struct SessionHolder {
    session: Retained<AVCaptureSession>,
    _delegate: Retained<FrameDelegate>,
    _queue: DispatchRetained<DispatchQueue>,
}
unsafe impl Send for SessionHolder {}

fn session_slot() -> &'static Mutex<Option<SessionHolder>> {
    static SLOT: OnceLock<Mutex<Option<SessionHolder>>> = OnceLock::new();
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
                    *slot = Some(frame);
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

    let mut rgba = vec![0u8; width * height * 4];
    if !base.is_null() && bytes_per_row >= width * 4 {
        for y in 0..height {
            let src_row = unsafe { base.add(y * bytes_per_row) };
            let dst_row = y * width * 4;
            for x in 0..width {
                let src = unsafe { src_row.add(x * 4) };
                let (b, g, r, a) = unsafe { (*src, *src.add(1), *src.add(2), *src.add(3)) };
                let dst = dst_row + x * 4;
                rgba[dst] = r;
                rgba[dst + 1] = g;
                rgba[dst + 2] = b;
                rgba[dst + 3] = a;
            }
        }
    }

    unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, flags) };

    Some(CameraFrame {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

fn start_session() -> Result<String, CameraError> {
    let media_type = unsafe { AVMediaTypeVideo }
        .ok_or_else(|| CameraError::Failed("AVMediaTypeVideo unavailable".into()))?;

    // Trigger the permission prompt (first launch); frames flow once granted.
    let handler = RcBlock::new(|_granted: Bool| {});
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler) };

    let device = unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media_type) }
        .ok_or(CameraError::Unsupported)?;
    let name = unsafe { device.localizedName() }.to_string();
    let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }
        .map_err(|_| CameraError::Failed("could not open camera input".into()))?;

    let session = unsafe { AVCaptureSession::new() };
    unsafe { session.beginConfiguration() };
    let preset = unsafe { AVCaptureSessionPreset1280x720 };
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
    unsafe { session.commitConfiguration() };
    unsafe { session.startRunning() };

    if let Ok(mut slot) = session_slot().lock() {
        *slot = Some(SessionHolder {
            session,
            _delegate: delegate,
            _queue: queue,
        });
    }
    Ok(name)
}
