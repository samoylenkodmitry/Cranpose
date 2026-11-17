//! Hardware video decoding for a low-latency stream.
//!
//! An app makes a [`VideoDecoder`] for a codec and a size, feeds it access units, and chooses
//! where the pictures go. [`DecoderOutput::Surface`] sends them straight to an
//! [`ExternalSurface`](crate::ExternalSurface), which a headset's compositor shows with no copy.
//! [`DecoderOutput::Frames`] returns each picture as a [`VideoFrame`] that a renderer imports as
//! a GPU image.
//!
//! The decoder asks the hardware for the least delay: it holds no picture back for reordering,
//! so the stream must have no B-frames. Android decodes through `MediaCodec`. Other platforms
//! report the decoder as unavailable.

mod codec;
mod config;
mod decoder;
mod error;
mod frame;
#[cfg(target_os = "android")]
mod image_reader;
#[cfg(target_os = "android")]
mod media_codec;
#[cfg(any(test, target_os = "android"))]
mod pending_units;
mod stats;
#[cfg(not(target_os = "android"))]
mod unavailable;

pub use codec::VideoCodec;
pub use config::{AccessUnit, DecoderConfig, DecoderOutput};
pub use decoder::{DecodedFrames, VideoDecoder};
pub use error::DecoderError;
pub use frame::{CropRect, FrameInfo, VideoFrame};
pub use stats::DecoderStats;
