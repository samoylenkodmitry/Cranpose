//! The Android decoder: an `AMediaCodec` fed by the app's thread and drained by a thread of its
//! own.
//!
//! Android <= 11 (API 30) `MediaCodec` stores a single `mActivityNotify` and mutates internal
//! buffer tables without full cross-thread locking, aborting with `CHECK(mActivityNotify == NULL)`
//! or returning NULL from `AMediaCodec_getInputBuffer` when two threads call `AMediaCodec_*`
//! concurrently. All `AMediaCodec` calls therefore run non-blocking (`Duration::ZERO`) under a
//! short-lived `Mutex`, while both threads sleep outside the lock on `Condvar`s that wake
//! immediately when inputs arrive or outputs release.
#![expect(
    unsafe_code,
    reason = "AMediaCodec is a C object serialized behind a Mutex so ndk::MediaCodec can cross threads"
)]

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use ndk::{
    media::{
        media_codec::{
            DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec,
            MediaCodecDirection, OutputBuffer,
        },
        media_format::MediaFormat,
    },
    native_window::NativeWindow,
};
use parking_lot::{Condvar, Mutex};
use portable_atomic::AtomicU64;

use super::{
    AccessUnit, ColorPrimaries, ColorRange, ColorTransfer, DecodeOrder, DecoderConfig,
    DecoderError, DecoderOutput, FrameInfo, VideoCodec, VideoColorInfo, VideoFrame, VideoRotation,
    admission::Admission,
    annex_b::AccessUnitNals,
    frame_queue::{FrameQueue, Frames},
    image_reader::{Acquired, Reader},
    pending_units::{PendingUnit, PendingUnits},
    sps::StreamMetadata,
};

// Idle wait when no frames are in flight.
const IDLE_WAIT: Duration = Duration::from_millis(20);

// Poll step while frames are in flight inside the hardware codec or AImageReader.
const ACTIVE_WAIT: Duration = Duration::from_millis(1);

// How long a rendered picture's image may take before the decoder counts the picture as dropped.
const IMAGE_LIMIT: Duration = Duration::from_millis(100);

// Maximum time `drain` waits for the hardware codec to emit its end-of-stream output buffer.
const DRAIN_WAIT: Duration = Duration::from_secs(2);

// Android `AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM`.
const BUFFER_FLAG_END_OF_STREAM: u32 = 4;

// The reports of the pictures sent to a surface that the decoder keeps for the app.
const REPORTS: usize = 32;

// The operating rate that asks the codec for its top speed, in frames a second.
const OPERATING_RATE: i32 = i16::MAX as i32;

// Serializes all `AMediaCodec` operations so no two threads ever enter `libmediandk` concurrently.
struct Codec(Mutex<MediaCodec>);

// SAFETY: Every access to the inner `MediaCodec` is mutually excluded by the `Mutex`.
unsafe impl Send for Codec {}
// SAFETY: as for Send.
unsafe impl Sync for Codec {}

// Color fields parsed from Android's `AMediaFormat` output format.
#[derive(Clone, Copy, Default)]
struct FormatColor {
    primaries: Option<ColorPrimaries>,
    range: Option<ColorRange>,
    transfer: Option<ColorTransfer>,
}

// What the feeding thread, the draining thread and the app's handles share.
struct Shared {
    pending: Mutex<PendingUnits>,
    queue: Arc<FrameQueue>,
    stop: AtomicBool,
    flush_epoch: AtomicU64,
    // The most pictures that wait for the app
    depth: usize,
    order: DecodeOrder,
    rotation: VideoRotation,
    config_color: Option<VideoColorInfo>,
    stream_meta: Mutex<StreamMetadata>,
    format_color: Mutex<FormatColor>,
    work_gate: Mutex<()>,
    work_ready: Condvar,
    input_gate: Mutex<()>,
    input_ready: Condvar,
    eos_seen: Mutex<bool>,
    eos_changed: Condvar,
}

impl Shared {
    fn resolve_color(&self, height: u32) -> VideoColorInfo {
        let format = *self.format_color.lock();
        let stream = *self.stream_meta.lock();
        let primaries = format
            .primaries
            .or(stream.primaries)
            .or_else(|| self.config_color.map(|c| c.primaries))
            .unwrap_or_else(|| ColorPrimaries::default_for_height(height));
        let range = format
            .range
            .or(stream.range)
            .or_else(|| self.config_color.map(|c| c.range))
            .unwrap_or(ColorRange::Limited);
        let transfer = format
            .transfer
            .or(stream.transfer)
            .or_else(|| self.config_color.map(|c| c.transfer))
            .unwrap_or(ColorTransfer::Sdr);
        VideoColorInfo {
            primaries,
            range,
            transfer,
        }
    }
}

