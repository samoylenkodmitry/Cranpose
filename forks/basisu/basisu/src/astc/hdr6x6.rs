//! Raw ASTC HDR 6x6 decode targets: the uncompressed half-float/RGB9E5
//! rasters, and BC6H via a 2x2-source-block reblocker that re-encodes 12x12
//! half-float tiles as 3x3 4x4 BC6H blocks with `fast_encode_bc6h`. Any block
//! that fails to unpack or decode aborts the whole image.

use crate::astc::{decode, unpack};
use crate::fastenc::bc6h_enc::{fast_encode_bc6h, FastBc6hParams};

/// Transcode to half-float texels with `ncomp` components (3 for RGB_HALF, 4
/// for RGBA_HALF), written into `out`. The output raster is `lw` x `lh` texels
/// with the default pitch semantics (row pitch = orig width, rows = orig
/// height), each block clipped by `min(6, pitch - block_x * 6)` per axis.
pub(crate) fn transcode_half(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    ncomp: usize,
    out: &mut [u8],
) -> Option<()> {
    if out.len() != (lw as usize) * (lh as usize) * 2 * ncomp {
        return None;
    }
    out.fill(0);
    for block_y in 0..by {
        for block_x in 0..bx {
            let b = ((block_x + block_y * bx) as usize) * 16;
            let src: &[u8; 16] = img[b..b + 16].try_into().ok()?;
            let log = unpack::unpack_block(src, 6, 6)?;
            let mut texels = [[0u16; 4]; 36];
            decode::decode_block_hdr16(&log, 6, 6, &mut texels)?;

            let max_x = 6.min(lw - block_x * 6);
            let max_y = 6.min(lh - block_y * 6);
            for y in 0..max_y {
                for x in 0..max_x {
                    let t = &texels[(x + y * 6) as usize];
                    let o = ((block_x * 6 + x) + (block_y * 6 + y) * lw) as usize * 2 * ncomp;
                    for c in 0..ncomp {
                        out[o + 2 * c..o + 2 * c + 2].copy_from_slice(&t[c].to_le_bytes());
                    }
                }
            }
        }
    }
    Some(())
}

/// Transcode to uncompressed RGB9E5 (one little-endian 32-bit word per
/// texel), written into `out`, with the same pitch and clip semantics as the
/// half-float targets.
pub(crate) fn transcode_9e5(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    out: &mut [u8],
) -> Option<()> {
    if out.len() != (lw as usize) * (lh as usize) * 4 {
        return None;
    }
    out.fill(0);
    for block_y in 0..by {
        for block_x in 0..bx {
            let b = ((block_x + block_y * bx) as usize) * 16;
            let src: &[u8; 16] = img[b..b + 16].try_into().ok()?;
            let log = unpack::unpack_block(src, 6, 6)?;
            let mut texels = [0u32; 36];
            decode::decode_block_9e5(&log, 6, 6, &mut texels)?;

            let max_x = 6.min(lw - block_x * 6);
            let max_y = 6.min(lh - block_y * 6);
            for y in 0..max_y {
                for x in 0..max_x {
                    let o = ((block_x * 6 + x) + (block_y * 6 + y) * lw) as usize * 4;
                    out[o..o + 4].copy_from_slice(&texels[(x + y * 6) as usize].to_le_bytes());
                }
            }
        }
    }
    Some(())
}

/// Transcode to BC6H unsigned (16 bytes per 4x4 output block over the
/// `ceil(lw/4) x ceil(lh/4)` grid), written into `out`. Source blocks are
/// processed in 2x2
/// groups: their texels land in a 12x12 half-float tile whose 4x4 sub-blocks
/// (up to 3x3 of them, clamped to the destination grid) are re-encoded with
/// `fast_encode_bc6h`. Reads past the group's decoded extent clamp to its
/// last texel, and a group's tile starts at destination block
/// `(src_block * 6) >> 2`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn transcode_bc6h(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    high_quality: bool,
    out: &mut [u8],
) -> Option<()> {
    let ndx = lw.div_ceil(4);
    let ndy = lh.div_ceil(4);
    if out.len() != (ndx as usize) * (ndy as usize) * 16 {
        return None;
    }
    out.fill(0);

    let params = FastBc6hParams {
        max_2subset_pats_to_try: high_quality as u32,
        ..FastBc6hParams::default()
    };

    let mut unpacked = [[[0u16; 3]; 12]; 12]; // [y][x][c]
    let mut src_block_y = 0;
    while src_block_y < by {
        let inner_y = 2.min(by - src_block_y);
        let mut src_block_x = 0;
        while src_block_x < bx {
            let inner_x = 2.min(bx - src_block_x);

            for iy in 0..inner_y {
                for ix in 0..inner_x {
                    let b = (((src_block_y + iy) * bx + (src_block_x + ix)) as usize) * 16;
                    let src: &[u8; 16] = img[b..b + 16].try_into().ok()?;
                    let log = unpack::unpack_block(src, 6, 6)?;
                    let mut texels = [[0u16; 4]; 36];
                    decode::decode_block_hdr16(&log, 6, 6, &mut texels)?;
                    for y in 0..6usize {
                        for x in 0..6usize {
                            let t = &texels[x + y * 6];
                            unpacked[iy as usize * 6 + y][ix as usize * 6 + x] = [t[0], t[1], t[2]];
                        }
                    }
                }
            }

            let dst_block_x = (src_block_x * 6) >> 2;
            let dst_block_y = (src_block_y * 6) >> 2;
            let inner_dst_x = 3.min(ndx - dst_block_x);
            let inner_dst_y = 3.min(ndy - dst_block_y);

            for dy in 0..inner_dst_y {
                for dx in 0..inner_dst_x {
                    let mut pixels = [0u16; 48];
                    for y in 0..4u32 {
                        let sy = (dy * 4 + y).min(inner_y * 6 - 1) as usize;
                        for x in 0..4u32 {
                            let sx = (dx * 4 + x).min(inner_x * 6 - 1) as usize;
                            let p = &unpacked[sy][sx];
                            let o = ((y * 4 + x) * 3) as usize;
                            pixels[o..o + 3].copy_from_slice(p);
                        }
                    }
                    let mut block = [0u8; 16];
                    fast_encode_bc6h(&pixels, &mut block, &params);
                    let o = (((dst_block_y + dy) * ndx + (dst_block_x + dx)) as usize) * 16;
                    out[o..o + 16].copy_from_slice(&block);
                }
            }

            src_block_x += 2;
        }
        src_block_y += 2;
    }
    Some(())
}
