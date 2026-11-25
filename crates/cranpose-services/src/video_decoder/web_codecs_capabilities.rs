//! WebCodecs decoder capability reporting for browser targets.

use wasm_bindgen::JsValue;

use super::{DecoderCapabilities, DecoderCapabilitySource, VideoCodec};

pub(super) struct Limits;

pub(super) fn webcodecs_available() -> bool {
    let global = js_sys::global();
    let has_decoder = js_sys::Reflect::get(&global, &JsValue::from_str("VideoDecoder"))
        .ok()
        .is_some_and(|v| v.is_function());
    let has_chunk = js_sys::Reflect::get(&global, &JsValue::from_str("EncodedVideoChunk"))
        .ok()
        .is_some_and(|v| v.is_function());
    has_decoder && has_chunk
}

impl DecoderCapabilitySource for Limits {
    fn capabilities(&self, codec: VideoCodec) -> Option<DecoderCapabilities> {
        if !webcodecs_available() {
            return None;
        }
        let name = match codec {
            VideoCodec::H264 => "webcodecs/h264",
            VideoCodec::Hevc => "webcodecs/hevc",
            VideoCodec::Vp9 => "webcodecs/vp9",
            VideoCodec::Av1 => "webcodecs/av1",
        };
        Some(DecoderCapabilities {
            codec,
            name: name.into(),
            hardware_accelerated: None,
            max_width: None,
            max_height: None,
            max_bitrate: None,
            b_frame_reordering: true,
            hdr_10bit: Some(codec != VideoCodec::H264),
            zero_copy_gpu: true,
        })
    }

    fn max_frame_rate(&self, _codec: VideoCodec, _width: u32, _height: u32) -> Option<f32> {
        None
    }
}
