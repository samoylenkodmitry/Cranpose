//! The live camera: what it is doing, the frames it is producing, and the
//! stills it takes.
//!
//! Three things about a camera are easy to get wrong and expensive to get wrong
//! twice, so the framework owns all three.
//!
//! A camera is **observable, not polled**. A viewfinder that asks "is there a
//! new frame yet?" every frame does that work whether or not one arrived, and
//! learns about a failure only by noticing that frames stopped. Here the
//! session publishes what it is doing and the frames it produces, and a screen
//! reacts.
//!
//! Frames arrive in the **format the sensor produced**. Encoding a preview
//! frame as JPEG to hand it across a language boundary and decoding it back
//! costs several milliseconds per frame, every frame, to arrive at the pixels
//! the camera already had. [`FrameFormat::Nv12`] is what a phone camera
//! actually produces, and it converts to RGBA in one pass over the bytes.
//!
//! Analysis is **bounded and latest-wins**. A detector that takes longer than a
//! frame interval must fall behind rather than accumulate: a queue of stale
//! frames costs memory to hold and produces answers about a scene that has
//! already moved.

use crate::registry::ServiceRegistry;
use cranpose_core::{rememberEventStream, EventStream, State};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// How a frame's pixels are laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FrameFormat {
    /// Tightly packed RGBA8, row-major, no padding.
    #[default]
    Rgba8,
    /// Tightly packed RGB8, row-major, no padding.
    ///
    /// What a USB webcam decodes to. Carried as-is so a desktop viewfinder does
    /// not pay a widening copy per frame for an alpha channel a camera never
    /// has.
    Rgb8,
    /// Full-range NV12: a `width * height` luma plane followed by an
    /// interleaved `width * height / 2` chroma plane at half resolution in both
    /// directions.
    ///
    /// What a phone camera produces. Carrying it as-is is what removes the
    /// encode-and-decode round trip a JPEG preview pays on every frame.
    Nv12,
}

impl FrameFormat {
    /// How many bytes a frame of this size occupies, or `None` when the size
    /// cannot be represented — which is a frame nobody can allocate anyway.
    pub fn byte_len(self, width: u32, height: u32) -> Option<usize> {
        let pixels = (width as usize).checked_mul(height as usize)?;
        match self {
            FrameFormat::Rgba8 => pixels.checked_mul(4),
            FrameFormat::Rgb8 => pixels.checked_mul(3),
            // NV12 needs even dimensions: the chroma plane is half-resolution
            // in both directions, and an odd row or column has no half.
            FrameFormat::Nv12 => {
                if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
                    return None;
                }
                pixels.checked_add(pixels / 2)
            }
        }
    }
}

/// One frame from the viewfinder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraFrame {
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    /// How far the frame must be rotated clockwise to be the right way up,
    /// which is the sensor's mounting plus the device's rotation.
    pub rotation_degrees: u16,
    /// Which frame this is in the session, so a consumer can tell a repeat from
    /// a new one and count what it missed.
    pub sequence: u64,
    /// The pixels, in [`format`](Self::format).
    pub bytes: Vec<u8>,
}

impl CameraFrame {
    /// A frame, or `None` when the bytes do not match the size and format —
    /// which is a backend bug, and one that reads as corrupted video rather
    /// than as an error if it is let through.
    pub fn new(
        width: u32,
        height: u32,
        format: FrameFormat,
        rotation_degrees: u16,
        sequence: u64,
        bytes: Vec<u8>,
    ) -> Option<Self> {
        if format.byte_len(width, height)? != bytes.len() {
            return None;
        }
        Some(Self {
            width,
            height,
            format,
            rotation_degrees: rotation_degrees % 360,
            sequence,
            bytes,
        })
    }

