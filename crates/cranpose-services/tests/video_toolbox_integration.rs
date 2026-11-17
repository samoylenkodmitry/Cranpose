// The VideoToolbox decoder on the clips of the XR demo.

#![cfg(any(target_os = "macos", target_os = "ios"))]

use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
    thread,
    time::{Duration, Instant},
};

use cranpose_services::{
    AccessUnit, DecodedFrames, DecoderConfig, DecoderError, DecoderOutput, FrameInfo, VideoCodec,
    VideoDecoder, split_access_units,
};

const H264_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h264");
const HEVC_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h265");
// Other parameter sets than the clip's, at the same size
const TONES_CLIP: &[u8] = include_bytes!("../../cranpose/tests/assets/test_tones_709_full.h264");
const STEP: Duration = Duration::from_micros(16_667);
const DEPTH: usize = 4;
// The decoder holds up to 8 units while it starts, and outputs their pictures at once
const BURST: usize = 16;

fn decoder(codec: VideoCodec, depth: usize) -> VideoDecoder {
    VideoDecoder::new(DecoderConfig::new(
        codec,
        640,
        360,
        DecoderOutput::Frames { depth },
    ))
    .expect("a VideoToolbox decoder")
}

fn unit(data: &[u8], index: u64) -> AccessUnit<'_> {
    AccessUnit {
        data,
        timestamp: STEP * u32::try_from(index).expect("small"),
        metadata: index * 7,
    }
}

// What the app learns of a picture. The app drops the picture itself at once: a decoder with
// four pictures in flight drops the new ones while the app holds four.
struct Picture {
    info: FrameInfo,
    size: (u32, u32),
    crop_width: u32,
    has_buffer: bool,
}

// Feeds `units` from the first, and takes every picture that comes out. Waits for `expected`
// pictures at most two seconds after the last unit.
fn play(decoder: &mut VideoDecoder, units: &[&[u8]], expected: usize) -> Vec<Picture> {
    let frames = decoder.frames();
    let mut out = Vec::new();
    for (index, data) in units.iter().enumerate() {
        loop {
            match decoder.submit(unit(data, index as u64)) {
                Err(DecoderError::Full) => drain(&frames, &mut out),
                result => break result.expect("a unit"),
            }
        }
        drain(&frames, &mut out);
    }
    wait_until(|| {
        drain(&frames, &mut out);
        out.len() >= expected
    });
    out
}

// Polls `done` until it is true, for two seconds at most, and tells whether it became true. The
// decoder's thread outputs the pictures, and the counts that it keeps are the only signal.
#[expect(
    clippy::disallowed_methods,
    reason = "a test waits for a thread of the decoder, and a poll is the way to see its counts"
)]
fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !done() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(1));
    }
    true
}

fn drain(frames: &DecodedFrames, out: &mut Vec<Picture>) {
    out.extend(frames.next_frame().map(|frame| Picture {
        info: frame.info(),
        size: frame.size(),
        crop_width: frame.crop().width,
        has_buffer: !frame.as_pixel_buffer_ptr().is_null(),
    }));
}

#[test]
fn each_clip_decodes_every_picture_with_its_metadata() {
    for (codec, clip) in [(VideoCodec::H264, H264_CLIP), (VideoCodec::Hevc, HEVC_CLIP)] {
        let units = split_access_units(codec, clip);
        let mut decoder = decoder(codec, BURST);
        let frames = play(&mut decoder, &units, units.len());
        assert_eq!(
            frames.len(),
            units.len(),
            "{codec:?} {:?}",
            decoder.frames().stats()
        );
        for (index, picture) in frames.iter().enumerate() {
            let info = picture.info;
            let index = index as u64;
            assert_eq!(info.metadata, index * 7, "{codec:?} picture {index}");
            assert_eq!(info.timestamp, unit(&[], index).timestamp);
            assert!(info.decode_time > Duration::ZERO && info.decode_time < Duration::from_secs(1));
            assert_eq!(picture.size, (640, 360));
            assert_eq!(picture.crop_width, 640);
            assert!(picture.has_buffer);
        }
        let stats = decoder.frames().stats();
        assert_eq!((stats.submitted, stats.decoded, stats.dropped), (60, 60, 0));
        assert!(stats.mean_decode > Duration::ZERO && stats.max_decode >= stats.last_decode);
    }
}

