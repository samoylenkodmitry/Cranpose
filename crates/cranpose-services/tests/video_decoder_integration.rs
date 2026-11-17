// The video decoder on a platform that has none.

#![cfg(not(target_os = "android"))]

use cranpose_services::{DecoderConfig, DecoderError, DecoderOutput, VideoCodec, VideoDecoder};

#[test]
fn a_platform_without_a_hardware_decoder_reports_it_unavailable() {
    for codec in [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1] {
        let config = DecoderConfig::new(codec, 640, 360, DecoderOutput::Frames { depth: 4 });
        assert!(matches!(
            VideoDecoder::new(config),
            Err(DecoderError::Unavailable)
        ));
    }
}
