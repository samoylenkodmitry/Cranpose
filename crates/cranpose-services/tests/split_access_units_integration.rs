// The access units of Annex-B streams.

use cranpose_services::{VideoCodec, split_access_units};

const H264_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h264");
const HEVC_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h265");

#[test]
fn a_clip_splits_into_its_pictures_and_loses_no_byte() {
    for (codec, clip) in [(VideoCodec::H264, H264_CLIP), (VideoCodec::Hevc, HEVC_CLIP)] {
        let units = split_access_units(codec, clip);
        assert_eq!(units.len(), 60, "{codec:?}");
        assert_eq!(units.concat(), clip, "{codec:?}");
    }
}

#[test]
fn a_stream_without_delimiters_splits_at_each_first_slice() {
    // H.264: an SPS, a PPS and an IDR slice, then two plain slices. The byte after a slice's
    // header has its top bit set when the slice is the picture's first.
    let h264: [&[u8]; 3] = [
        &[
            0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2, 0, 0, 0, 1, 0x65, 0x88, 3,
        ],
        &[0, 0, 1, 0x41, 0x9a, 4],
        &[0, 0, 0, 1, 0x41, 0x9a, 5],
    ];
    assert_eq!(split_access_units(VideoCodec::H264, &h264.concat()), h264);
    // HEVC: a second slice of the same picture stays with its picture
    let hevc: [&[u8]; 2] = [
        &[
            0, 0, 0, 1, 0x40, 1, 0, 0, 0, 1, 0x42, 1, 0, 0, 0, 1, 0x44, 1, 0, 0, 0, 1, 0x26, 1,
            0x80, 0, 0, 1, 0x26, 1, 0x00, 7,
        ],
        &[0, 0, 0, 1, 0x02, 1, 0x80, 9],
    ];
    assert_eq!(split_access_units(VideoCodec::Hevc, &hevc.concat()), hevc);
}

#[test]
fn bytes_before_the_first_start_code_belong_to_no_unit() {
    let stream = [7, 7, 7, 0, 0, 0, 1, 0x65, 0x88, 1, 0, 0, 1, 0x41, 0x9a];
    assert_eq!(
        split_access_units(VideoCodec::H264, &stream),
        [&stream[3..10], &stream[10..]]
    );
}

#[test]
fn a_stream_with_no_unit_gives_no_units() {
    assert!(split_access_units(VideoCodec::H264, &[1, 2, 3]).is_empty());
    assert!(split_access_units(VideoCodec::Hevc, &[]).is_empty());
}
