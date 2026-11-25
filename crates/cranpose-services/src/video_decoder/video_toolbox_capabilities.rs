//! The decoder limits that VideoToolbox gives. It tells whether a codec decodes on dedicated
//! hardware and offers no query for the sizes, rates or bit rates that it takes.
#![expect(
    unsafe_code,
    reason = "VTIsHardwareDecodeSupported is a C function of VideoToolbox"
)]

use objc2_core_media::{kCMVideoCodecType_H264, kCMVideoCodecType_HEVC};
use objc2_video_toolbox::VTIsHardwareDecodeSupported;

use super::{DecoderCapabilities, DecoderCapabilitySource, VideoCodec};

pub(super) struct Limits;

impl DecoderCapabilitySource for Limits {
    fn capabilities(&self, codec: VideoCodec) -> Option<DecoderCapabilities> {
        // The decoder takes H.264 and HEVC only, as its `new` says
        let (codec_type, name) = match codec {
            VideoCodec::H264 => (kCMVideoCodecType_H264, "VideoToolbox H.264"),
            VideoCodec::Hevc => (kCMVideoCodecType_HEVC, "VideoToolbox HEVC"),
            VideoCodec::Vp9 | VideoCodec::Av1 => return None,
        };
        // SAFETY: the function reads no memory of the caller. It takes a codec type by value.
        let hardware = unsafe { VTIsHardwareDecodeSupported(codec_type) };
        Some(DecoderCapabilities {
            codec,
            name: name.into(),
            hardware_accelerated: Some(hardware),
            max_width: None,
            max_height: None,
            max_bitrate: None,
            b_frame_reordering: true,
            hdr_10bit: Some(codec == VideoCodec::Hevc),
            zero_copy_gpu: true,
        })
    }

    fn max_frame_rate(&self, _codec: VideoCodec, _width: u32, _height: u32) -> Option<f32> {
        None
    }
}
