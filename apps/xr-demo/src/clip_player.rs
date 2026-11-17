// Plays a short H.264 clip through Cranpose's hardware decoder from its own thread, in a loop.
// The clip has no B-frames, an access unit delimiter before each picture, and the parameter sets
// in front of each keyframe. Two decoders take the pictures: one sends them to a surface, and one
// returns them as frames for a streamed scene.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use cranpose::{
    AccessUnit, DecodedFrames, DecoderConfig, DecoderError, DecoderOutput, ExternalSurface,
    VideoCodec, VideoDecoder,
};

const CLIP: &[u8] = include_bytes!("../assets/test_clip.h264");
// The clip's size in pixels and its rate in pictures a second
const SIZE: (u32, u32) = (640, 360);
const RATE: u32 = 30;
// The pictures that the frames decoder may have in flight. A streamed scene holds up to three
// waiting frames and the frame shown last, and the decoder needs one more to write, and one more
// to keep its output moving.
pub(crate) const DEPTH: usize = 6;
const REPORT_EVERY: Duration = Duration::from_secs(1);

// A player of the clip. It stops when dropped. The thread ends at its next picture.
pub(crate) struct ClipPlayer {
    stop: Arc<AtomicBool>,
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
        let surface_decoder = decoder(DecoderOutput::Surface(surface))?;
        let frames_decoder = decoder(DecoderOutput::Frames { depth: DEPTH })?;
        let frames = frames_decoder.frames();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let reports = surface_decoder.frames();
        let stats = frames.clone();
        let spawned = thread::Builder::new()
            .name("clip-player".into())
            .spawn(move || play([surface_decoder, frames_decoder], &reports, &stats, &flag));
        if let Err(error) = spawned {
            log::error!("xr demo: no clip thread: {error}");
        }
        Ok((Self { stop }, frames))
    }
}

impl Drop for ClipPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn decoder(output: DecoderOutput) -> Result<VideoDecoder, DecoderError> {
    VideoDecoder::new(DecoderConfig::new(VideoCodec::H264, SIZE.0, SIZE.1, output))
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
    mut decoders: [VideoDecoder; 2],
    reports: &DecodedFrames,
    frames: &DecodedFrames,
    stop: &AtomicBool,
) {
    let units = access_units(CLIP);
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
        let mean = self.decode / self.reports.max(1);
        let surface = reports.stats();
        let images = frames.stats();
        log::info!(
            "xr demo: surface decoder {} pictures, decode {:.2} ms mean {:.2} ms longest, \
             {} wrong metadata, {} submitted, {} dropped | frames decoder {} decoded, {} late, \
             decode {:.2} ms mean, {} dropped",
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

// The access units of an Annex-B stream. Each begins at an access unit delimiter, which is the
// NAL unit of type 9.
pub(crate) fn access_units(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut at = 0;
    while at + 3 < stream.len() {
        if stream[at..at + 3] == [0, 0, 1] {
            if stream[at + 3] & 0x1f == 9 {
                // A start code of four bytes begins with a zero byte that belongs to it
                starts.push(if at > 0 && stream[at - 1] == 0 {
                    at - 1
                } else {
                    at
                });
            }
            at += 3;
        } else {
            at += 1;
        }
    }
    let ends = starts.iter().skip(1).copied().chain([stream.len()]);
    starts
        .iter()
        .zip(ends)
        .map(|(start, end)| &stream[*start..end])
        .collect()
}