    /// The frame as tightly packed RGBA8, converting only when it has to.
    ///
    /// One pass over the bytes with no allocation beyond the result, because
    /// this runs per frame: a conversion that allocates per row shows up as
    /// dropped frames rather than as a slow function.
    pub fn to_rgba8(&self) -> Vec<u8> {
        match self.format {
            FrameFormat::Rgba8 => self.bytes.clone(),
            FrameFormat::Rgb8 => rgb8_to_rgba8(&self.bytes),
            FrameFormat::Nv12 => nv12_to_rgba8(self.width, self.height, &self.bytes),
        }
    }
}

/// Widens tightly packed RGB8 to RGBA8, opaque throughout.
fn rgb8_to_rgba8(bytes: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bytes.len() / 3 * 4);
    // Fixed-size chunks rather than a runtime length: the three reads below
    // then carry no bounds check, which is what lets this vectorise.
    for [red, green, blue] in bytes.as_chunks::<3>().0 {
        rgba.extend_from_slice(&[*red, *green, *blue, 255]);
    }
    rgba
}

/// Converts a full-range NV12 frame to tightly packed RGBA8.
///
/// The BT.601 full-range matrix, which is what `ImageFormat.YUV_420_888` and
/// the equivalent capture formats deliver. Written over whole rows so the
/// bounds checks fall out of the inner loop and the compiler can vectorise it,
/// which matters because this runs on every previewed frame.
fn nv12_to_rgba8(width: u32, height: u32, bytes: &[u8]) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let pixels = width * height;
    let mut rgba = vec![0u8; pixels * 4];
    if bytes.len() < pixels + pixels / 2 || width == 0 || height == 0 {
        return rgba;
    }
    let (luma, chroma) = bytes.split_at(pixels);

    for y in 0..height {
        let luma_row = &luma[y * width..(y + 1) * width];
        let chroma_row = &chroma[(y / 2) * width..(y / 2 + 1) * width];
        let out_row = &mut rgba[y * width * 4..(y + 1) * width * 4];
        for x in 0..width {
            let luminance = luma_row[x] as i32;
            let blue_difference = chroma_row[x & !1] as i32 - 128;
            let red_difference = chroma_row[(x & !1) + 1] as i32 - 128;
            let out = &mut out_row[x * 4..x * 4 + 4];
            out[0] = clamp_byte(luminance + ((91881 * red_difference) >> 16));
            out[1] =
                clamp_byte(luminance - ((22554 * blue_difference + 46802 * red_difference) >> 16));
            out[2] = clamp_byte(luminance + ((116130 * blue_difference) >> 16));
            out[3] = 255;
        }
    }
    rgba
}

fn clamp_byte(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// A full-resolution still photograph as an encoded image.
///
/// Unlike a viewfinder frame, a still comes through the platform's dedicated
/// photo pipeline at full sensor resolution. The bytes are an encoded JPEG
/// whose EXIF orientation tag reflects the device rotation; decode with an
/// orientation-aware decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraStill {
    pub jpeg: Vec<u8>,
}

/// One capture device the application may pick.
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
/// no flash reports `false` from [`Camera::set_flash`] and the application
/// hides the control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlashMode {
    #[default]
    Off,
    Auto,
    On,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum CameraError {
    /// No live camera backend on this platform.
    #[error("live camera capture is not supported here")]
    Unsupported,
    /// The user denied camera access.
    #[error("camera permission denied")]
    PermissionDenied,
    /// A still was asked for while nothing was running.
    #[error("the camera is not running")]
    NotRunning,
    /// Any other failure — no device, a configuration the hardware refused.
    #[error("{0}")]
    Failed(String),
}

/// What the camera session is doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CameraState {
    /// Nothing has been asked of it.
    #[default]
    Idle,
    /// A session is opening. Phones take a noticeable moment over this, which
    /// is why it is a state a screen can show rather than a gap before frames.
    Starting,
    /// Frames are being produced.
    Running {
        /// The device the session opened, for a label.
        device: String,
    },
    /// The session was stopped and the device released.
    Stopped,
    /// The session could not start, or ended on its own.
    Failed(CameraError),
}

impl CameraState {
    /// Whether frames are being produced now.
    pub fn is_running(&self) -> bool {
        matches!(self, CameraState::Running { .. })
    }

