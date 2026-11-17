//! The Android decoder: an `AMediaCodec` fed by the app's thread and drained by a thread of its
//! own.
//!
//! Neither thread sleeps. The feeding thread waits in the codec for a free input buffer, and the
//! draining thread waits in the codec for an output. Each wakes when the codec has something,
//! and the draining thread also wakes at an interval to see that the decoder stopped.
#![expect(
    unsafe_code,
    reason = "AMediaCodec is a thread-safe C object that the ndk crate does not mark Send"
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
use parking_lot::Mutex;

use super::{
    AccessUnit, DecoderConfig, DecoderError, DecoderOutput, DecoderStats, FrameInfo, VideoCodec,
    VideoFrame,
    image_reader::{Acquired, Reader},
    pending_units::{PendingUnit, PendingUnits},
    stats::StatsCounters,
};

// How long the draining thread waits for an output before it checks that the decoder is still
// wanted. The wait ends at once when an output comes.
const OUTPUT_WAIT: Duration = Duration::from_millis(20);

// The reports of the pictures sent to a surface that the decoder keeps for the app.
const REPORTS: usize = 32;

// The operating rate that asks the codec for its top speed, in frames a second.
const OPERATING_RATE: i32 = i16::MAX as i32;

// An AMediaCodec that two threads use: one dequeues its inputs, and the other its outputs.
struct Codec(MediaCodec);

// SAFETY: AMediaCodec's calls are thread-safe. The decoder lets one thread wait for an input
// and another thread wait for an output.
unsafe impl Send for Codec {}
// SAFETY: as for Send
unsafe impl Sync for Codec {}

// What the feeding thread, the draining thread and the app's handles share.
struct Shared {
    pending: Mutex<PendingUnits>,
    frames: Mutex<VecDeque<VideoFrame>>,
    reports: Mutex<VecDeque<FrameInfo>>,
    stats: StatsCounters,
    stop: AtomicBool,
    // The most pictures that wait for the app
    depth: usize,
}

// The feeding side of a decoder.
pub(super) struct Backend {
    codec: Arc<Codec>,
    shared: Arc<Shared>,
    last_pts_us: Option<u64>,
    input_wait: Duration,
    // The largest access unit that the codec's input takes
    input_limit: usize,
}

// The receiving side of a decoder.
#[derive(Clone)]
pub(super) struct Frames(Arc<Shared>);

impl PartialEq for Frames {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Backend {
    pub(super) fn new(config: DecoderConfig) -> Result<Self, DecoderError> {
        let DecoderConfig {
            codec: kind,
            width,
            height,
            output,
            input_wait,
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
        let codec = Arc::new(Codec(configure(kind, width, height, input_limit, window)?));
        let shared = Arc::new(Shared {
            pending: Mutex::new(PendingUnits::new()),
            frames: Mutex::new(VecDeque::with_capacity(depth)),
            reports: Mutex::new(VecDeque::with_capacity(REPORTS)),
            stats: StatsCounters::default(),
            stop: AtomicBool::new(false),
            depth,
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
            last_pts_us: None,
            input_wait,
            input_limit,
        })
    }

    pub(super) fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        let pts_us = u64::try_from(unit.timestamp.as_micros()).unwrap_or(u64::MAX);
        if self.last_pts_us.is_some_and(|last| pts_us <= last) {
            return Err(DecoderError::TimestampNotRising);
        }
        if unit.data.len() > self.input_limit {
            return Err(DecoderError::AccessUnitTooLarge {
                size: unit.data.len(),
                limit: self.input_limit,
            });
        }
        let mut buffer = match self
            .codec
            .0
            .dequeue_input_buffer(self.input_wait)
            .map_err(|error| DecoderError::Failed(error.to_string()))?
        {
            DequeuedInputBufferResult::Buffer(buffer) => buffer,
            DequeuedInputBufferResult::TryAgainLater => return Err(DecoderError::Full),
        };
        let room = buffer.buffer_mut().len();
        // The unit is registered before the codec can output its picture
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
        self.codec
            .0
            .queue_input_buffer(buffer, 0, size, pts_us, 0)
            .map_err(|error| DecoderError::Failed(error.to_string()))?;
        if size < unit.data.len() {
            return Err(DecoderError::AccessUnitTooLarge {
                size: unit.data.len(),
                limit: room,
            });
        }
        self.last_pts_us = Some(pts_us);
        self.shared.stats.submitted();
        Ok(())
    }

    pub(super) fn frames(&self) -> Frames {
        Frames(Arc::clone(&self.shared))
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // The draining thread stops the codec when it sees the flag, and ends. The drop does
        // not wait for it.
        self.shared.stop.store(true, Ordering::Release);
    }
}

impl Frames {
    pub(super) fn next_frame(&self) -> Option<VideoFrame> {
        self.0.frames.lock().pop_front()
    }

    pub(super) fn latest_frame(&self) -> Option<VideoFrame> {
        let mut frames = self.0.frames.lock();
        while frames.len() > 1 {
            frames.pop_front();
            self.0.stats.dropped();
        }
        frames.pop_front()
    }

    pub(super) fn next_report(&self) -> Option<FrameInfo> {
        self.0.reports.lock().pop_front()
    }

