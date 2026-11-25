// Windows Media Foundation video decoder integration tests.

#![cfg(target_os = "windows")]

use std::{
    thread,
    time::{Duration, Instant},
};

use cranpose_services::{
    AccessUnit, ColorPrimaries, ColorRange, ColorTransfer, DecodeOrder, DecodedFrames,
    DecoderConfig, DecoderOutput, VideoCodec, VideoDecoder, VideoRotation, split_access_units,
};

const H264_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h264");
const BFRAMES_H264: &[u8] = include_bytes!("../../cranpose/tests/assets/test_bframes.h264");
const BFRAMES_H264_PTS: [u64; 12] = [0, 4, 2, 1, 3, 8, 6, 5, 7, 11, 9, 10];
const STEP: Duration = Duration::from_micros(16_667);

fn wait_frames(frames: &DecodedFrames, expected: usize) -> Vec<Duration> {
    let mut timestamps = Vec::with_capacity(expected);
    let deadline = Instant::now() + Duration::from_secs(5);
    while timestamps.len() < expected && Instant::now() < deadline {
        if let Some(frame) = frames.next_frame() {
            assert!(!frame.dxgi_shared_frame().texture_handle.is_null());
            assert!(!frame.dxgi_shared_frame().fence_handle.is_null());
            timestamps.push(frame.info().timestamp);
        } else {
            thread::sleep(Duration::from_millis(2));
        }
    }
    timestamps
}

#[test]
fn media_foundation_reports_h264_capabilities() {
    let caps = VideoDecoder::capabilities(VideoCodec::H264).expect("H.264 decoder capabilities");
    assert_eq!(caps.codec, VideoCodec::H264);
    assert!(!caps.name.is_empty());
    assert!(caps.b_frame_reordering);
}

#[test]
fn media_foundation_decodes_b_frames_in_presentation_order_and_drains() {
    let config = DecoderConfig::new(
        VideoCodec::H264,
        640,
        360,
        DecoderOutput::Frames { depth: 8 },
    )
    .with_order(DecodeOrder::Presentation)
    .with_rotation(VideoRotation::Degrees90);
    let mut decoder = VideoDecoder::new(config).expect("Media Foundation H.264 decoder");
    let frames = decoder.frames();

    let units = split_access_units(VideoCodec::H264, BFRAMES_H264);
    assert_eq!(units.len(), BFRAMES_H264_PTS.len());
    for (&pts_index, data) in BFRAMES_H264_PTS.iter().zip(units) {
        let unit = AccessUnit {
            data,
            timestamp: STEP * u32::try_from(pts_index).expect("small"),
            metadata: pts_index,
        };
        decoder.submit(unit).expect("submit access unit");
    }
    decoder.drain().expect("drain remaining reordered pictures");

    let got = wait_frames(&frames, BFRAMES_H264_PTS.len());
    let expected: Vec<Duration> = (0..BFRAMES_H264_PTS.len())
        .map(|i| STEP * u32::try_from(i).expect("small"))
        .collect();
    assert_eq!(got, expected);
}

#[test]
fn media_foundation_flush_resets_and_decodes_fresh_segment() {
    let config = DecoderConfig::new(
        VideoCodec::H264,
        640,
        360,
        DecoderOutput::Frames { depth: 4 },
    )
    .with_order(DecodeOrder::Presentation);
    let mut decoder = VideoDecoder::new(config).expect("Media Foundation H.264 decoder");
    let frames = decoder.frames();
    let units = split_access_units(VideoCodec::H264, H264_CLIP);

    decoder
        .submit(AccessUnit {
            data: units[0],
            timestamp: STEP * 100,
            metadata: 1,
        })
        .expect("initial keyframe");
    decoder.flush().expect("flush decoder");
    assert!(frames.next_frame().is_none());

    decoder
        .submit(AccessUnit {
            data: units[0],
            timestamp: Duration::ZERO,
            metadata: 2,
        })
        .expect("post-flush keyframe");
    decoder.drain().expect("drain post-flush segment");
    let frame = frames
        .next_frame()
        .expect("a decoded picture after flush and drain");
    assert_eq!(frame.info().timestamp, Duration::ZERO);
    assert_eq!(frame.info().metadata, 2);
    assert!(matches!(
        frame.color().primaries,
        ColorPrimaries::Bt601 | ColorPrimaries::Bt709 | ColorPrimaries::Bt2020
    ));
    assert!(matches!(
        frame.color().range,
        ColorRange::Limited | ColorRange::Full
    ));
    assert_eq!(frame.color().transfer, ColorTransfer::Sdr);
}
