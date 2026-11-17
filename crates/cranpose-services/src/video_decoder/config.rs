//! What a decoder is made with and what it takes.

use std::time::Duration;

use super::VideoCodec;
use crate::ExternalSurface;

// How long `submit` waits for the decoder to take an access unit before it reports `Full`.
const INPUT_WAIT: Duration = Duration::from_millis(4);

/// Where a decoder puts its pictures.
#[derive(Debug)]
pub enum DecoderOutput {
    /// The pictures go straight to the surface with no copy. The compositor that shows the
    /// surface reads them. [`DecodedFrames::next_report`](super::DecodedFrames::next_report)
    /// tells which frames the decoder sent.
    Surface(ExternalSurface),
    /// Each picture comes back as a [`VideoFrame`](super::VideoFrame). At most `depth` pictures
    /// are in flight: those that the decoder keeps for the app, and those that the app holds. A
    /// decoder whose pictures all stay with the app waits until the app drops one.
    Frames {
        /// The number of pictures that may be in flight, at least 2.
        depth: usize,
    },
}

/// What [`VideoDecoder::new`](super::VideoDecoder::new) makes a decoder from.
#[derive(Debug)]
pub struct DecoderConfig {
    /// The codec of the stream.
    pub codec: VideoCodec,
    /// The width of the pictures in pixels.
    pub width: u32,
    /// The height of the pictures in pixels.
    pub height: u32,
    /// Where the pictures go.
    pub output: DecoderOutput,
    /// How long [`VideoDecoder::submit`](super::VideoDecoder::submit) waits for the decoder to
    /// take an access unit before it reports [`DecoderError::Full`](super::DecoderError::Full).
    pub input_wait: Duration,
}

impl DecoderConfig {
    /// A configuration for a stream of `width` by `height` pixels in `codec` with the default
    /// input wait of 4 milliseconds.
    pub const fn new(codec: VideoCodec, width: u32, height: u32, output: DecoderOutput) -> Self {
        Self {
            codec,
            width,
            height,
            output,
            input_wait: INPUT_WAIT,
        }
    }
}

/// One picture's coded bytes, with the values that come back with the decoded picture.
#[derive(Clone, Copy, Debug)]
pub struct AccessUnit<'a> {
    /// The bytes of one picture: Annex-B NAL units for H.264 and HEVC, which may start with the
    /// parameter sets, or one temporal unit of OBUs for AV1.
    pub data: &'a [u8],
    /// The presentation time. It must rise from one access unit to the next: the decoder finds
    /// the picture's metadata by it. The decoder keeps it to the microsecond.
    pub timestamp: Duration,
    /// A value of the app that returns with the decoded picture, such as a frame index that
    /// maps to a render pose.
    pub metadata: u64,
}
