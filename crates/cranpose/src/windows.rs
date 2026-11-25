//! Windows-only video and GPU platform integrations.

#[cfg(all(feature = "renderer-wgpu", feature = "video"))]
pub(crate) mod d3d12_converter;
