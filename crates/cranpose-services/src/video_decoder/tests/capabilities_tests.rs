use super::*;
use crate::{VideoDecoder, registry::test_service_guard};

// A device whose HEVC decoder takes pictures up to 4096 by 2304 and whose H.264 decoder is missing
struct Device;

impl DecoderCapabilitySource for Device {
    fn capabilities(&self, codec: VideoCodec) -> Option<DecoderCapabilities> {
        (codec == VideoCodec::Hevc).then(|| DecoderCapabilities {
            codec,
            name: "c2.test.hevc.decoder".into(),
            hardware_accelerated: Some(true),
            max_width: Some(4096),
            max_height: Some(2304),
            max_bitrate: Some(100_000_000),
            b_frame_reordering: true,
            hdr_10bit: Some(true),
            zero_copy_gpu: true,
        })
    }

    fn max_frame_rate(&self, codec: VideoCodec, width: u32, height: u32) -> Option<f32> {
        (codec == VideoCodec::Hevc && width <= 4096 && height <= 2304).then(|| {
            if width * height > 1920 * 1080 {
                60.0
            } else {
                120.0
            }
        })
    }
}

// Apple platforms have a source of their own
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[test]
fn a_platform_without_a_source_reports_the_decoder_as_unavailable() {
    let _guard = test_service_guard();
    assert!(matches!(
        VideoDecoder::capabilities(VideoCodec::Hevc),
        Err(DecoderError::Unavailable)
    ));
}

#[test]
fn the_limits_of_a_codec_and_its_frame_rate_at_a_size_come_from_the_platform() {
    let _guard = test_service_guard();
    set_platform_decoder_capabilities(Some(Arc::new(Device)));
    let hevc = VideoDecoder::capabilities(VideoCodec::Hevc).expect("an HEVC decoder");
    assert_eq!((hevc.max_width, hevc.max_height), (Some(4096), Some(2304)));
    assert_eq!(hevc.max_frame_rate(1920, 1080), Some(120.0));
    assert_eq!(hevc.max_frame_rate(4096, 2304), Some(60.0));
    // A size beyond the limits has no frame rate
    assert_eq!(hevc.max_frame_rate(4128, 2208), None);
    assert!(matches!(
        VideoDecoder::capabilities(VideoCodec::H264),
        Err(DecoderError::NoDecoder(VideoCodec::H264))
    ));
}

#[test]
fn each_codec_names_the_mime_type_of_the_platform_s_codec_list() {
    assert_eq!(VideoCodec::H264.mime(), "video/avc");
    assert_eq!(VideoCodec::Hevc.mime(), "video/hevc");
    assert_eq!(VideoCodec::Vp9.mime(), "video/x-vnd.on2.vp9");
    assert_eq!(VideoCodec::Av1.mime(), "video/av01");
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn videotoolbox_tells_the_hardware_of_a_codec_and_no_sizes() {
    let _guard = test_service_guard();
    for codec in [VideoCodec::H264, VideoCodec::Hevc] {
        let limits = VideoDecoder::capabilities(codec).expect("VideoToolbox decodes the codec");
        assert!(limits.hardware_accelerated.is_some());
        assert_eq!((limits.max_width, limits.max_height), (None, None));
        assert_eq!(limits.max_frame_rate(1920, 1080), None);
    }
    assert!(matches!(
        VideoDecoder::capabilities(VideoCodec::Av1),
        Err(DecoderError::NoDecoder(VideoCodec::Av1))
    ));
}
