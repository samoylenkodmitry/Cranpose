// Linux VA-API DMABUF and software fallback video decoder integration tests.

#![cfg(target_os = "linux")]

use std::{
    thread,
    time::{Duration, Instant},
};

use cranpose_services::{
    AccessUnit, ColorPrimaries, ColorRange, ColorTransfer, DecodeOrder, DecodedFrames,
    DecoderConfig, DecoderOutput, LinuxFrameMemory, LinuxPixelFormat, VideoCodec, VideoDecoder,
    VideoRotation, split_access_units,
};

const H264_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h264");
const HEVC_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h265");
const BFRAMES_H264: &[u8] = include_bytes!("../../cranpose/tests/assets/test_bframes.h264");
const BFRAMES_H264_PTS: [u64; 12] = [0, 4, 2, 1, 3, 8, 6, 5, 7, 11, 9, 10];
const BFRAMES_HEVC: &[u8] = include_bytes!("../../cranpose/tests/assets/test_bframes.h265");
const BFRAMES_HEVC_PTS: [u64; 12] = [0, 4, 2, 1, 3, 8, 6, 5, 7, 11, 10, 9];
const TONES_2020_PQ: &[u8] = include_bytes!("../../cranpose/tests/assets/test_tones_2020_pq.h265");
const STEP: Duration = Duration::from_micros(16_667);

fn drain_available(frames: &DecodedFrames, got: &mut Vec<Duration>) {
    while let Some(frame) = frames.next_frame() {
        match frame.linux_frame() {
            LinuxFrameMemory::DmaBuf(dmabuf) => {
                assert!(dmabuf.luma.fd >= 0);
                assert!(dmabuf.chroma.fd >= 0);
                assert!(dmabuf.luma.stride > 0);
                assert!(dmabuf.chroma.stride > 0);
            }
            LinuxFrameMemory::Software(planes) => {
                assert!(!planes.luma.is_empty());
                assert!(!planes.chroma.is_empty());
            }
        }
        got.push(frame.info().timestamp);
    }
}

fn collect_timestamps(frames: &DecodedFrames, got: &mut Vec<Duration>, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while got.len() < expected && Instant::now() < deadline {
        drain_available(frames, got);
        if got.len() < expected {
            thread::sleep(Duration::from_millis(2));
        }
    }
}

#[test]
fn linux_capabilities_report_h264_and_hevc() {
    for codec in [VideoCodec::H264, VideoCodec::Hevc] {
        let caps = VideoDecoder::capabilities(codec).expect("Linux video decoder capabilities");
        assert_eq!(caps.codec, codec);
        assert!(!caps.name.is_empty());
        assert!(caps.b_frame_reordering);
        assert_eq!(caps.zero_copy_gpu, caps.hardware_accelerated == Some(true));
    }
}

#[test]
fn linux_decodes_h264_and_hevc_b_frame_streams_in_presentation_order() {
    for (codec, clip, pts_order) in [
        (VideoCodec::H264, BFRAMES_H264, &BFRAMES_H264_PTS[..]),
        (VideoCodec::Hevc, BFRAMES_HEVC, &BFRAMES_HEVC_PTS[..]),
    ] {
        let config = DecoderConfig::new(codec, 640, 360, DecoderOutput::Frames { depth: 16 })
            .with_order(DecodeOrder::Presentation)
            .with_rotation(VideoRotation::Degrees90);
        let mut decoder = VideoDecoder::new(config).expect("Linux video decoder");
        let frames = decoder.frames();
        let mut got = Vec::with_capacity(pts_order.len());

        let units = split_access_units(codec, clip);
        assert_eq!(units.len(), pts_order.len());
        for (&pts_index, data) in pts_order.iter().zip(units) {
            let unit = AccessUnit {
                data,
                timestamp: STEP * u32::try_from(pts_index).expect("small"),
                metadata: pts_index,
            };
            decoder.submit(unit).expect("submit access unit");
            drain_available(&frames, &mut got);
        }
        decoder.drain().expect("drain reordered tail");
        collect_timestamps(&frames, &mut got, pts_order.len());

        let expected: Vec<Duration> = (0..pts_order.len())
            .map(|i| STEP * u32::try_from(i).expect("small"))
            .collect();
        assert_eq!(got, expected, "presentation order mismatch for {codec:?}");
    }
}

#[test]
fn linux_decodes_10bit_hdr_hevc_and_flushes_cleanly() {
    let config = DecoderConfig::new(
        VideoCodec::Hevc,
        640,
        360,
        DecoderOutput::Frames { depth: 4 },
    )
    .with_order(DecodeOrder::Presentation);
    let mut decoder = VideoDecoder::new(config).expect("Linux HEVC decoder");
    let frames = decoder.frames();
    for (idx, data) in split_access_units(VideoCodec::Hevc, TONES_2020_PQ)
        .into_iter()
        .enumerate()
    {
        decoder
            .submit(AccessUnit {
                data,
                timestamp: STEP * u32::try_from(idx).expect("small"),
                metadata: idx as u64,
            })
            .expect("submit 10-bit PQ unit");
    }
    decoder.drain().expect("drain 10-bit PQ clip");
    let frame = frames.next_frame().expect("decoded 10-bit PQ frame");
    assert_eq!(frame.color().primaries, ColorPrimaries::Bt2020);
    assert_eq!(frame.color().range, ColorRange::Limited);
    assert_eq!(frame.color().transfer, ColorTransfer::Pq);
    let fmt = match frame.linux_frame() {
        LinuxFrameMemory::DmaBuf(d) => d.format,
        LinuxFrameMemory::Software(s) => s.format,
    };
    assert_eq!(fmt, LinuxPixelFormat::P010);

    let h264_units = split_access_units(VideoCodec::H264, H264_CLIP);
    let mut h264 = VideoDecoder::new(
        DecoderConfig::new(
            VideoCodec::H264,
            640,
            360,
            DecoderOutput::Frames { depth: 4 },
        )
        .with_order(DecodeOrder::Presentation),
    )
    .expect("H.264 decoder");
    let h264_frames = h264.frames();
    h264.submit(AccessUnit {
        data: h264_units[0],
        timestamp: STEP * 50,
        metadata: 1,
    })
    .expect("first unit");
    h264.flush().expect("flush");
    assert!(h264_frames.next_frame().is_none());
    h264.submit(AccessUnit {
        data: h264_units[0],
        timestamp: Duration::ZERO,
        metadata: 2,
    })
    .expect("post-flush keyframe");
    h264.drain().expect("drain post-flush");
    let restarted = h264_frames.next_frame().expect("frame after flush");
    assert_eq!(restarted.info().timestamp, Duration::ZERO);
    assert_eq!(restarted.info().metadata, 2);
    let _ = HEVC_CLIP;
}