    /// Whether the session is opening or open — which is when a screen keeps
    /// the viewfinder on screen rather than showing a placeholder.
    pub fn is_active(&self) -> bool {
        matches!(self, CameraState::Starting | CameraState::Running { .. })
    }

    /// The failure, if the session ended in one.
    pub fn failure(&self) -> Option<&CameraError> {
        match self {
            CameraState::Failed(error) => Some(error),
            _ => None,
        }
    }
}

/// A platform capture session.
///
/// A backend starts and stops the device and publishes what it produces through
/// [`publish_camera_frame`] and [`publish_camera_state`]; nothing here is
/// polled, and no method blocks for the length of a capture.
pub trait Camera: Send + Sync {
    /// Opens the session.
    ///
    /// Returns as soon as the request is accepted. The session's progress
    /// arrives as [`CameraState`], because opening a camera takes long enough
    /// on a phone that a screen has to show something in the meantime.
    fn start(&self) -> Result<(), CameraError>;

    /// Stops the session and releases the device.
    fn stop(&self);

    /// Asks for a full-resolution still.
    ///
    /// The picture arrives through [`publish_camera_still`], because the device
    /// takes as long as it takes to expose and encode, and a call that blocked
    /// for it would block whatever asked.
    fn request_still(&self) -> Result<(), CameraError> {
        Err(CameraError::Unsupported)
    }

    /// Turns the torch on or off while the session runs.
    ///
    /// Scanner-style applications light a dim scene rather than analysing
    /// photon-starved frames. Returns `false` where the device has no torch;
    /// the torch dies with the session.
    fn set_torch(&self, _on: bool) -> bool {
        false
    }

    /// The devices the application may pick between, back cameras first and in
    /// field-of-view order, widest first.
    ///
    /// An empty list means the application shows no lens control: either the
    /// platform has one camera or the backend does not list them.
    fn lenses(&self) -> Vec<CameraLens> {
        Vec::new()
    }

    /// The device the session is using, or `None` when nothing is open and the
    /// backend has no stored choice.
    fn lens(&self) -> Option<String> {
        None
    }

    /// Opens `id` instead of the current device, keeping the session running.
    /// Returns `false` when the id is unknown or the backend cannot switch.
    fn use_lens(&self, _id: &str) -> bool {
        false
    }

    /// Whether the current device has a flash for stills.
    fn has_flash(&self) -> bool {
        false
    }

    /// What the flash does on the next still. Returns `false` where the device
    /// has no flash; the mode dies with the session.
    fn set_flash(&self, _mode: FlashMode) -> bool {
        false
    }
}

/// Shared handle to the platform camera.
pub type CameraRef = Arc<dyn Camera>;

static PLATFORM_CAMERA: ServiceRegistry<dyn Camera> = ServiceRegistry::new();

/// Installs the platform camera, replacing any previous one.
pub fn set_platform_camera(camera: CameraRef) {
    PLATFORM_CAMERA.set(camera);
}

/// Removes the platform camera and forgets everything the last session
/// published.
pub fn clear_platform_camera() {
    PLATFORM_CAMERA.clear();
    if let Ok(mut observers) = frame_observers().lock() {
        observers.clear();
    }
    if let Ok(mut observers) = state_observers().lock() {
        observers.clear();
    }
    if let Ok(mut observers) = still_observers().lock() {
        observers.clear();
    }
    if let Ok(mut latest) = latest_frame_slot().lock() {
        *latest = None;
    }
    if let Ok(mut state) = state_slot().lock() {
        *state = CameraState::Idle;
    }
    DROPPED_FRAMES.store(0, Ordering::Release);
}

/// The installed camera, or `None` where live capture is unsupported — which is
/// when an application falls back to the image picker.
pub fn camera() -> Option<CameraRef> {
    PLATFORM_CAMERA.get()
}

/// Whether this platform has a live camera at all.
pub fn camera_supported() -> bool {
    PLATFORM_CAMERA.get().is_some()
}

