// Plays a short clip through Cranpose's hardware decoder from its own thread, in a loop. The
// clips have no B-frames, an access unit delimiter before each picture, and the parameter sets in
// front of each keyframe. Two decoders take the H.264 pictures: one sends them to a surface, and
// one returns them as frames for a streamed scene.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use cranpose::{
    AccessUnit, DecodedFrames, DecoderConfig, DecoderError, DecoderOutput, ExternalSurface,
    VideoCodec, VideoDecoder, split_access_units,
};
use player_demo::FeedThread;

const H264_CLIP: &[u8] = include_bytes!("../assets/test_clip.h264");
// The clips' size in pixels
const SIZE: (u32, u32) = (640, 360);
// The rate of the headset's clip in pictures a second
const RATE: u32 = 30;
// The pictures that the frames decoder may have in flight. A streamed scene holds up to three
// waiting frames and the frame shown last, and the decoder needs one more to write, and one more
// to keep its output moving.
pub(crate) const DEPTH: usize = 6;
const REPORT_EVERY: Duration = Duration::from_secs(1);

// A player of the clip. It stops when dropped. The thread ends at its next picture.
pub(crate) struct ClipPlayer {
    _thread: FeedThread,
}

// The time at which the frame with metadata `index` should reach the display, for a stream that
// began at `base` on the headset's clock: the clip's rate times the index, and the delay that a
// host would estimate for its encoder, its network and the display.
pub(crate) fn target_display_time(base: Duration, index: u64) -> Duration {
    const DELAY: Duration = Duration::from_millis(20);
    base + Duration::from_secs(1) * u32::try_from(index).unwrap_or(u32::MAX) / RATE + DELAY
}

impl ClipPlayer {
    // Makes both decoders and starts the thread that feeds them. Returns the player and the
    // frames of the second decoder.
    pub(crate) fn start(surface: ExternalSurface) -> Result<(Self, DecodedFrames), DecoderError> {
        let surface_decoder = decoder(VideoCodec::H264, DecoderOutput::Surface(surface))?;
        let frames_decoder = decoder(VideoCodec::H264, DecoderOutput::Frames { depth: DEPTH })?;
        let frames = frames_decoder.frames();
        let reports = surface_decoder.frames();
        Ok((
            Self::spawn(
                vec![surface_decoder, frames_decoder],
                reports,
                frames.clone(),
            ),
            frames,
        ))
    }

    fn spawn(decoders: Vec<VideoDecoder>, reports: DecodedFrames, stats: DecodedFrames) -> Self {
        Self {
            _thread: FeedThread::spawn("clip-player", move |stop| {
                play(decoders, &reports, &stats, stop);
            }),
        }
    }
}

fn decoder(codec: VideoCodec, output: DecoderOutput) -> Result<VideoDecoder, DecoderError> {
    VideoDecoder::new(DecoderConfig::new(codec, SIZE.0, SIZE.1, output))
}

// What the player counts in one report interval.
#[derive(Default)]
struct Interval {
    reports: u32,
    wrong_metadata: u32,
    decode: Duration,
    longest: Duration,
}

fn play(
    mut decoders: Vec<VideoDecoder>,
    reports: &DecodedFrames,
    frames: &DecodedFrames,
    stop: &AtomicBool,
) {
    let units = split_access_units(VideoCodec::H264, H264_CLIP);
    let step = Duration::from_secs(1) / RATE;
    let started = Instant::now();
    let mut index = 0_u64;
    let mut reported = Instant::now();
    let mut interval = Interval::default();
    while !stop.load(Ordering::Relaxed) {
        for unit in &units {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let access_unit = AccessUnit {
                data: unit,
                timestamp: step * u32::try_from(index).unwrap_or(u32::MAX),
                metadata: index,
            };
            for decoder in &mut decoders {
                if let Err(error) = decoder.submit(access_unit) {
                    log::warn!("xr demo: picture {index} dropped: {error}");
                }
            }
            index += 1;
            interval.take_reports(reports, step);
            if reported.elapsed() >= REPORT_EVERY {
                interval.log(reports, frames);
                interval = Interval::default();
                reported = Instant::now();
            }
            // The app paces the pictures at the clip's rate
            let due = started + step * u32::try_from(index).unwrap_or(u32::MAX);
            #[expect(
                clippy::disallowed_methods,
                reason = "the clip's own thread paces its pictures; no frame loop waits on it"
            )]
            thread::sleep(due.saturating_duration_since(Instant::now()));
        }
    }
}

impl Interval {
    // Reads the surface decoder's reports. The metadata of a picture is its index, and its
    // timestamp is the index times the step.
    fn take_reports(&mut self, reports: &DecodedFrames, step: Duration) {
        while let Some(report) = reports.next_report() {
            self.reports += 1;
            self.decode += report.decode_time;
            self.longest = self.longest.max(report.decode_time);
            if step * u32::try_from(report.metadata).unwrap_or(u32::MAX) != report.timestamp {
                self.wrong_metadata += 1;
            }
        }
    }

    fn log(&self, reports: &DecodedFrames, frames: &DecodedFrames) {
        let images = frames.stats();
        let mean = self.decode / self.reports.max(1);
        let surface = reports.stats();
        log::info!(
            "xr demo: surface decoder {} pictures, decode {:.2} ms mean {:.2} ms longest, \
             {} wrong metadata, {} submitted, {} dropped | frames decoder {} decoded, \
             {} late, decode {:.2} ms mean, {} dropped",
            self.reports,
            mean.as_secs_f64() * 1000.0,
            self.longest.as_secs_f64() * 1000.0,
            self.wrong_metadata,
            surface.submitted,
            surface.dropped,
            images.decoded,
            images.late,
            images.mean_decode.as_secs_f64() * 1000.0,
            images.dropped,
        );
    }
}