    pub(super) fn stats(&self) -> DecoderStats {
        self.0.stats.snapshot()
    }
}

// Makes a started decoder that renders into `window`. A codec that refuses the operating rate
// is made again without it.
fn configure(
    kind: VideoCodec,
    width: u32,
    height: u32,
    input_limit: usize,
    window: &NativeWindow,
) -> Result<MediaCodec, DecoderError> {
    let mut failure = String::new();
    for with_rate in [true, false] {
        let codec =
            MediaCodec::from_decoder_type(kind.mime()).ok_or(DecoderError::NoDecoder(kind))?;
        let mut format = MediaFormat::new();
        format.set_str("mime", kind.mime());
        format.set_i32("width", width as i32);
        format.set_i32("height", height as i32);
        format.set_i32("max-input-size", input_limit as i32);
        // The decoder outputs each picture at once and does not wait to reorder
        format.set_i32("low-latency", 1);
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

// The draining thread's state.
struct Output {
    codec: Arc<Codec>,
    shared: Arc<Shared>,
    reader: Option<Arc<Reader>>,
}

impl Output {
    fn run(self) {
        // The pictures that left the codec and wait for their images, with their reports
        let mut released: VecDeque<(u64, FrameInfo)> =
            VecDeque::with_capacity(self.shared.depth + 4);
        while !self.shared.stop.load(Ordering::Acquire) {
            match self.codec.0.dequeue_output_buffer(OUTPUT_WAIT) {
                Ok(DequeuedOutputBufferInfoResult::Buffer(buffer)) => {
                    self.release(buffer, &mut released);
                }
                Ok(DequeuedOutputBufferInfoResult::TryAgainLater) => self.collect(&mut released),
                Ok(_) => {}
                Err(error) => {
                    log::error!("video decoder: output: {error}");
                    break;
                }
            }
        }
        // The codec stops before the reader goes
        if let Err(error) = self.codec.0.stop() {
            log::warn!("video decoder: stop: {error}");
        }
    }

    // Renders the picture of `buffer` and records it.
    fn release(&self, buffer: OutputBuffer<'_>, released: &mut VecDeque<(u64, FrameInfo)>) {
        let pts_us = u64::try_from(buffer.info().presentation_time_us()).unwrap_or(0);
        let unit = self.shared.pending.lock().take(pts_us);
        let info = unit.map_or_else(
            || orphan(pts_us),
            |unit| FrameInfo {
                timestamp: unit.timestamp,
                metadata: unit.metadata,
                decode_time: unit.submitted.elapsed(),
            },
        );
        if let Err(error) = self.codec.0.release_output_buffer(buffer, true) {
            log::error!("video decoder: render: {error}");
            return;
        }
        self.shared.stats.decoded(info.decode_time);
        if self.reader.is_none() {
            let mut reports = self.shared.reports.lock();
            if reports.len() == REPORTS {
                reports.pop_front();
            }
            reports.push_back(info);
            return;
        }
        if released.len() == released.capacity() {
            released.pop_front();
        }
        released.push_back((pts_us, info));
        self.collect(released);
        // A picture whose image has not come yet reaches the app one picture late
        if released.iter().any(|(pts, _)| *pts == pts_us) {
            self.shared.stats.late();
        }
    }

    // Takes the images that the decoder rendered into the reader, and queues them for the app
    // with their reports.
    fn collect(&self, released: &mut VecDeque<(u64, FrameInfo)>) {
        let Some(reader) = &self.reader else {
            return;
        };
        loop {
            match reader.acquire() {
                Acquired::Image(image) => {
                    let info = take_info(released, image.timestamp_ns());
                    let frame = VideoFrame::new(info, image.size(), image.crop(), image);
                    let mut frames = self.shared.frames.lock();
                    if frames.len() >= self.shared.depth {
                        frames.pop_front();
                        self.shared.stats.dropped();
                    }
                    frames.push_back(frame);
                }
                Acquired::Empty => break,
                Acquired::Full => {
                    // The app and the queue hold every image. The oldest queued picture goes, so
                    // the newest comes in.
                    let mut frames = self.shared.frames.lock();
                    if frames.pop_front().is_none() {
                        break;
                    }
                    self.shared.stats.dropped();
                }
                Acquired::Failed(status) => {
                    log::error!("video decoder: image reader status {status}");
                    break;
                }
            }
        }
    }
}

// The report of the picture whose image has `timestamp_ns`. The codec's presentation time in
// microseconds is the image's time in nanoseconds divided by a thousand. An image whose time
// matches no report takes the oldest report.
fn take_info(released: &mut VecDeque<(u64, FrameInfo)>, timestamp_ns: i64) -> FrameInfo {
    let pts_us = u64::try_from(timestamp_ns / 1000).unwrap_or(0);
    let index = released
        .iter()
        .position(|(pts, _)| *pts == pts_us)
        .unwrap_or(0);
    let (_, info) = released
        .remove(index)
        .unwrap_or_else(|| (pts_us, orphan(pts_us)));
    info
}

// The report of a picture that no submitted unit claims.
fn orphan(pts_us: u64) -> FrameInfo {
    FrameInfo {
        timestamp: Duration::from_micros(pts_us),
        metadata: 0,
        decode_time: Duration::ZERO,
    }
}
