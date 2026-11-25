//! Hardware video decoding for real-time and playback video streams.
//!
//! An app makes a [`VideoDecoder`] for a codec and a size, feeds it access units, and chooses
//! where the pictures go. [`DecoderOutput::Surface`] sends them straight to an
//! [`ExternalSurface`](crate::ExternalSurface), which a headset's compositor shows with no copy.
//! [`DecoderOutput::Frames`] returns each picture as a [`VideoFrame`] that a renderer imports as
//! a GPU image.
//!
//! [`DecodeOrder::LowLatency`] asks the hardware for the least delay without reordering, so the
//! stream must have no B-frames. [`DecodeOrder::Presentation`] reorders B-frames into ascending
//! presentation-timestamp order and carries stream color primaries, range, transfer function,
//! display rotation, and clean aperture (`CropRect`) on every [`VideoFrame`]. Android decodes
//! through `MediaCodec`. macOS and iOS decode through VideoToolbox and output [`VideoFrame`]s
//! only. Other platforms report the decoder as unavailable.

#[cfg(any(target_os = "android", target_os = "macos", target_os = "ios"))]
mod admission;
mod annex_b;
mod capabilities;
mod codec;
mod color;
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
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod reorder_buffer;
mod split_access_units;
#[cfg(any(test, target_os = "android", target_os = "macos", target_os = "ios"))]
mod sps;
mod stats;
#[cfg(not(any(target_os = "android", target_os = "macos", target_os = "ios")))]
mod unavailable;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod video_toolbox;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod video_toolbox_capabilities;

pub use capabilities::{
    DecoderCapabilities, DecoderCapabilitySource, set_platform_decoder_capabilities,
};
pub use codec::VideoCodec;
pub use color::{ColorPrimaries, ColorRange, ColorTransfer, VideoColorInfo, VideoRotation};
pub use config::{AccessUnit, DecodeOrder, DecoderConfig, DecoderOutput};
pub use decoder::{DecodedFrames, VideoDecoder};
pub use error::DecoderError;
pub use frame::{CropRect, FrameInfo, VideoFrame};
pub use split_access_units::split_access_units;
pub use stats::DecoderStats;
