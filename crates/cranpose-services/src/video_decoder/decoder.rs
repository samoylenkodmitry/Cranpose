//! The decoder and the handle that receives its pictures.

#[cfg(target_os = "android")]
use super::media_codec::{Backend, Frames};
#[cfg(not(target_os = "android"))]
use super::unavailable::{Backend, Frames};
use super::{
    AccessUnit, DecoderCapabilities, DecoderConfig, DecoderError, DecoderStats, FrameInfo,
    VideoCodec, VideoFrame, capabilities,
};

/// A hardware video decoder for one stream.
///
/// The app feeds it access units with [`submit`](Self::submit), from one thread at a time. A
/// thread of the decoder drains the pictures, so a picture is ready as soon as the hardware has
/// it. [`frames`](Self::frames) gives the handle that receives them.
///
/// The decoder holds no picture back for reordering, so the stream must have no B-frames.
/// Dropping the decoder stops it. It never waits for the app's threads.
pub struct VideoDecoder {
    backend: Backend,
}

impl VideoDecoder {
    /// Makes a decoder for `config`. The decoder is unavailable on a platform without a hardware
    /// decoder that Cranpose drives.
    pub fn new(config: DecoderConfig) -> Result<Self, DecoderError> {
        Ok(Self {
            backend: Backend::new(config)?,
        })
    }

    /// The limits of the hardware decoder that [`new`](Self::new) opens for `codec`: its largest
    /// picture and bit rate, and through
    /// [`DecoderCapabilities::max_frame_rate`] its frame rate at a size. Android reads them from
    /// the device's media codec list. The call costs a query of the platform, so an app makes it
    /// once for each codec. It reports [`DecoderError::Unavailable`] where the platform gives no
    /// limits and [`DecoderError::NoDecoder`] for a codec that the device cannot decode.
    pub fn capabilities(codec: VideoCodec) -> Result<DecoderCapabilities, DecoderError> {
        capabilities::of(codec)
    }

    /// Gives the decoder one access unit. It waits up to
    /// [`DecoderConfig::input_wait`] for the decoder to take it, and reports
    /// [`DecoderError::Full`] after that. The unit's data is copied, so the app may reuse it.
    pub fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        self.backend.submit(unit)
    }

    /// The handle that receives the decoded pictures. It stays usable after the decoder drops,
    /// and returns the pictures that were left.
    pub fn frames(&self) -> DecodedFrames {
        DecodedFrames {
            frames: self.backend.frames(),
        }
    }
}

/// The decoded pictures of a [`VideoDecoder`]. A clone receives from the same decoder, on any
/// thread. Two handles are equal when they receive from the same decoder.
#[derive(Clone, PartialEq)]
pub struct DecodedFrames {
    frames: Frames,
}

impl DecodedFrames {
    /// The oldest picture that the decoder has output, if the output is
    /// [`DecoderOutput::Frames`](super::DecoderOutput::Frames).
    pub fn next_frame(&self) -> Option<VideoFrame> {
        self.frames.next_frame()
    }

    /// The newest picture, if the output is [`DecoderOutput::Frames`](super::DecoderOutput::Frames).
    /// It drops the older pictures.
    pub fn latest_frame(&self) -> Option<VideoFrame> {
        self.frames.latest_frame()
    }

    /// The report of the oldest picture that the decoder sent to the surface, if the output is
    /// [`DecoderOutput::Surface`](super::DecoderOutput::Surface). The decoder keeps the latest 32
    /// reports.
    pub fn next_report(&self) -> Option<FrameInfo> {
        self.frames.next_report()
    }

    /// What the decoder has done since it started.
    pub fn stats(&self) -> DecoderStats {
        self.frames.stats()
    }
}
