//! The Apple decoder: a `VTDecompressionSession` fed by the app's thread. VideoToolbox's own
//! thread outputs each picture to a callback, which queues it for the app.
//!
//! The app's thread waits only for room: it sleeps in a condition variable while the decoder
//! holds too many pictures, and wakes when the callback frees a place.
#![expect(
    unsafe_code,
    reason = "VideoToolbox calls the output callback through a C function pointer and a raw pointer"
)]

use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

use objc2_core_foundation::CFRetained;
use objc2_core_media::CMTime;
use objc2_core_video::CVImageBuffer;
use objc2_video_toolbox::{
    VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord, kVTInvalidSessionErr,
    kVTVideoDecoderMalfunctionErr,
};
use parking_lot::{Condvar, Mutex};

use super::{
    AccessUnit, DecoderConfig, DecoderError, DecoderOutput, FrameInfo, VideoCodec, VideoFrame,
    admission::Admission,
    annex_b::AccessUnitNals,
    decompression_session::{Format, Session, TIMESCALE},
    frame_queue::{FrameQueue, Frames},
    pending_units::{PendingUnit, PendingUnits},
    pixel_buffer::{Image, LivePictures},
};

// The most access units that the decoder holds before it outputs a picture. A stream in real
// time keeps one or two in the decoder. More mean that the app feeds faster than the decoder
// decodes, and each adds a picture of delay.
const MAX_IN_FLIGHT: usize = 8;

// What the app's thread, the decoder's callback and the app's handles share.
struct Shared {
    pending: Mutex<PendingUnits>,
    // Wakes the app's thread when the callback takes a unit from `pending`
    room: Condvar,
    queue: Arc<FrameQueue>,
    // The pictures in `frames` and the pictures that the app holds
    live: LivePictures,
    // The most pictures that wait for the app or are in its hands
    depth: usize,
    // Set by the callback when the session reports that it can decode no more
    session_lost: AtomicBool,
}

// The format of the stream and the session that decodes it. The session ends when the stream's
// format changes in a way that it cannot take, or when VideoToolbox reports it lost, and a
// keyframe starts the next one.
#[derive(Default)]
struct Stream {
    format: Option<Format>,
    session: Option<Session>,
}

// The feeding side of a decoder.
pub(super) struct Backend {
    // The session ends before the state that its callback reads
    stream: Stream,
    shared: Arc<Shared>,
    codec: VideoCodec,
    nals: AccessUnitNals,
    admission: Admission,
    input_wait: Duration,
}

impl Backend {
    pub(super) fn new(config: DecoderConfig) -> Result<Self, DecoderError> {
        let DecoderConfig {
            codec,
            width,
            height,
            output,
            input_wait,
        } = config;
        if codec == VideoCodec::Av1 {
            return Err(DecoderError::NoDecoder(codec));
        }
        let DecoderOutput::Frames { depth } = output else {
            // A surface of the headset's compositor exists on Android only
            return Err(DecoderError::Unavailable);
        };
        let depth = depth.max(2);
        Ok(Self {
            stream: Stream::default(),
            shared: Arc::new(Shared {
                pending: Mutex::new(PendingUnits::new()),
                room: Condvar::new(),
                queue: Arc::new(FrameQueue::new(depth, 0)),
                live: LivePictures::default(),
                depth,
                session_lost: AtomicBool::new(false),
            }),
            codec,
            nals: AccessUnitNals::default(),
            admission: Admission::new((width as usize) * (height as usize)),
            input_wait,
        })
    }

    pub(super) fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        let pts_us = self.admission.check(&unit)?;
        self.nals.parse(self.codec, unit.data);
        if let Some(sets) = self.nals.parameter_sets(self.codec, unit.data) {
            self.stream.follow(self.codec, sets)?;
        }
        if self.nals.has_picture() {
            self.decode(unit, pts_us)?;
        }
        self.admission.accept(pts_us);
        self.shared.queue.stats.submitted();
        Ok(())
    }

    // Decodes the picture of the parsed access unit.
    fn decode(&mut self, unit: AccessUnit<'_>, pts_us: u64) -> Result<(), DecoderError> {
        self.stream.start(&self.shared, self.nals.has_keyframe())?;
        let (Some(format), Some(session)) = (&self.stream.format, &self.stream.session) else {
            return Err(DecoderError::NeedsKeyframe);
        };
        // The unit waits for its picture before the decoder can output it
        self.shared.reserve(
            PendingUnit {
                pts_us,
                timestamp: unit.timestamp,
                metadata: unit.metadata,
                submitted: Instant::now(),
            },
            self.input_wait,
        )?;
        let sample = format.sample(
            self.nals.length_prefixed_len(),
            |bytes| self.nals.write_length_prefixed(unit.data, bytes),
            pts_us,
        );
        let sample = match sample {
            Ok(sample) => sample,
            Err(error) => {
                self.shared.withdraw(pts_us);
                return Err(error);
            }
        };
        let status = session.decode(&sample);
        if status == 0 {
            return Ok(());
        }
        self.shared.withdraw(pts_us);
        if is_session_lost(status) {
            self.stream.session = None;
            return Err(DecoderError::NeedsKeyframe);
        }
        Err(DecoderError::Failed(format!(
            "the decoder refused the unit: {status}"
        )))
    }

    pub(super) fn frames(&self) -> Frames {
        Frames(Arc::clone(&self.shared.queue))
    }
}

