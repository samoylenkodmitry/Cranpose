//! The Apple decoder: a `VTDecompressionSession` fed by the app's thread. VideoToolbox's own
//! thread outputs each picture to a callback, which reorders B-frames when configured for
//! presentation order and queues ready pictures for the app.
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
        atomic::{AtomicBool, Ordering},
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
    AccessUnit, DecodeOrder, DecoderConfig, DecoderError, DecoderOutput, VideoCodec,
    VideoColorInfo, VideoFrame, VideoRotation,
    admission::Admission,
    annex_b::AccessUnitNals,
    decompression_session::{Format, Session, TIMESCALE},
    frame_queue::{FrameQueue, Frames},
    pending_units::{PendingUnit, PendingUnits},
    pixel_buffer::{Image, LivePictures},
    reorder_buffer::{DEFAULT_REORDER_DEPTH, ReorderBuffer},
    sps::StreamMetadata,
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
    // Holds decoded pictures while reordering B-frames into presentation order
    reorder: Mutex<ReorderBuffer>,
    // The pictures in `frames`, `reorder`, and the pictures that the app holds
    live: LivePictures,
    // The most pictures that wait for the app or are in its hands
    depth: usize,
    order: DecodeOrder,
    rotation: VideoRotation,
    config_color: Option<VideoColorInfo>,
    stream_meta: Mutex<StreamMetadata>,
    // Set during `flush` so in-flight callbacks discard stale pictures
    discard_output: AtomicBool,
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
            order,
            rotation,
            color,
        } = config;
        if matches!(codec, VideoCodec::Av1 | VideoCodec::Vp9) {
            return Err(DecoderError::NoDecoder(codec));
        }
        let DecoderOutput::Frames { depth } = output else {
            // A surface of the headset's compositor exists on Android only
            return Err(DecoderError::Unavailable);
        };
        let depth = depth.max(2);
        let initial_reorder = match order {
            DecodeOrder::LowLatency => 0,
            DecodeOrder::Presentation => DEFAULT_REORDER_DEPTH,
        };
        Ok(Self {
            stream: Stream::default(),
            shared: Arc::new(Shared {
                pending: Mutex::new(PendingUnits::new()),
                room: Condvar::new(),
                queue: Arc::new(FrameQueue::new(depth, 0)),
                reorder: Mutex::new(ReorderBuffer::new(initial_reorder)),
                live: LivePictures::default(),
                depth,
                order,
                rotation,
                config_color: color,
                stream_meta: Mutex::new(StreamMetadata::default()),
                discard_output: AtomicBool::new(false),
                session_lost: AtomicBool::new(false),
            }),
            codec,
            nals: AccessUnitNals::default(),
            admission: Admission::new((width as usize) * (height as usize), order),
            input_wait,
        })
    }

    pub(super) fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        let pts_us = self.admission.check(&unit)?;
        self.nals.parse(self.codec, unit.data);
        if let Some(sps) = self.nals.sequence_parameter_set(unit.data)
            && let Some(meta) = StreamMetadata::parse_sps(self.codec, sps)
        {
            self.shared.apply_stream_metadata(meta);
        }
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

    pub(super) fn drain(&mut self) -> Result<(), DecoderError> {
        if let Some(session) = &self.stream.session {
            session.wait_for_asynchronous_frames();
        }
        self.shared.drain_reordered();
        self.admission.reset();
        Ok(())
    }

    pub(super) fn flush(&mut self) -> Result<(), DecoderError> {
        self.shared.discard_output.store(true, Ordering::Release);
        if let Some(session) = &self.stream.session {
            session.wait_for_asynchronous_frames();
        }
        self.shared.pending.lock().clear();
        self.shared.reorder.lock().clear();
        self.shared.queue.frames.lock().clear();
        self.shared.discard_output.store(false, Ordering::Release);
        self.shared.room.notify_all();
        self.admission.reset();
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
        let status = session.decode(&sample, self.shared.order);
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
        if shared.session_lost.swap(false, Ordering::Relaxed) {
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
        self.session = Some(Session::new(format, callback, shared.order)?);
        Ok(())
    }
}

