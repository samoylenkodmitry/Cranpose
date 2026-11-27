//! UASTC to BC3 (DXT5) and BC5 (3Dc). Both targets are pure compositions of the
//! UASTC-to-BC4 and UASTC-to-BC1 writers, so each can reuse those whole:
//!
//! - BC3 is the BC4 alpha block (channel 3) in bytes 0..8 followed by the BC1
//!   color block in bytes 8..16. The two writers each re-unpack the source UASTC
//!   block, but since both are pure functions of the block plus `high_quality`
//!   the result is identical to unpacking once and feeding both halves.
//! - BC5 is two BC4 blocks: channel `chan0` in bytes 0..8 and channel `chan1` in
//!   bytes 8..16. The UASTC dispatch passes `chan0 = 0` (red) and `chan1 = 3`
//!   (alpha) for a BC5 target.

use super::bc1::transcode_uastc_to_bc1;
use super::bc4::transcode_uastc_to_bc4;

/// Transcode one UASTC block to BC3: BC4 alpha (channel 3) plus BC1 color.
pub fn transcode_uastc_to_bc3(src: &[u8; 16], high_quality: bool) -> Option<[u8; 16]> {
    let alpha = transcode_uastc_to_bc4(src, high_quality, 3)?;
    let color = transcode_uastc_to_bc1(src, high_quality)?;
    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&alpha);
    out[8..16].copy_from_slice(&color);
    Some(out)
}

/// Transcode one UASTC block to BC5: two BC4 blocks, one from channel `chan0`
/// and one from channel `chan1`.
pub fn transcode_uastc_to_bc5(
    src: &[u8; 16],
    high_quality: bool,
    chan0: usize,
    chan1: usize,
) -> Option<[u8; 16]> {
    let b0 = transcode_uastc_to_bc4(src, high_quality, chan0)?;
    let b1 = transcode_uastc_to_bc4(src, high_quality, chan1)?;
    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&b0);
    out[8..16].copy_from_slice(&b1);
    Some(out)
}