// The feeding side of a decoder.
pub(super) struct Backend {
    codec: Arc<Codec>,
    shared: Arc<Shared>,
    codec_kind: VideoCodec,
    nals: AccessUnitNals,
    admission: Admission,
    input_wait: Duration,
}

impl Backend {
    pub(super) fn new(config: DecoderConfig) -> Result<Self, DecoderError> {
        let DecoderConfig {
            codec: kind,
            width,
            height,
            output,
            input_wait,
            order,
            rotation,
            color,
        } = config;
        let (reader, depth) = match &output {
            DecoderOutput::Surface(_) => (None, 0),
            DecoderOutput::Frames { depth } => {
                let depth = (*depth).max(2);
                (Some(Arc::new(Reader::new(width, height, depth)?)), depth)
            }
        };
        let window = match (&output, &reader) {
            (DecoderOutput::Surface(surface), _) => surface.window(),
            (_, Some(reader)) => reader.window(),
            (_, None) => return Err(DecoderError::Unavailable),
        };
        let input_limit = (width as usize) * (height as usize);
        let codec = Arc::new(Codec(Mutex::new(configure(
            kind,
            width,
            height,
            input_limit,
            order,
            window,
        )?)));
        let shared = Arc::new(Shared {
            pending: Mutex::new(PendingUnits::new()),
            queue: Arc::new(FrameQueue::new(depth, REPORTS)),
            stop: AtomicBool::new(false),
            flush_epoch: AtomicU64::new(0),
            depth,
            order,
            rotation,
            config_color: color,
            stream_meta: Mutex::new(StreamMetadata::default()),
            format_color: Mutex::new(FormatColor::default()),
            work_gate: Mutex::new(()),
            work_ready: Condvar::new(),
            input_gate: Mutex::new(()),
            input_ready: Condvar::new(),
            eos_seen: Mutex::new(false),
            eos_changed: Condvar::new(),
        });
        let output = Output {
            codec: Arc::clone(&codec),
            shared: Arc::clone(&shared),
            reader,
        };
        thread::Builder::new()
            .name("video-decoder-output".into())
            .spawn(move || output.run())
            .map_err(|error| DecoderError::Failed(format!("the output thread: {error}")))?;
        Ok(Self {
            codec,
            shared,
            codec_kind: kind,
            nals: AccessUnitNals::default(),
            admission: Admission::new(input_limit, order),
            input_wait,
        })
    }

