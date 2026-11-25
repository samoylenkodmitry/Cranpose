use super::*;
use crate::video_decoder::annex_b::AccessUnitNals;

const H264_CLIP: &[u8] = include_bytes!("../../../../../apps/xr-demo/assets/test_clip.h264");
const HEVC_CLIP: &[u8] = include_bytes!("../../../../../apps/xr-demo/assets/test_clip.h265");
const TONES_2020_PQ: &[u8] =
    include_bytes!("../../../../cranpose/tests/assets/test_tones_2020_pq.h265");

#[test]
fn webcodecs_codec_strings_parse_h264_hevc_and_hdr_sps() {
    let mut nals = AccessUnitNals::default();

    nals.parse(VideoCodec::H264, H264_CLIP);
    let h264_sps = nals
        .sequence_parameter_set(H264_CLIP)
        .expect("H.264 clip SPS");
    let h264_str = webcodecs_codec_string(VideoCodec::H264, Some(h264_sps));
    assert!(
        h264_str.starts_with("avc1.") && h264_str.len() == 11,
        "unexpected H.264 codec string: {h264_str}"
    );
    assert_eq!(
        webcodecs_codec_string(VideoCodec::H264, None),
        "avc1.640028"
    );

    nals.parse(VideoCodec::Hevc, HEVC_CLIP);
    let hevc_sps = nals
        .sequence_parameter_set(HEVC_CLIP)
        .expect("HEVC clip SPS");
    let hevc_str = webcodecs_codec_string(VideoCodec::Hevc, Some(hevc_sps));
    assert!(
        hevc_str.starts_with("hvc1.1."),
        "expected HEVC Main profile string, got {hevc_str}"
    );

    nals.parse(VideoCodec::Hevc, TONES_2020_PQ);
    let hdr_sps = nals
        .sequence_parameter_set(TONES_2020_PQ)
        .expect("10-bit HEVC SPS");
    let hdr_str = webcodecs_codec_string(VideoCodec::Hevc, Some(hdr_sps));
    assert!(
        hdr_str.starts_with("hvc1.2."),
        "expected HEVC Main 10 profile string, got {hdr_str}"
    );

    assert_eq!(
        webcodecs_codec_string(VideoCodec::Av1, None),
        "av01.0.04M.08"
    );
}