impl Shared {
    fn apply_stream_metadata(&self, meta: StreamMetadata) {
        *self.stream_meta.lock() = meta;
        if self.order == DecodeOrder::Presentation
            && let Some(reorder_depth) = meta.max_reorder_frames
        {
            self.reorder.lock().set_depth(reorder_depth);
            loop {
                let (picture, buffered) = {
                    let mut reorder = self.reorder.lock();
                    let picture = reorder.pop_excess();
                    (picture, reorder.len())
                };
                let Some(picture) = picture else {
                    break;
                };
                let decode_time = picture.info().decode_time;
                self.enqueue(picture, buffered);
                self.queue.stats.decoded(decode_time);
            }
        }
    }

    // Records `unit` as waiting for its picture. It waits up to `wait` for room.
    fn reserve(&self, unit: PendingUnit, wait: Duration) -> Result<(), DecoderError> {
        let deadline = Instant::now() + wait;
        let max_in_flight = MAX_IN_FLIGHT + self.reorder.lock().depth();
        let mut pending = self.pending.lock();
        while pending.len() >= max_in_flight {
            if self.room.wait_until(&mut pending, deadline).timed_out() {
                self.queue.stats.refused();
                return Err(DecoderError::Full);
            }
        }
        pending.push(unit);
        drop(pending);
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
        if self.discard_output.load(Ordering::Acquire) {
            return;
        }
        let Some(image) = NonNull::new(image).filter(|_| status == 0) else {
            if status != 0 {
                log::warn!("video decoder: picture at {pts_us} us failed: {status}");
                if is_session_lost(status) {
                    self.session_lost.store(true, Ordering::Relaxed);
                }
            }
            self.queue.stats.dropped();
            return;
        };
        if info.contains(VTDecodeInfoFlags::FrameDropped) {
            self.queue.stats.dropped();
            return;
        }
        // SAFETY: the callback's image is valid for the call, and this takes a count of its own.
        let image = Image::new(unsafe { CFRetained::retain(image) }, &self.live);
        let frame_info = PendingUnit::into_frame_info(unit, pts_us);
        let stream_meta = *self.stream_meta.lock();
        let color = image.color(stream_meta, self.config_color);
        let frame = VideoFrame::new(
            frame_info,
            image.size(),
            image.crop(),
            color,
            self.rotation,
            image,
        );
        self.push_reordered(frame);
    }

    fn push_reordered(&self, frame: VideoFrame) {
        let (ready, buffered) = {
            let mut reorder = self.reorder.lock();
            let ready = reorder.push(frame);
            (ready, reorder.len())
        };
        if let Some(picture) = ready {
            let decode_time = picture.info().decode_time;
            self.enqueue(picture, buffered);
            // The count follows the queue: an app that sees a picture counted finds it in `frames`.
            self.queue.stats.decoded(decode_time);
        }
    }

    fn drain_reordered(&self) {
        loop {
            let (picture, remaining) = {
                let mut reorder = self.reorder.lock();
                let picture = reorder.pop_front();
                (picture, reorder.len())
            };
            let Some(picture) = picture else {
                break;
            };
            let decode_time = picture.info().decode_time;
            self.enqueue(picture, remaining);
            self.queue.stats.decoded(decode_time);
        }
    }

    // Queues `frame` and wakes the task that waits for a picture. The oldest queued pictures go
    // until the new one fits. If the app holds the rest, the new one goes.
    fn enqueue(&self, frame: VideoFrame, buffered_in_reorder: usize) {
        let budget = self.depth + buffered_in_reorder;
        let mut frames = self.queue.frames.lock();
        while self.live.load(Ordering::Relaxed) > budget {
            self.queue.stats.dropped();
            if frames.pop_front().is_none() {
                return;
            }
        }
        frames.push_back(frame);
        drop(frames);
        self.queue.wake();
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

#[cfg(test)]
#[path = "tests/video_toolbox_tests.rs"]
mod tests;
