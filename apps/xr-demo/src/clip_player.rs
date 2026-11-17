// Plays a short H.264 clip through Cranpose's hardware decoder from its own thread, in a loop.
// The clip has no B-frames, an access unit delimiter before each picture, and the parameter sets
// in front of each keyframe. A second decoder in frames mode takes the same pictures and is made
// anew at every loop, as a self-test of its start, its images and its end.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use cranpose::{
    AccessUnit, CropRect, DecodedFrames, DecoderConfig, DecoderError, DecoderOutput,
    ExternalSurface, VideoCodec, VideoDecoder,
};

const CLIP: &[u8] = include_bytes!("../assets/test_clip.h264");
// The clip's size in pixels and its rate in pictures a second
const SIZE: (u32, u32) = (640, 360);
const RATE: u32 = 30;
// The pictures that the frames-mode decoder keeps
const DEPTH: usize = 4;
const REPORT_EVERY: Duration = Duration::from_secs(1);

// A player of the clip. It stops when dropped. The thread ends at its next picture.
pub(crate) struct ClipPlayer {
    stop: Arc<AtomicBool>,
}

impl ClipPlayer {
    pub(crate) fn start(surface: ExternalSurface) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("clip-player".into())
            .spawn(move || play(surface, &flag));
        if let Err(error) = spawned {
            log::error!("xr demo: no clip thread: {error}");
        }
        Self { stop }
    }
}

impl Drop for ClipPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// What the player counts in one report interval.
#[derive(Default)]
struct Interval {
    reports: u32,
    wrong_metadata: u32,
    decode: Duration,
    longest: Duration,
    frames: u32,
    frame_buffers_missing: u32,
    wrong_frame_metadata: u32,
    // The buffer size and the crop of the latest picture
    buffer: Option<((u32, u32), CropRect)>,
}

fn play(surface: ExternalSurface, stop: &AtomicBool) {
    let config = DecoderConfig::new(
        VideoCodec::H264,
        SIZE.0,
        SIZE.1,
        DecoderOutput::Surface(surface),
    );
    let mut decoder = match VideoDecoder::new(config) {
        Ok(decoder) => decoder,
        Err(error) => {
            log::error!("xr demo: no decoder for the clip: {error}");
            return;
        }
    };
    let reports = decoder.frames();
    let units = access_units(CLIP);
    let step = Duration::from_secs(1) / RATE;
    let started = Instant::now();
    let mut index = 0_u64;
    let mut reported = Instant::now();
    let mut interval = Interval::default();
    while !stop.load(Ordering::Relaxed) {
        // A new decoder for the frames output at every loop
        let (mut frames_decoder, frames) = match frames_decoder() {
            Ok(pair) => pair,
            Err(error) => {
                log::error!("xr demo: no frames decoder: {error}");
                return;
            }
        };
        for unit in &units {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let access_unit = AccessUnit {
                data: unit,
                timestamp: step * u32::try_from(index).unwrap_or(u32::MAX),
                metadata: index,
            };
            for decoder in [&mut decoder, &mut frames_decoder] {
                if let Err(error) = decoder.submit(access_unit) {
                    log::warn!("xr demo: picture {index} dropped: {error}");
                }
            }
            index += 1;
            interval.take_reports(&reports, step);
            interval.take_frames(&frames, step);
            if reported.elapsed() >= REPORT_EVERY {
                interval.log(&reports, &frames);
                interval = Interval::default();
                reported = Instant::now();
            }
            // The app paces the pictures at the clip's rate
            let due = started + step * u32::try_from(index).unwrap_or(u32::MAX);
            thread::sleep(due.saturating_duration_since(Instant::now()));
        }
    }
}

fn frames_decoder() -> Result<(VideoDecoder, DecodedFrames), DecoderError> {
    let config = DecoderConfig::new(
        VideoCodec::H264,
        SIZE.0,
        SIZE.1,
        DecoderOutput::Frames { depth: DEPTH },
    );
    let decoder = VideoDecoder::new(config)?;
    let frames = decoder.frames();
    Ok((decoder, frames))
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

    // Reads the frames decoder's pictures and drops them at once.
    fn take_frames(&mut self, frames: &DecodedFrames, step: Duration) {
        while let Some(frame) = frames.next_frame() {
            self.frames += 1;
            if frame.as_hardware_buffer_ptr().is_null() {
                self.frame_buffers_missing += 1;
            }
            let info = frame.info();
            if step * u32::try_from(info.metadata).unwrap_or(u32::MAX) != info.timestamp {
                self.wrong_frame_metadata += 1;
            }
            self.buffer = Some((frame.size(), frame.crop()));
        }
    }

    fn log(&self, reports: &DecodedFrames, frames: &DecodedFrames) {
        let mean = self.decode / self.reports.max(1);
        let surface = reports.stats();
        let images = frames.stats();
        log::info!(
            "xr demo: surface decoder {} pictures, decode {:.2} ms mean {:.2} ms longest, \
             {} wrong metadata, {} submitted, {} dropped | frames decoder {} pictures, \
             {} without a buffer, {} wrong metadata, {} late, decode {:.2} ms mean, {} dropped, \
             buffer and crop {:?}",
            self.reports,
            mean.as_secs_f64() * 1000.0,
            self.longest.as_secs_f64() * 1000.0,
            self.wrong_metadata,
            surface.submitted,
            surface.dropped,
            self.frames,
            self.frame_buffers_missing,
            self.wrong_frame_metadata,
            images.late,
            images.mean_decode.as_secs_f64() * 1000.0,
            images.dropped,
            self.buffer,
        );
    }
}

// The access units of an Annex-B stream. Each begins at an access unit delimiter, which is the
// NAL unit of type 9.
fn access_units(stream: &[u8]) -> Vec<&[u8]> {
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
