// Plays a short clip through Cranpose's hardware decoder, in a loop. The clips have no B-frames,
// an access unit delimiter before each picture, and the parameter sets in front of each keyframe.
// The decoder returns the pictures as frames, which a player surface draws.

use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Instant,
};

#[cfg(target_arch = "wasm32")]
use cranpose::prelude::delay;
use cranpose::{
    AccessUnit, DecodedFrames, DecoderConfig, DecoderError, DecoderOutput, VideoCodec,
    VideoDecoder, split_access_units,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::feed_thread::FeedThread;

const H264_CLIP: &[u8] = include_bytes!("../../xr-demo/assets/test_clip.h264");
const HEVC_CLIP: &[u8] = include_bytes!("../../xr-demo/assets/test_clip.h265");
// The clips' size in pixels
const SIZE: (u32, u32) = (640, 360);
// The pictures that the decoder may have in flight: the player surface keeps each picture until
// the GPU has read it, and the GPU may hold three frames
const DEPTH: usize = 8;
#[cfg(not(target_arch = "wasm32"))]
const REPORT_EVERY: Duration = Duration::from_secs(1);

fn clip_bytes(codec: VideoCodec) -> &'static [u8] {
    match codec {
        VideoCodec::Hevc => HEVC_CLIP,
        VideoCodec::H264 | VideoCodec::Av1 => H264_CLIP,
    }
}

fn open_decoder(codec: VideoCodec) -> Result<VideoDecoder, DecoderError> {
    VideoDecoder::new(DecoderConfig::new(
        codec,
        SIZE.0,
        SIZE.1,
        DecoderOutput::Frames { depth: DEPTH },
    ))
}

// Makes a decoder for `codec` and starts the thread that feeds it the clip at `rate` pictures a
// second. Returns the feed and the decoder's frames.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn start(
    codec: VideoCodec,
    rate: u32,
) -> Result<(FeedThread, DecodedFrames), DecoderError> {
    let decoder = open_decoder(codec)?;
    let frames = decoder.frames();
    let feed = FeedThread::spawn("clip-feed", move |stop| play(codec, rate, decoder, stop));
    Ok((feed, frames))
}

// Opens a WebCodecs decoder and feeds the clip asynchronously on the browser event loop.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn run_async(
    codec: VideoCodec,
    rate: u32,
    on_frames: impl FnOnce(DecodedFrames),
) -> Result<(), DecoderError> {
    let mut decoder = open_decoder(codec)?;
    on_frames(decoder.frames());
    let units = split_access_units(codec, clip_bytes(codec));
    let step = Duration::from_secs(1) / rate;
    let mut index = 0_u64;
    loop {
        for unit in &units {
            let timestamp = step * u32::try_from(index).unwrap_or(u32::MAX);
            let access_unit = AccessUnit {
                data: unit,
                timestamp,
                metadata: index,
            };
            if let Err(error) = decoder.submit(access_unit)
                && !matches!(error, DecoderError::Full)
            {
                log::warn!("player demo: picture {index} dropped: {error}");
            }
            index += 1;
            delay(step).await;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn play(codec: VideoCodec, rate: u32, mut decoder: VideoDecoder, stop: &AtomicBool) {
    let units = split_access_units(codec, clip_bytes(codec));
    let step = Duration::from_secs(1) / rate;
    let started = Instant::now();
    let mut reported = Instant::now();
    let mut index = 0_u64;
    while !stop.load(Ordering::Relaxed) {
        for unit in &units {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let timestamp = step * u32::try_from(index).unwrap_or(u32::MAX);
            let access_unit = AccessUnit {
                data: unit,
                timestamp,
                metadata: index,
            };
            if let Err(error) = decoder.submit(access_unit) {
                log::warn!("player demo: picture {index} dropped: {error}");
            }
            index += 1;
            if reported.elapsed() >= REPORT_EVERY {
                let counts = decoder.frames().stats();
                log::info!(
                    "player demo: {codec:?} {} decoded, decode {:.2} ms mean {:.2} ms longest, {} dropped",
                    counts.decoded,
                    counts.mean_decode.as_secs_f64() * 1000.0,
                    counts.max_decode.as_secs_f64() * 1000.0,
                    counts.dropped,
                );
                reported = Instant::now();
            }
            // The app paces the pictures at the clip's rate
            #[expect(
                clippy::disallowed_methods,
                reason = "the clip's own thread paces its pictures; no frame loop waits on it"
            )]
            thread::sleep(
                (started + step * u32::try_from(index).unwrap_or(u32::MAX))
                    .saturating_duration_since(Instant::now()),
            );
        }
    }
}
