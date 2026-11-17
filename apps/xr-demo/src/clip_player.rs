// Plays a short clip through Cranpose's hardware decoder from its own thread, in a loop. The
// clips have no B-frames, an access unit delimiter before each picture, and the parameter sets in
// front of each keyframe. In a headset two decoders take the H.264 pictures: one sends them to a
// surface, and one returns them as frames for a streamed scene. Elsewhere one decoder returns
// the frames of an H.264 or an HEVC clip for a player surface.

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
    VideoCodec, VideoDecoder, split_access_units,
};

const H264_CLIP: &[u8] = include_bytes!("../assets/test_clip.h264");
const HEVC_CLIP: &[u8] = include_bytes!("../assets/test_clip.h265");
// The clips' size in pixels
const SIZE: (u32, u32) = (640, 360);
// The rate of the headset's clip in pictures a second
const RATE: u32 = 30;
// The rate of the flat players, which is the clip's own
#[cfg(not(target_os = "android"))]
const FLAT_RATE: u32 = 60;
// The pictures that the frames decoder may have in flight: the stream holds one, the decoder
// queues some, and the rest are free for the decoder to write.
const DEPTH: usize = 4;
// The pictures that a flat player may have in flight: the player surface keeps each picture until
// the GPU has read it, and the GPU may hold three frames
#[cfg(not(target_os = "android"))]
const FLAT_DEPTH: usize = 6;
const REPORT_EVERY: Duration = Duration::from_secs(1);

// A player of the clip. It stops when dropped. The thread ends at its next picture.
pub(crate) struct ClipPlayer {
    stop: Arc<AtomicBool>,
}

impl ClipPlayer {
    // Makes both decoders and starts the thread that feeds them. Returns the player and the
    // frames of the second decoder.
    pub(crate) fn start(surface: ExternalSurface) -> Result<(Self, DecodedFrames), DecoderError> {
        let surface_decoder = decoder(VideoCodec::H264, DecoderOutput::Surface(surface))?;
        let frames_decoder = decoder(VideoCodec::H264, DecoderOutput::Frames { depth: DEPTH })?;
        let frames = frames_decoder.frames();
        let reports = Some(surface_decoder.frames());
        Ok((
            Self::spawn(
                Clip::new(VideoCodec::H264, RATE),
                vec![surface_decoder, frames_decoder],
                reports,
                frames.clone(),
            ),
            frames,
        ))
    }

    // Makes one decoder that returns frames, and starts the thread that feeds it a clip of
    // `codec`. Returns the player and the frames.
    #[cfg(not(target_os = "android"))]
    pub(crate) fn start_flat(codec: VideoCodec) -> Result<(Self, DecodedFrames), DecoderError> {
        let frames_decoder = decoder(codec, DecoderOutput::Frames { depth: FLAT_DEPTH })?;
        let frames = frames_decoder.frames();
        Ok((
            Self::spawn(
                Clip::new(codec, FLAT_RATE),
                vec![frames_decoder],
                None,
                frames.clone(),
            ),
            frames,
        ))
    }

    fn spawn(
        clip: Clip,
        decoders: Vec<VideoDecoder>,
        reports: Option<DecodedFrames>,
        stats: DecodedFrames,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("clip-player".into())
            .spawn(move || play(&clip, decoders, reports.as_ref(), &stats, &flag));
        if let Err(error) = spawned {
            log::error!("xr demo: no clip thread: {error}");
        }
        Self { stop }
    }
}

// A clip and the rate that the player feeds it at.
struct Clip {
    codec: VideoCodec,
    bytes: &'static [u8],
    rate: u32,
}

impl Clip {
    fn new(codec: VideoCodec, rate: u32) -> Self {
        let bytes = match codec {
            VideoCodec::Hevc => HEVC_CLIP,
            VideoCodec::H264 | VideoCodec::Av1 => H264_CLIP,
        };
        Self { codec, bytes, rate }
    }
}

impl Drop for ClipPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
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
    clip: &Clip,
    mut decoders: Vec<VideoDecoder>,
    reports: Option<&DecodedFrames>,
    frames: &DecodedFrames,
    stop: &AtomicBool,
) {
    let units = split_access_units(clip.codec, clip.bytes);
    let step = Duration::from_secs(1) / clip.rate;
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
            if let Some(reports) = reports {
                interval.take_reports(reports, step);
            }
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

    fn log(&self, reports: Option<&DecodedFrames>, frames: &DecodedFrames) {
        let images = frames.stats();
        if let Some(reports) = reports {
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
        } else {
            log::info!(
                "xr demo: frames decoder {} decoded, decode {:.2} ms mean {:.2} ms longest, \
                 {} dropped",
                images.decoded,
                images.mean_decode.as_secs_f64() * 1000.0,
                images.max_decode.as_secs_f64() * 1000.0,
                images.dropped,
            );
        }
    }
}