fn state_slot() -> &'static Mutex<CameraState> {
    static SLOT: OnceLock<Mutex<CameraState>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(CameraState::Idle))
}

fn latest_frame_slot() -> &'static Mutex<Option<CameraFrame>> {
    static SLOT: OnceLock<Mutex<Option<CameraFrame>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Frames produced while every observer was still busy with an earlier one.
///
/// Counted rather than queued: a detector that falls behind should see the
/// scene as it is now and know how much it missed, not work through a backlog
/// describing a scene that has moved.
static DROPPED_FRAMES: AtomicU64 = AtomicU64::new(0);

type FrameObserver = Arc<dyn Fn(CameraFrame) + Send + Sync>;
type StateObserver = Arc<dyn Fn(CameraState) + Send + Sync>;
type StillObserver = Arc<dyn Fn(Result<CameraStill, CameraError>) + Send + Sync>;

fn frame_observers() -> &'static Mutex<Vec<(u64, FrameObserver)>> {
    static SLOT: OnceLock<Mutex<Vec<(u64, FrameObserver)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

fn state_observers() -> &'static Mutex<Vec<(u64, StateObserver)>> {
    static SLOT: OnceLock<Mutex<Vec<(u64, StateObserver)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

fn still_observers() -> &'static Mutex<Vec<(u64, StillObserver)>> {
    static SLOT: OnceLock<Mutex<Vec<(u64, StillObserver)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

static NEXT_OBSERVER: AtomicU64 = AtomicU64::new(1);

/// Keeps a camera observer registered until it is dropped.
pub struct CameraObserver {
    id: u64,
    kind: ObserverKind,
}

#[derive(Clone, Copy)]
enum ObserverKind {
    Frame,
    State,
    Still,
}

impl Drop for CameraObserver {
    fn drop(&mut self) {
        match self.kind {
            ObserverKind::Frame => retain_without(frame_observers(), self.id),
            ObserverKind::State => retain_without(state_observers(), self.id),
            ObserverKind::Still => retain_without(still_observers(), self.id),
        }
    }
}

fn retain_without<T>(slot: &'static Mutex<Vec<(u64, T)>>, id: u64) {
    if let Ok(mut observers) = slot.lock() {
        observers.retain(|(observer, _)| *observer != id);
    }
}

