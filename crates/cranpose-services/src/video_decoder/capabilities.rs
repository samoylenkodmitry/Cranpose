//! What a device's hardware decoder takes.

use std::sync::Arc;

use super::{DecoderError, VideoCodec};
use crate::registry::ServiceRegistry;

/// The limits of the hardware decoder that [`VideoDecoder::new`](super::VideoDecoder::new) opens
/// for one codec. A stream above a limit decodes with errors or not at all. An app reads the
/// limits with [`VideoDecoder::capabilities`](super::VideoDecoder::capabilities) before it asks
/// its host for a picture size.
///
/// Android reports every limit. VideoToolbox on macOS and iOS offers no query for sizes, rates or
/// bit rates, so those limits are `None` there: the decoder takes a size until it fails.
#[derive(Clone, Debug, PartialEq)]
pub struct DecoderCapabilities {
    /// The codec that these limits belong to.
    pub codec: VideoCodec,
    /// The name of the decoder component, such as `c2.qti.hevc.decoder` on a Quest 3.
    pub name: String,
    /// Whether the component runs on dedicated hardware, or `None` when the platform does not
    /// say.
    pub hardware_accelerated: Option<bool>,
    /// The widest picture in pixels that the decoder takes at any height, or `None` when the
    /// platform does not say.
    pub max_width: Option<u32>,
    /// The tallest picture in pixels that the decoder takes at any width, or `None` when the
    /// platform does not say.
    pub max_height: Option<u32>,
    /// The highest bit rate in bits a second that the decoder takes, or `None` when the platform
    /// does not say.
    pub max_bitrate: Option<u32>,
    /// Whether the decoder supports [`DecodeOrder::Presentation`](super::DecodeOrder::Presentation)
    /// B-frame reordering on this platform.
    pub b_frame_reordering: bool,
    /// Whether the decoder can output 10-bit HDR pictures (such as P010), or `None` when the
    /// platform does not report bit-depth support ahead of time.
    pub hdr_10bit: Option<bool>,
    /// Whether decoded pictures stay in GPU memory without a CPU pixel copy on this platform.
    pub zero_copy_gpu: bool,
}

impl DecoderCapabilities {
    /// The highest frame rate in pictures a second at which the decoder takes pictures of `width`
    /// by `height` pixels, or `None` when it takes no picture of that size or the platform gives
    /// no rates, which [`max_width`](Self::max_width) being `None` tells. A size can lie within
    /// both maximums and still fail: decoders also limit the number of pixels in a picture.
    pub fn max_frame_rate(&self, width: u32, height: u32) -> Option<f32> {
        source()?
            .max_frame_rate(self.codec, width, height)
            .filter(|rate| *rate > 0.0)
    }
}

/// What the platform knows of its decoders. The platform's backend installs it with
/// [`set_platform_decoder_capabilities`].
pub trait DecoderCapabilitySource: Send + Sync {
    /// The limits of the decoder that the platform opens for `codec`, or `None` when it has none.
    fn capabilities(&self, codec: VideoCodec) -> Option<DecoderCapabilities>;

    /// The highest frame rate at which that decoder takes pictures of `width` by `height` pixels,
    /// or `None` when it takes no picture of that size.
    fn max_frame_rate(&self, codec: VideoCodec, width: u32, height: u32) -> Option<f32>;
}

static PLATFORM: ServiceRegistry<dyn DecoderCapabilitySource> = ServiceRegistry::new();

/// Installs the platform's source of decoder limits, or removes it with `None`. Platform backends
/// call it.
pub fn set_platform_decoder_capabilities(source: Option<Arc<dyn DecoderCapabilitySource>>) {
    match source {
        Some(source) => PLATFORM.set(source),
        None => PLATFORM.clear(),
    }
}

// The source that a platform backend installed, or the built-in probe on platforms whose decoder
// lives in this crate (Apple VideoToolbox and Windows Media Foundation).
fn source() -> Option<Arc<dyn DecoderCapabilitySource>> {
    let installed = PLATFORM.get();
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let installed = installed.or_else(|| Some(Arc::new(super::video_toolbox_capabilities::Limits)));
    #[cfg(target_os = "windows")]
    let installed =
        installed.or_else(|| Some(Arc::new(super::media_foundation_capabilities::Limits)));
    installed
}

pub(super) fn of(codec: VideoCodec) -> Result<DecoderCapabilities, DecoderError> {
    source()
        .ok_or(DecoderError::Unavailable)?
        .capabilities(codec)
        .ok_or(DecoderError::NoDecoder(codec))
}

#[cfg(test)]
#[path = "tests/capabilities_tests.rs"]
mod tests;
