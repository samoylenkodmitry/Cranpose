//! Why a decoder fails.

use super::VideoCodec;

/// Why a video decoder could not start or take an access unit.
#[derive(Debug, thiserror::Error)]
pub enum DecoderError {
    /// This platform has no hardware video decoder that Cranpose drives.
    #[error("this platform has no hardware video decoder")]
    Unavailable,
    /// The device has no decoder for the codec.
    #[error("no video decoder for {0:?}")]
    NoDecoder(VideoCodec),
    /// The decoder refused its configuration.
    #[error("the video decoder refused its configuration: {0}")]
    Configure(String),
    /// The decoder did not take the access unit in time. It counts the unit as refused. The app
    /// may send the unit again. A stream with inter pictures decodes wrongly after a unit that
    /// the decoder never saw, until the next keyframe. An app that drops the unit therefore asks
    /// the sender for a keyframe.
    #[error("the video decoder is not ready for another access unit")]
    Full,
    /// An access unit is larger than the decoder's input buffer.
    #[error("an access unit of {size} bytes does not fit the decoder's input of {limit} bytes")]
    AccessUnitTooLarge {
        /// The size of the access unit in bytes.
        size: usize,
        /// The largest size that the decoder takes in bytes.
        limit: usize,
    },
    /// An access unit's timestamp does not rise above the one before.
    #[error("an access unit's timestamp does not rise above the one before")]
    TimestampNotRising,
    /// The decoder cannot decode the access unit yet, and has skipped it. It starts with a
    /// keyframe that carries the stream's parameter sets, or one that follows them. The decoder
    /// asks for one again after it ends its session, as an iPhone does when an app goes to the
    /// background. The app asks the sender for a keyframe.
    #[error("the video decoder needs a keyframe")]
    NeedsKeyframe,
    /// The decoder stopped with an error.
    #[error("the video decoder failed: {0}")]
    Failed(String),
}