#[test]
fn a_decoder_skips_units_until_a_keyframe_brings_the_parameter_sets() {
    let units = split_access_units(VideoCodec::H264, H264_CLIP);
    let mut decoder = decoder(VideoCodec::H264, DEPTH);
    assert!(matches!(
        decoder.submit(unit(units[1], 1)),
        Err(DecoderError::NeedsKeyframe)
    ));
    decoder.submit(unit(units[0], 2)).expect("the keyframe");
    decoder.submit(unit(units[1], 3)).expect("the next picture");
    let frames = decoder.frames();
    assert!(wait_until(|| frames.stats().decoded >= 2));
    assert_eq!(frames.stats().decoded, 2);
}

#[test]
fn timestamps_must_rise_and_a_codec_without_a_decoder_is_refused() {
    let units = split_access_units(VideoCodec::H264, H264_CLIP);
    let mut decoder = decoder(VideoCodec::H264, DEPTH);
    decoder.submit(unit(units[0], 5)).expect("the keyframe");
    assert!(matches!(
        decoder.submit(unit(units[1], 5)),
        Err(DecoderError::TimestampNotRising)
    ));
    let av1 = DecoderConfig::new(
        VideoCodec::Av1,
        640,
        360,
        DecoderOutput::Frames { depth: 4 },
    );
    assert!(matches!(
        VideoDecoder::new(av1),
        Err(DecoderError::NoDecoder(VideoCodec::Av1))
    ));
}

#[test]
fn an_app_that_holds_every_picture_makes_the_decoder_drop_the_new_ones() {
    let units = split_access_units(VideoCodec::H264, H264_CLIP);
    let mut decoder = decoder(VideoCodec::H264, DEPTH);
    let frames = decoder.frames();
    let mut held = Vec::new();
    for (index, data) in units.iter().take(20).enumerate() {
        // A full decoder waits for room in `submit`, so the retry does not spin
        while let Err(DecoderError::Full) = decoder.submit(unit(data, index as u64)) {}
        // The app takes the oldest picture and keeps it
        held.extend(frames.next_frame());
    }
    assert!(wait_until(|| frames.stats().decoded >= 20));
    assert_eq!(frames.stats().decoded, 20);
    // The pool holds `DEPTH` pictures in all, so the app got that many and no more
    held.extend(frames.next_frame());
    assert_eq!(held.len(), DEPTH);
    assert!(frames.stats().dropped >= 16);
    // A picture that the app drops frees a place for the next one
    drop(held);
    decoder.submit(unit(units[20], 20)).expect("a unit");
    assert!(wait_until(|| frames.stats().decoded >= 21));
    assert!(frames.next_frame().is_some());
}

#[test]
fn a_task_wakes_when_the_decoder_outputs_a_picture() {
    let units = split_access_units(VideoCodec::H264, H264_CLIP);
    let mut decoder = decoder(VideoCodec::H264, DEPTH);
    let frames = decoder.frames();
    for (index, expected) in [(0, 0), (1, 7)] {
        // No picture waits, so the first poll leaves the waker of a task that does nothing
        let mut wait = pin!(frames.wait_for_frame());
        let mut idle = Context::from_waker(Waker::noop());
        assert!(wait.as_mut().poll(&mut idle).is_pending());
        decoder
            .submit(unit(units[index], index as u64))
            .expect("a unit");
        // Another task takes the waker's place, and the decoder's thread wakes it
        pollster::block_on(wait);
        assert_eq!(
            frames.next_frame().map(|frame| frame.info().metadata),
            Some(expected)
        );
    }
}

#[test]
fn a_stream_that_changes_its_parameter_sets_keeps_decoding() {
    let first = split_access_units(VideoCodec::H264, H264_CLIP);
    let second = split_access_units(VideoCodec::H264, TONES_CLIP);
    let units: Vec<&[u8]> = first[..5].iter().chain(&second[..5]).copied().collect();
    let mut decoder = decoder(VideoCodec::H264, BURST);
    let pictures = play(&mut decoder, &units, units.len());
    assert_eq!(pictures.len(), units.len());
    assert_eq!(pictures[9].info.metadata, 9 * 7);
    assert_eq!(pictures[9].size, (640, 360));
}
