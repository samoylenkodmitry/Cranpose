//! The limits of the device's video decoders on Android, through `CranposeVideoCodecs`. The NDK
//! lists no media codecs at the API levels that headsets run, so the Java `MediaCodecList` answers.
//! Each query runs when an app asks, so an app that never asks pays nothing at its start.

use std::sync::Arc;

use cranpose_services::{
    DecoderCapabilities, DecoderCapabilitySource, VideoCodec, set_platform_decoder_capabilities,
};
use jni::{
    JValueOwned, jni_sig, jni_str,
    objects::{JIntArray, JString, JValue},
};

use super::jni::AndroidContext;

const CODECS_CLASS: &str = "dev/cranpose/android/CranposeVideoCodecs";

pub(crate) fn register(context: &'static AndroidContext) {
    set_platform_decoder_capabilities(Some(Arc::new(AndroidDecoderCapabilities { context })));
}

struct AndroidDecoderCapabilities {
    context: &'static AndroidContext,
}

impl DecoderCapabilitySource for AndroidDecoderCapabilities {
    fn capabilities(&self, codec: VideoCodec) -> Option<DecoderCapabilities> {
        let answer = self.context.call_class(CODECS_CLASS, |env, _, class| {
            let mime = env.new_string(codec.mime())?;
            let name = env
                .call_static_method(
                    &class,
                    jni_str!("decoderName"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                    &[JValue::Object(mime.as_ref())],
                )
                .and_then(JValueOwned::l)?;
            let limits = env
                .call_static_method(
                    &class,
                    jni_str!("limits"),
                    jni_sig!("(Ljava/lang/String;)[I"),
                    &[JValue::Object(mime.as_ref())],
                )
                .and_then(JValueOwned::l)?;
            if name.is_null() || limits.is_null() {
                return Ok(None);
            }
            let name = JString::cast_local(env, name)?.try_to_string(env)?;
            let limits = JIntArray::cast_local(env, limits)?;
            let mut values = [0_i32; 4];
            limits.get_region(env, 0, &mut values)?;
            Ok(Some((name, values)))
        });
        match answer {
            Ok(Some((name, [width, height, bitrate, hardware]))) => Some(DecoderCapabilities {
                codec,
                name,
                hardware_accelerated: match hardware {
                    0 => Some(false),
                    1 => Some(true),
                    _ => None,
                },
                max_width: width.max(0).cast_unsigned(),
                max_height: height.max(0).cast_unsigned(),
                max_bitrate: bitrate.max(0).cast_unsigned(),
            }),
            Ok(None) => None,
            Err(error) => {
                log::warn!("the decoder limits of {codec:?} could not be read: {error}");
                None
            }
        }
    }

    fn max_frame_rate(&self, codec: VideoCodec, width: u32, height: u32) -> Option<f32> {
        let rate = self.context.call_class(CODECS_CLASS, |env, _, class| {
            let mime = env.new_string(codec.mime())?;
            env.call_static_method(
                &class,
                jni_str!("maxFrameRate"),
                jni_sig!("(Ljava/lang/String;II)D"),
                &[
                    JValue::Object(mime.as_ref()),
                    JValue::Int(i32::try_from(width).unwrap_or(i32::MAX)),
                    JValue::Int(i32::try_from(height).unwrap_or(i32::MAX)),
                ],
            )
            .and_then(JValueOwned::d)
        });
        match rate {
            Ok(rate) => Some(rate as f32),
            Err(error) => {
                log::warn!("the frame rate of {codec:?} could not be read: {error}");
                None
            }
        }
    }
}
