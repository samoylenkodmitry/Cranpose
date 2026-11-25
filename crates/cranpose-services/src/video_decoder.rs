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
//! through `MediaCodec`. macOS and iOS decode through VideoToolbox, Windows decodes through
//! Media Foundation into D3D11 shared textures, and Linux decodes through VA-API DMABUF (with
//! explicit software fallback), outputting [`VideoFrame`]s only. Other platforms report the
//! decoder as unavailable.

#[cfg(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
))]
mod admission;
mod annex_b;
mod capabilities;
mod codec;
mod color;
mod config;
#[cfg(target_os = "windows")]
mod d3d11_frame;
mod decoder;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod decompression_session;
mod error;
mod frame;
mod frame_queue;
#[cfg(target_os = "android")]
mod image_reader;
#[cfg(target_os = "linux")]
mod linux_decoder;
#[cfg(target_os = "linux")]
mod linux_frame;
#[cfg(target_os = "android")]
mod media_codec;
#[cfg(target_os = "windows")]
mod media_foundation;
#[cfg(target_os = "windows")]
mod media_foundation_capabilities;
#[cfg(target_os = "windows")]
mod media_foundation_transform;
#[cfg(any(
    test,
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
))]
mod pending_units;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod pixel_buffer;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod reorder_buffer;
mod split_access_units;
#[cfg(any(
    test,
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
))]
mod sps;
mod stats;
#[cfg(not(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "linux",
    all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
)))]
mod unavailable;
#[cfg(target_os = "linux")]
mod vaapi_capabilities;
#[cfg(target_os = "linux")]
mod vaapi_loader;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod video_toolbox;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod video_toolbox_capabilities;
#[cfg(all(feature = "video-web", target_arch = "wasm32", target_os = "unknown"))]
mod web_codecs;
#[cfg(all(feature = "video-web", target_arch = "wasm32", target_os = "unknown"))]
mod web_codecs_capabilities;
#[cfg(all(feature = "video-web", target_arch = "wasm32", target_os = "unknown"))]
mod web_frame;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod worker_mailbox;

pub use capabilities::{
    DecoderCapabilities, DecoderCapabilitySource, set_platform_decoder_capabilities,
};
pub use codec::VideoCodec;
pub use color::{ColorPrimaries, ColorRange, ColorTransfer, VideoColorInfo, VideoRotation};
pub use config::{AccessUnit, DecodeOrder, DecoderConfig, DecoderOutput};
#[cfg(target_os = "windows")]
pub use d3d11_frame::{DxgiPixelFormat, DxgiSharedFrame};
pub use decoder::{DecodedFrames, VideoDecoder};
pub use error::DecoderError;
pub use frame::{CropRect, FrameInfo, VideoFrame};
pub use frame_queue::FrameProducer;
#[cfg(target_os = "linux")]
pub use linux_frame::{
    DmaBufFrame, DmaBufPlane, LinuxFrameMemory, LinuxPixelFormat, SoftwareBiPlanarPlanes,
};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use pixel_buffer::video_frame_from_pixel_buffer_ptr;
pub use split_access_units::split_access_units;
pub use stats::DecoderStats;
#[cfg(all(feature = "video-web", target_arch = "wasm32", target_os = "unknown"))]
pub use web_frame::WebVideoSource;