    pub(super) fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        let pts_us = self.admission.check(&unit)?;
        if !matches!(self.codec_kind, VideoCodec::Av1 | VideoCodec::Vp9) {
            self.nals.parse(self.codec_kind, unit.data);
            if let Some(sps) = self.nals.sequence_parameter_set(unit.data)
                && let Some(meta) = StreamMetadata::parse_sps(self.codec_kind, sps)
            {
                *self.shared.stream_meta.lock() = meta;
            }
        }
        let deadline = Instant::now() + self.input_wait;
        loop {
            let result = {
                let codec = self.codec.0.lock();
                match codec
                    .dequeue_input_buffer(Duration::ZERO)
                    .map_err(|error| DecoderError::Failed(error.to_string()))?
                {
                    DequeuedInputBufferResult::Buffer(mut buffer) => {
                        let room = buffer.buffer_mut().len();
                        // Register before queuing so output thread always finds the pending unit.
                        self.shared.pending.lock().push(PendingUnit {
                            pts_us,
                            timestamp: unit.timestamp,
                            metadata: unit.metadata,
                            submitted: Instant::now(),
                        });
                        let size = if unit.data.len() <= room {
                            buffer.buffer_mut()[..unit.data.len()].write_copy_of_slice(unit.data);
                            unit.data.len()
                        } else {
                            0
                        };
                        codec
                            .queue_input_buffer(buffer, 0, size, pts_us, 0)
                            .map_err(|error| DecoderError::Failed(error.to_string()))?;
                        if size < unit.data.len() {
                            Some(Err(DecoderError::AccessUnitTooLarge {
                                size: unit.data.len(),
                                limit: room,
                            }))
                        } else {
                            Some(Ok(()))
                        }
                    }
                    DequeuedInputBufferResult::TryAgainLater => None,
                }
            };
            if let Some(done) = result {
                done?;
                self.admission.accept(pts_us);
                self.shared.queue.stats.submitted();
                self.shared.work_ready.notify_one();
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                self.shared.queue.stats.refused();
                return Err(DecoderError::Full);
            }
            let slice = (deadline - now).min(ACTIVE_WAIT);
            let mut gate = self.shared.input_gate.lock();
            let _: parking_lot::WaitTimeoutResult =
                self.shared.input_ready.wait_for(&mut gate, slice);
        }
    }

    pub(super) fn drain(&mut self) -> Result<(), DecoderError> {
        *self.shared.eos_seen.lock() = false;
        let wait = self.input_wait.max(Duration::from_millis(200));
        let deadline = Instant::now() + wait;
        loop {
            let queued = {
                let codec = self.codec.0.lock();
                match codec
                    .dequeue_input_buffer(Duration::ZERO)
                    .map_err(|error| DecoderError::Failed(error.to_string()))?
                {
                    DequeuedInputBufferResult::Buffer(buffer) => {
                        codec
                            .queue_input_buffer(buffer, 0, 0, 0, BUFFER_FLAG_END_OF_STREAM)
                            .map_err(|error| DecoderError::Failed(error.to_string()))?;
                        true
                    }
                    DequeuedInputBufferResult::TryAgainLater => false,
                }
            };
            if queued {
                self.shared.work_ready.notify_one();
                break;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(DecoderError::Full);
            }
            let slice = (deadline - now).min(ACTIVE_WAIT);
            let mut gate = self.shared.input_gate.lock();
            let _: parking_lot::WaitTimeoutResult =
                self.shared.input_ready.wait_for(&mut gate, slice);
        }

        let eos_deadline = Instant::now() + DRAIN_WAIT;
        let mut eos = self.shared.eos_seen.lock();
        while !*eos {
            if self
                .shared
                .eos_changed
                .wait_until(&mut eos, eos_deadline)
                .timed_out()
            {
                break;
            }
        }
        drop(eos);
        // Reset the codec after EOS while keeping the drained frames in the output queue.
        self.flush_codec(FlushTarget::CodecOnly)
    }

    pub(super) fn flush(&mut self) -> Result<(), DecoderError> {
        self.flush_codec(FlushTarget::CodecAndQueue)
    }

    fn flush_codec(&mut self, target: FlushTarget) -> Result<(), DecoderError> {
        let result = {
            let codec = self.codec.0.lock();
            codec
                .flush()
                .map_err(|error| DecoderError::Failed(error.to_string()))
        };
        self.shared.flush_epoch.fetch_add(1, Ordering::Release);
        self.shared.pending.lock().clear();
        if target == FlushTarget::CodecAndQueue {
            self.shared.queue.frames.lock().clear();
            self.shared.queue.reports.lock().clear();
        }
        self.admission.reset();
        self.shared.input_ready.notify_all();
        self.shared.work_ready.notify_all();
        result
    }

    pub(super) fn frames(&self) -> Frames {
        Frames(Arc::clone(&self.shared.queue))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlushTarget {
    CodecOnly,
    CodecAndQueue,
}

impl Drop for Backend {
    fn drop(&mut self) {
        // The draining thread stops the codec when it sees the flag, and ends. The drop does
        // not wait for it.
        self.shared.stop.store(true, Ordering::Release);
        self.shared.work_ready.notify_all();
        self.shared.input_ready.notify_all();
    }
}

// Makes a started decoder that renders into `window`. A codec that refuses the operating rate
// is made again without it.
fn configure(
    kind: VideoCodec,
    width: u32,
    height: u32,
    input_limit: usize,
    order: DecodeOrder,
    window: &NativeWindow,
) -> Result<MediaCodec, DecoderError> {
    let mut failure = String::new();
    let rates: &[bool] = match order {
        DecodeOrder::LowLatency => &[true, false],
        DecodeOrder::Presentation => &[false],
    };
    for &with_rate in rates {
        let codec =
            MediaCodec::from_decoder_type(kind.mime()).ok_or(DecoderError::NoDecoder(kind))?;
        let mut format = MediaFormat::new();
        format.set_str("mime", kind.mime());
        format.set_i32("width", width as i32);
        format.set_i32("height", height as i32);
        format.set_i32("max-input-size", input_limit as i32);
        let low_latency = i32::from(order == DecodeOrder::LowLatency);
        format.set_i32("low-latency", low_latency);
        format.set_i32("priority", 0);
        if with_rate {
            format.set_i32("operating-rate", OPERATING_RATE);
        }
        let started = codec
            .configure(&format, Some(window), MediaCodecDirection::Decoder)
            .and_then(|()| codec.start());
        match started {
            Ok(()) => return Ok(codec),
            Err(error) => {
                failure = error.to_string();
                log::warn!(
                    "video decoder: configure failed (operating rate {with_rate}): {failure}"
                );
            }
        }
    }
    Err(DecoderError::Configure(failure))
}

// A picture that left the codec for the reader, and waits for its image.
struct Rendered {
    pts_us: u64,
    info: FrameInfo,
    at: Instant,
}

// The draining thread's state.
struct Output {
    codec: Arc<Codec>,
    shared: Arc<Shared>,
    reader: Option<Arc<Reader>>,
}

impl Output {
    fn run(self) {
        let mut released: VecDeque<Rendered> = VecDeque::with_capacity(self.shared.depth + 4);
        let mut seen_epoch = self.shared.flush_epoch.load(Ordering::Acquire);
        while !self.shared.stop.load(Ordering::Acquire) {
            let epoch = self.shared.flush_epoch.load(Ordering::Acquire);
            if epoch != seen_epoch {
                released.clear();
                seen_epoch = epoch;
            }
            let (did_work, hit_eos, fatal) = self.poll_codec_outputs(&mut released);
            if fatal {
                break;
            }
            self.collect(&mut released);
            if did_work && !released.is_empty() {
                self.shared.queue.stats.late();
            }
            if hit_eos {
                self.drain_rendered(&mut released);
                self.signal_eos();
            }
            if !did_work && !self.shared.stop.load(Ordering::Acquire) {
                let busy = !released.is_empty() || self.shared.pending.lock().len() > 0;
                let wait = if busy { ACTIVE_WAIT } else { IDLE_WAIT };
                let mut gate = self.shared.work_gate.lock();
                if !self.shared.stop.load(Ordering::Acquire) {
                    let _: parking_lot::WaitTimeoutResult =
                        self.shared.work_ready.wait_for(&mut gate, wait);
                }
            }
        }
        let codec = self.codec.0.lock();
        if let Err(error) = codec.stop() {
            log::warn!("video decoder: stop: {error}");
        }
    }

    fn poll_codec_outputs(&self, released: &mut VecDeque<Rendered>) -> (bool, bool, bool) {
        let mut did_work = false;
        let mut hit_eos = false;
        let codec = self.codec.0.lock();
        for _ in 0..8 {
            match codec.dequeue_output_buffer(Duration::ZERO) {
                Ok(DequeuedOutputBufferInfoResult::Buffer(buffer)) => {
                    did_work = true;
                    if self.release(&codec, buffer, released) {
                        hit_eos = true;
                        break;
                    }
                }
                Ok(DequeuedOutputBufferInfoResult::OutputFormatChanged) => {
                    did_work = true;
                    self.read_output_format(&codec);
                }
                Ok(DequeuedOutputBufferInfoResult::TryAgainLater) => break,
                Ok(_) => {}
                Err(error) => {
                    log::error!("video decoder: output: {error}");
                    return (did_work, hit_eos, true);
                }
            }
        }
        (did_work, hit_eos, false)
    }

    fn read_output_format(&self, codec: &MediaCodec) {
        let format = codec.output_format();
        let primaries = match format.i32("color-standard") {
            Some(1) => Some(ColorPrimaries::Bt709),
            Some(2 | 4) => Some(ColorPrimaries::Bt601),
            Some(6) => Some(ColorPrimaries::Bt2020),
            _ => None,
        };
        let range = match format.i32("color-range") {
            Some(1) => Some(ColorRange::Full),
            Some(2) => Some(ColorRange::Limited),
            _ => None,
        };
        let transfer = match format.i32("color-transfer") {
            Some(1 | 3) => Some(ColorTransfer::Sdr),
            Some(6) => Some(ColorTransfer::Pq),
            Some(7) => Some(ColorTransfer::Hlg),
            _ => None,
        };
        *self.shared.format_color.lock() = FormatColor {
            primaries,
            range,
            transfer,
        };
    }

    // Renders the picture of `buffer` and records it. Returns `true` when EOS was reached.
    fn release(
        &self,
        codec: &MediaCodec,
        buffer: OutputBuffer<'_>,
        released: &mut VecDeque<Rendered>,
    ) -> bool {
        let flags = buffer.info().flags();
        let eos = (flags & BUFFER_FLAG_END_OF_STREAM) != 0;
        let has_bytes = buffer.info().size() > 0;
        let pts_us = u64::try_from(buffer.info().presentation_time_us()).unwrap_or(0);
        if !has_bytes && eos {
            let _: ndk::media_error::Result<()> = codec.release_output_buffer(buffer, false);
            self.shared.input_ready.notify_one();
            return true;
        }
        let unit = {
            let mut pending = self.shared.pending.lock();
            match self.shared.order {
                DecodeOrder::LowLatency => pending.take(pts_us),
                DecodeOrder::Presentation => pending.remove(pts_us),
            }
        };
        let info = PendingUnit::into_frame_info(unit, pts_us);
        if let Err(error) = codec.release_output_buffer(buffer, true) {
            log::error!("video decoder: render: {error}");
            self.shared.queue.stats.dropped();
            self.shared.input_ready.notify_one();
            return eos;
        }
        self.shared.input_ready.notify_one();
        self.shared.queue.stats.decoded(info.decode_time);
        if self.reader.is_none() {
            let mut reports = self.shared.queue.reports.lock();
            if reports.len() == REPORTS {
                reports.pop_front();
            }
            reports.push_back(info);
            return eos;
        }
        if released.len() == released.capacity() {
            released.pop_front();
            self.shared.queue.stats.dropped();
        }
        released.push_back(Rendered {
            pts_us,
            info,
            at: Instant::now(),
        });
        eos
    }

    fn drain_rendered(&self, released: &mut VecDeque<Rendered>) {
        let deadline = Instant::now() + IMAGE_LIMIT;
        while !released.is_empty()
            && !self.shared.stop.load(Ordering::Acquire)
            && Instant::now() < deadline
        {
            self.collect(released);
            if !released.is_empty() {
                let mut gate = self.shared.work_gate.lock();
                if !self.shared.stop.load(Ordering::Acquire) {
                    let _: parking_lot::WaitTimeoutResult =
                        self.shared.work_ready.wait_for(&mut gate, ACTIVE_WAIT);
                }
            }
        }
    }

    fn signal_eos(&self) {
        *self.shared.eos_seen.lock() = true;
        self.shared.eos_changed.notify_all();
    }

    // Takes the images that the decoder rendered into the reader, and queues them for the app
    // with their reports.
    fn collect(&self, released: &mut VecDeque<Rendered>) {
        let Some(reader) = &self.reader else {
            return;
        };
        loop {
            match reader.acquire() {
                Acquired::Image(image) => {
                    let info = take_info(released, image.timestamp_ns());
                    let size = image.size();
                    let crop = image.crop();
                    let color = self.shared.resolve_color(crop.height.max(size.1));
                    let frame =
                        VideoFrame::new(info, size, crop, color, self.shared.rotation, image);
                    let mut frames = self.shared.queue.frames.lock();
                    if frames.len() >= self.shared.depth {
                        frames.pop_front();
                        self.shared.queue.stats.dropped();
                    }
                    frames.push_back(frame);
                    drop(frames);
                    self.shared.queue.wake();
                }
                Acquired::Empty => break,
                Acquired::Full => {
                    // The app and the queue hold every image. The oldest queued picture goes, so
                    // the newest comes in.
                    let mut frames = self.shared.queue.frames.lock();
                    if frames.pop_front().is_none() {
                        break;
                    }
                    self.shared.queue.stats.dropped();
                }
                Acquired::Failed(status) => {
                    log::error!("video decoder: image reader status {status}");
                    break;
                }
            }
        }
        // The reader lost the image of a picture that waits this long
        while released
            .front()
            .is_some_and(|rendered| rendered.at.elapsed() > IMAGE_LIMIT)
        {
            released.pop_front();
            self.shared.queue.stats.dropped();
        }
    }
}

// The report of the picture whose image has `timestamp_ns`. The codec's presentation time in
// microseconds is the image's time in nanoseconds divided by a thousand. An image whose time
// matches no report takes the oldest report.
fn take_info(released: &mut VecDeque<Rendered>, timestamp_ns: i64) -> FrameInfo {
    let pts_us = u64::try_from(timestamp_ns / 1000).unwrap_or(0);
    let index = released
        .iter()
        .position(|rendered| rendered.pts_us == pts_us)
        .unwrap_or(0);
    released
        .remove(index)
        .map_or_else(|| orphan(pts_us), |rendered| rendered.info)
}

// The report of a picture that no submitted unit claims.
fn orphan(pts_us: u64) -> FrameInfo {
    FrameInfo {
        timestamp: Duration::from_micros(pts_us),
        metadata: 0,
        decode_time: Duration::ZERO,
    }
}
