//! Windows-only video and GPU platform integrations.

#[cfg(all(feature = "renderer-wgpu", feature = "video"))]
pub(crate) mod d3d12_converter;
#[cfg(feature = "video")]
pub(crate) mod player;
#[cfg(feature = "video")]
mod player_source;
