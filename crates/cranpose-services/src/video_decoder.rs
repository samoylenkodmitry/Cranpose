//! Hardware video decoding for a low-latency stream.
//!
//! An app makes a [`VideoDecoder`] for a codec and a size, feeds it access units, and chooses
//! where the pictures go. [`DecoderOutput::Surface`] sends them straight to an
//! [`ExternalSurface`](crate::ExternalSurface), which a headset's compositor shows with no copy.
//! [`DecoderOutput::Frames`] returns each picture as a [`VideoFrame`] that a renderer imports as
//! a GPU image.
//!
//! The decoder asks the hardware for the least delay: it holds no picture back for reordering,
//! so the stream must have no B-frames. Android decodes through `MediaCodec`. macOS and iOS
//! decode through VideoToolbox and output [`VideoFrame`]s only. Other platforms report the
//! decoder as unavailable.

#[cfg(any(target_os = "android", target_os = "macos", target_os = "ios"))]
mod admission;
mod annex_b;
mod codec;
mod config;
mod decoder;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod decompression_session;
mod error;
mod frame;
#[cfg(any(target_os = "android", target_os = "macos", target_os = "ios"))]
mod frame_queue;
#[cfg(target_os = "android")]
mod image_reader;
#[cfg(target_os = "android")]
mod media_codec;
#[cfg(any(test, target_os = "android", target_os = "macos", target_os = "ios"))]
mod pending_units;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod pixel_buffer;
mod split_access_units;
mod stats;
#[cfg(not(any(target_os = "android", target_os = "macos", target_os = "ios")))]
mod unavailable;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod video_toolbox;

pub use codec::VideoCodec;
pub use config::{AccessUnit, DecoderConfig, DecoderOutput};
pub use decoder::{DecodedFrames, VideoDecoder};
pub use error::DecoderError;
pub use frame::{CropRect, FrameInfo, VideoFrame};
pub use split_access_units::split_access_units;
pub use stats::DecoderStats;