impl Stream {
    // Takes the parameter sets of an access unit. A format that the sets change goes to the
    // session, or ends the session if it cannot take it.
    fn follow<'a>(
        &mut self,
        codec: VideoCodec,
        sets: impl Iterator<Item = &'a [u8]> + Clone,
    ) -> Result<(), DecoderError> {
        if self
            .format
            .as_ref()
            .is_some_and(|format| format.is_made_of(sets.clone()))
        {
            return Ok(());
        }
        let format = Format::new(codec, sets)?;
        if self
            .session
            .as_ref()
            .is_some_and(|session| !session.can_accept(&format))
        {
            self.session = None;
        }
        self.format = Some(format);
        Ok(())
    }

    // Makes the session that decodes the next picture, if there is none. Only a keyframe starts
    // a session.
    fn start(&mut self, shared: &Arc<Shared>, keyframe: bool) -> Result<(), DecoderError> {
        if shared.session_lost.swap(false, Relaxed) {
            self.session = None;
        }
        if self.session.is_some() {
            return Ok(());
        }
        let format = self.format.as_ref().ok_or(DecoderError::NeedsKeyframe)?;
        if !keyframe {
            return Err(DecoderError::NeedsKeyframe);
        }
        let callback = VTDecompressionOutputCallbackRecord {
            decompressionOutputCallback: Some(on_output),
            decompressionOutputRefCon: Arc::as_ptr(shared).cast_mut().cast(),
        };
        self.session = Some(Session::new(format, callback)?);
        Ok(())
    }
}

impl Shared {
    // Records `unit` as waiting for its picture. It waits up to `wait` for room.
    fn reserve(&self, unit: PendingUnit, wait: Duration) -> Result<(), DecoderError> {
        let deadline = Instant::now() + wait;
        let mut pending = self.pending.lock();
        while pending.len() >= MAX_IN_FLIGHT {
            if self.room.wait_until(&mut pending, deadline).timed_out() {
                return Err(DecoderError::Full);
            }
        }
        pending.push(unit);
        Ok(())
    }

    // Takes back a unit that the decoder did not accept.
    fn withdraw(&self, pts_us: u64) {
        self.pending.lock().remove(pts_us);
        self.room.notify_one();
    }

    // Takes the answer of the decoder for the unit at `pts_us`.
    fn output(&self, status: i32, info: VTDecodeInfoFlags, image: *mut CVImageBuffer, pts_us: u64) {
        let unit = self.pending.lock().remove(pts_us);
        self.room.notify_one();
        let Some(image) = NonNull::new(image).filter(|_| status == 0) else {
            if status != 0 {
                log::warn!("video decoder: picture at {pts_us} us failed: {status}");
                if is_session_lost(status) {
                    self.session_lost.store(true, Relaxed);
                }
            }
            return;
        };
        if info.contains(VTDecodeInfoFlags::FrameDropped) {
            return;
        }
        // SAFETY: the callback's image is valid for the call, and this takes a count of its own.
        let image = Image::new(unsafe { CFRetained::retain(image) }, &self.live);
        let frame_info = unit.map_or_else(
            || FrameInfo {
                timestamp: Duration::from_micros(pts_us),
                metadata: 0,
                decode_time: Duration::ZERO,
            },
            |unit| FrameInfo {
                timestamp: unit.timestamp,
                metadata: unit.metadata,
                decode_time: unit.submitted.elapsed(),
            },
        );
        self.queue.stats.decoded(frame_info.decode_time);
        let frame = VideoFrame::new(frame_info, image.size(), image.crop(), image);
        let mut frames = self.queue.frames.lock();
        // The oldest queued pictures go until the new one fits. If the app holds the rest, the
        // new one goes.
        while self.live.load(Relaxed) > self.depth {
            self.queue.stats.dropped();
            if frames.pop_front().is_none() {
                return;
            }
        }
        frames.push_back(frame);
    }
}

// Whether `status` says that the session ended, as it does when an app goes to the background.
fn is_session_lost(status: i32) -> bool {
    status == kVTInvalidSessionErr || status == kVTVideoDecoderMalfunctionErr
}

// VideoToolbox's output callback, on a thread of its own.
unsafe extern "C-unwind" fn on_output(
    refcon: *mut c_void,
    _frame_refcon: *mut c_void,
    status: i32,
    info: VTDecodeInfoFlags,
    image: *mut CVImageBuffer,
    pts: CMTime,
    _duration: CMTime,
) {
    // SAFETY: the refcon is the `Shared` that the session's owner holds an `Arc` of until it
    // drops the session, after which VideoToolbox calls no more.
    let shared = unsafe { &*refcon.cast::<Shared>() };
    let micros = i128::from(pts.value) * i128::from(TIMESCALE) / i128::from(pts.timescale.max(1));
    shared.output(status, info, image, u64::try_from(micros).unwrap_or(0));
}
