// The video decoder on a platform that has none.

#![cfg(not(any(target_os = "android", target_os = "macos", target_os = "ios")))]

use cranpose_services::{
    DecoderConfig, DecoderError, DecoderOutput, MediaCapabilities, VideoCodec, VideoDecoder,
};

#[test]
fn a_platform_without_a_hardware_decoder_reports_it_unavailable() {
    for codec in [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1] {
        let config = DecoderConfig::new(codec, 640, 360, DecoderOutput::Frames { depth: 4 });
        assert!(matches!(
            VideoDecoder::new(config),
            Err(DecoderError::Unavailable)
        ));
        assert!(matches!(
            VideoDecoder::capabilities(codec),
            Err(DecoderError::Unavailable)
        ));
    }
    assert!(!MediaCapabilities::default().video);
}