fn snapshot<T: Clone>(slot: &'static Mutex<Vec<(u64, T)>>) -> Vec<T> {
    slot.lock()
        .map(|observers| {
            observers
                .iter()
                .map(|(_, observer)| observer.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The last frame the session produced, or `None` before the first one.
///
/// Read outside composition — during draw — so a viewfinder shows the newest
/// frame without a recomposition per frame.
pub fn latest_camera_frame() -> Option<CameraFrame> {
    latest_frame_slot()
        .lock()
        .map(|frame| frame.clone())
        .unwrap_or(None)
}

/// What the session is doing.
pub fn camera_state() -> CameraState {
    state_slot()
        .lock()
        .map(|state| state.clone())
        .unwrap_or_default()
}

/// How many frames were produced while every observer was still busy.
pub fn dropped_camera_frames() -> u64 {
    DROPPED_FRAMES.load(Ordering::Acquire)
}

/// Publishes a frame. Backends call this from whichever thread the platform
/// delivers frames on.
///
/// The newest frame always replaces the stored one, so a viewfinder never draws
/// a stale frame; observers that are keeping up see every frame, and one that
/// is not is counted in [`dropped_camera_frames`] rather than queued behind.
pub fn publish_camera_frame(frame: CameraFrame) {
    if let Ok(mut latest) = latest_frame_slot().lock() {
        *latest = Some(frame.clone());
    }
    let observers = snapshot(frame_observers());
    if observers.is_empty() {
        return;
    }
    for observer in observers {
        observer(frame.clone());
    }
}

/// Records that the platform produced a frame nobody could take.
///
/// A backend running a bounded analysis queue calls this when it drops one, so
/// the count reflects what the device produced rather than what got through.
pub fn record_dropped_camera_frame() {
    DROPPED_FRAMES.fetch_add(1, Ordering::AcqRel);
}

/// Publishes what the session is doing.
pub fn publish_camera_state(state: CameraState) {
    {
        let Ok(mut current) = state_slot().lock() else {
            return;
        };
        if *current == state {
            return;
        }
        *current = state.clone();
    }
    if matches!(state, CameraState::Idle | CameraState::Starting) {
        DROPPED_FRAMES.store(0, Ordering::Release);
        if let Ok(mut latest) = latest_frame_slot().lock() {
            *latest = None;
        }
    }
    for observer in snapshot(state_observers()) {
        observer(state.clone());
    }
}

/// Publishes the answer to a still request.
pub fn publish_camera_still(still: Result<CameraStill, CameraError>) {
    for observer in snapshot(still_observers()) {
        observer(still.clone());
    }
}

/// Registers `observer` for frames. Applications collect
/// [`rememberCameraFrames`] instead of calling this.
pub fn observe_camera_frames(
    observer: impl Fn(CameraFrame) + Send + Sync + 'static,
) -> CameraObserver {
    let id = NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut observers) = frame_observers().lock() {
        observers.push((id, Arc::new(observer)));
    }
    CameraObserver {
        id,
        kind: ObserverKind::Frame,
    }
}

/// Registers `observer` for session state. The current state is delivered at
/// once, so a screen composed mid-session shows what is happening rather than
/// waiting for the next change.
pub fn observe_camera_state(
    observer: impl Fn(CameraState) + Send + Sync + 'static,
) -> CameraObserver {
    let id = NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
    let observer: StateObserver = Arc::new(observer);
    if let Ok(mut observers) = state_observers().lock() {
        observers.push((id, Arc::clone(&observer)));
    }
    observer(camera_state());
    CameraObserver {
        id,
        kind: ObserverKind::State,
    }
}

/// Registers `observer` for stills.
pub fn observe_camera_stills(
    observer: impl Fn(Result<CameraStill, CameraError>) + Send + Sync + 'static,
) -> CameraObserver {
    let id = NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut observers) = still_observers().lock() {
        observers.push((id, Arc::new(observer)));
    }
    CameraObserver {
        id,
        kind: ObserverKind::Still,
    }
}

/// What the camera session is doing, observed for as long as this call stays in
/// the composition.
#[allow(non_snake_case)]
pub fn rememberCameraState() -> State<CameraState> {
    let updates = rememberEventStream((), |sender| {
        observe_camera_state(move |state| sender.send(state))
    });
    cranpose_core::collectAsState(updates, (), camera_state())
}

/// The frames the session produces, as a stream this composition collects.
///
/// This is the analysis path: a detector collects it, and one that cannot keep
/// up falls behind by frames rather than by memory — see
/// [`dropped_camera_frames`]. A viewfinder draws [`latest_camera_frame`]
/// instead, which costs no recomposition at all.
#[allow(non_snake_case)]
pub fn rememberCameraFrames() -> EventStream<CameraFrame> {
    rememberEventStream((), |sender| {
        observe_camera_frames(move |frame| sender.send(frame))
    })
}

/// The stills the session produces, as a stream this composition collects.
#[allow(non_snake_case)]
pub fn rememberCameraStills() -> EventStream<Result<CameraStill, CameraError>> {
    rememberEventStream((), |sender| {
        observe_camera_stills(move |still| sender.send(still))
    })
}

/// Starts the camera, publishing [`CameraState::Starting`] before the backend
/// is asked so a screen shows the wait rather than a gap.
pub fn start_camera() -> Result<(), CameraError> {
    let Some(camera) = camera() else {
        publish_camera_state(CameraState::Failed(CameraError::Unsupported));
        return Err(CameraError::Unsupported);
    };
    publish_camera_state(CameraState::Starting);
    camera.start().inspect_err(|error| {
        publish_camera_state(CameraState::Failed(error.clone()));
    })
}

/// Stops the camera and releases the device.
pub fn stop_camera() {
    if let Some(camera) = camera() {
        camera.stop();
    }
    publish_camera_state(CameraState::Stopped);
}

/// Asks for a full-resolution still, which arrives through
/// [`rememberCameraStills`].
pub fn request_camera_still() -> Result<(), CameraError> {
    let Some(camera) = camera() else {
        return Err(CameraError::Unsupported);
    };
    if !camera_state().is_running() {
        return Err(CameraError::NotRunning);
    }
    camera.request_still()
}

/// Takes one full-resolution still and resolves with it.
///
/// [`request_camera_still`] asks and [`observe_camera_stills`] answers, which is
/// the right shape for a screen that keeps a shutter open. A caller that wants
/// one photograph wants the two joined, and joining them by hand means holding
/// an observer alive across an await in every application that takes a picture.
///
/// The observer is dropped as soon as a still arrives, so a second capture is a
/// second call rather than a subscription to unregister.
pub async fn capture_camera_still() -> Result<CameraStill, CameraError> {
    let signal = crate::async_io::Signal::new();
    let deliver = signal.clone();
    let observer = observe_camera_stills(move |result| deliver.set(result));
    request_camera_still()?;
    let arrived = signal.wait().await;
    drop(observer);
    // The signal only closes empty if the backend went away mid-capture, which
    // is a camera that stopped rather than a still that failed.
    arrived.unwrap_or(Err(CameraError::NotRunning))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend that records what it was asked and publishes what a real one
    /// would.
    struct FakeCamera {
        started: AtomicU64,
        stopped: AtomicU64,
        stills: AtomicU64,
        fails: bool,
    }

    impl FakeCamera {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                started: AtomicU64::new(0),
                stopped: AtomicU64::new(0),
                stills: AtomicU64::new(0),
                fails: false,
            })
        }

        fn failing() -> Arc<Self> {
            Arc::new(Self {
                started: AtomicU64::new(0),
                stopped: AtomicU64::new(0),
                stills: AtomicU64::new(0),
                fails: true,
            })
        }
    }

    impl Camera for FakeCamera {
        fn start(&self) -> Result<(), CameraError> {
            self.started.fetch_add(1, Ordering::Relaxed);
            if self.fails {
                return Err(CameraError::PermissionDenied);
            }
            publish_camera_state(CameraState::Running {
                device: "fake".to_string(),
            });
            Ok(())
        }

        fn stop(&self) {
            self.stopped.fetch_add(1, Ordering::Relaxed);
        }

        fn request_still(&self) -> Result<(), CameraError> {
            self.stills.fetch_add(1, Ordering::Relaxed);
            publish_camera_still(Ok(CameraStill {
                jpeg: vec![0xff, 0xd8],
            }));
            Ok(())
        }
    }

    fn rgba_frame(sequence: u64) -> CameraFrame {
        CameraFrame::new(2, 2, FrameFormat::Rgba8, 90, sequence, vec![7; 16])
            .expect("a well-formed frame")
    }

    #[test]
    fn a_frame_size_is_the_one_its_format_implies() {
        assert_eq!(FrameFormat::Rgba8.byte_len(4, 2), Some(32));
        assert_eq!(FrameFormat::Nv12.byte_len(4, 2), Some(12));
        assert_eq!(
            FrameFormat::Nv12.byte_len(3, 2),
            None,
            "NV12 has no half column for an odd width"
        );
        assert_eq!(FrameFormat::Nv12.byte_len(4, 3), None);
    }

    /// A frame whose bytes do not match its size reads as corrupted video
    /// rather than as an error, so it is refused where it enters.
    #[test]
    fn a_frame_that_does_not_match_its_size_is_refused() {
        assert!(CameraFrame::new(2, 2, FrameFormat::Rgba8, 0, 0, vec![0; 15]).is_none());
        assert!(CameraFrame::new(2, 2, FrameFormat::Nv12, 0, 0, vec![0; 5]).is_none());
        assert!(CameraFrame::new(2, 2, FrameFormat::Rgba8, 0, 0, vec![0; 16]).is_some());
    }

    #[test]
    fn a_rotation_is_kept_inside_one_turn() {
        let frame = CameraFrame::new(2, 2, FrameFormat::Rgba8, 450, 0, vec![0; 16])
            .expect("a well-formed frame");
        assert_eq!(frame.rotation_degrees, 90);
    }

    #[test]
    fn an_rgba_frame_is_handed_over_unchanged() {
        let frame = rgba_frame(0);
        assert_eq!(frame.to_rgba8(), frame.bytes);
    }

    /// The conversion has to agree with what every other BT.601 full-range
    /// implementation produces, or the viewfinder is a different colour from
    /// the photo the same camera takes.
    #[test]
    fn nv12_black_white_and_primaries_convert_to_the_expected_colours() {
        fn convert(luma: u8, blue: u8, red: u8) -> [u8; 4] {
            let bytes = vec![luma, luma, luma, luma, blue, red];
            let frame = CameraFrame::new(2, 2, FrameFormat::Nv12, 0, 0, bytes)
                .expect("a well-formed frame");
            let rgba = frame.to_rgba8();
            [rgba[0], rgba[1], rgba[2], rgba[3]]
        }

        assert_eq!(convert(0, 128, 128), [0, 0, 0, 255], "black");
        assert_eq!(convert(255, 128, 128), [255, 255, 255, 255], "white");

        let red = convert(76, 84, 255);
        assert!(
            red[0] > 240 && red[1] < 20 && red[2] < 20,
            "red, got {red:?}"
        );
        let blue = convert(29, 255, 107);
        assert!(
            blue[2] > 240 && blue[0] < 20 && blue[1] < 20,
            "blue, got {blue:?}"
        );
        assert!(
            convert(128, 128, 128).iter().all(|value| *value > 0),
            "a grey frame must not clamp to black"
        );
    }

    #[test]
    fn a_short_nv12_frame_converts_to_black_rather_than_reading_past_its_end() {
        let rgba = nv12_to_rgba8(4, 4, &[0u8; 3]);
        assert_eq!(rgba.len(), 4 * 4 * 4);
        assert!(rgba.iter().all(|value| *value == 0));
    }

    #[test]
    fn a_platform_without_a_camera_says_so_rather_than_pretending() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        assert!(!camera_supported());
        assert_eq!(start_camera(), Err(CameraError::Unsupported));
        assert_eq!(
            camera_state(),
            CameraState::Failed(CameraError::Unsupported)
        );
        assert_eq!(request_camera_still(), Err(CameraError::Unsupported));
        clear_platform_camera();
    }

    /// Opening a camera takes long enough on a phone that a screen has to show
    /// something in the meantime, so the wait is a state and not a gap.
    #[test]
    fn the_session_reports_starting_before_it_reports_running() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let observer = observe_camera_state(move |state| {
            recorder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(state)
        });
        set_platform_camera(FakeCamera::new());
        start_camera().expect("the session starts");

        assert_eq!(
            *seen.lock().unwrap_or_else(|error| error.into_inner()),
            vec![
                CameraState::Idle,
                CameraState::Starting,
                CameraState::Running {
                    device: "fake".to_string()
                }
            ]
        );
        assert!(camera_state().is_running());
        assert!(camera_state().is_active());
        drop(observer);
        clear_platform_camera();
    }

    #[test]
    fn a_session_that_cannot_open_reports_why() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        set_platform_camera(FakeCamera::failing());
        assert_eq!(start_camera(), Err(CameraError::PermissionDenied));
        assert_eq!(
            camera_state().failure(),
            Some(&CameraError::PermissionDenied)
        );
        assert!(!camera_state().is_active());
        clear_platform_camera();
    }

    #[test]
    fn stopping_releases_the_device_and_says_so() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        let backend = FakeCamera::new();
        set_platform_camera(backend.clone());
        start_camera().expect("the session starts");
        stop_camera();
        assert_eq!(backend.stopped.load(Ordering::Relaxed), 1);
        assert_eq!(camera_state(), CameraState::Stopped);
        clear_platform_camera();
    }

    /// A viewfinder draws the newest frame, so the stored one is always the
    /// newest — never a queue a slow drawer works through.
    #[test]
    fn the_stored_frame_is_always_the_newest_one() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        assert_eq!(latest_camera_frame(), None);
        publish_camera_frame(rgba_frame(1));
        publish_camera_frame(rgba_frame(2));
        publish_camera_frame(rgba_frame(3));
        assert_eq!(latest_camera_frame().map(|frame| frame.sequence), Some(3));
        clear_platform_camera();
    }

    #[test]
    fn frame_observers_see_frames_and_stop_when_dropped() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let observer = observe_camera_frames(move |frame| {
            recorder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(frame.sequence)
        });
        publish_camera_frame(rgba_frame(1));
        publish_camera_frame(rgba_frame(2));
        drop(observer);
        publish_camera_frame(rgba_frame(3));
        assert_eq!(
            *seen.lock().unwrap_or_else(|error| error.into_inner()),
            vec![1, 2]
        );
        clear_platform_camera();
    }

    /// A detector that cannot keep up must fall behind by frames, and the
    /// framework has to be able to say how far.
    #[test]
    fn frames_the_platform_could_not_deliver_are_counted_rather_than_queued() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        assert_eq!(dropped_camera_frames(), 0);
        record_dropped_camera_frame();
        record_dropped_camera_frame();
        assert_eq!(dropped_camera_frames(), 2);
        // A new session starts the count and the stored frame over: what the
        // last one missed says nothing about this one.
        publish_camera_state(CameraState::Starting);
        assert_eq!(dropped_camera_frames(), 0);
        assert_eq!(latest_camera_frame(), None);
        clear_platform_camera();
    }

    /// A still takes as long as the device takes; asking must not block
    /// whatever asked.
    #[test]
    fn a_still_is_asked_for_and_arrives_separately() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        let backend = FakeCamera::new();
        set_platform_camera(backend.clone());

        assert_eq!(
            request_camera_still(),
            Err(CameraError::NotRunning),
            "nothing is running, so there is nothing to photograph"
        );

        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let observer = observe_camera_stills(move |still| {
            recorder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(still)
        });
        start_camera().expect("the session starts");
        request_camera_still().expect("a still is asked for");
        assert_eq!(backend.stills.load(Ordering::Relaxed), 1);
        assert_eq!(
            *seen.lock().unwrap_or_else(|error| error.into_inner()),
            vec![Ok(CameraStill {
                jpeg: vec![0xff, 0xd8]
            })]
        );
        drop(observer);
        clear_platform_camera();
    }

    #[test]
    fn a_backend_that_lists_no_lens_and_no_flash_says_so() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        set_platform_camera(FakeCamera::new());
        let backend = camera().expect("registered");
        assert!(backend.lenses().is_empty());
        assert_eq!(backend.lens(), None);
        assert!(!backend.use_lens("0"));
        assert!(!backend.has_flash());
        assert!(!backend.set_flash(FlashMode::On));
        assert!(!backend.set_torch(true));
        clear_platform_camera();
    }

    #[test]
    fn a_backend_that_lists_two_lenses_hands_them_over_in_order() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_camera();
        struct TwoLenses;
        impl Camera for TwoLenses {
            fn start(&self) -> Result<(), CameraError> {
                Ok(())
            }
            fn stop(&self) {}
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
        }
        set_platform_camera(Arc::new(TwoLenses));
        let backend = camera().expect("registered");
        let lenses = backend.lenses();
        assert_eq!(lenses.len(), 2);
        assert_eq!(lenses[0].name, "Ultra wide");
        assert_eq!(backend.lens().as_deref(), Some("w"));
        assert!(backend.use_lens("u"));
        assert!(!backend.use_lens("tele"));
        clear_platform_camera();
    }
}
