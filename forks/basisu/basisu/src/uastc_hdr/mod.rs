//! The UASTC HDR 4x4 source codec (`basis_tex_format::cUASTC_HDR_4x4`). Every
//! 16-byte block is a valid, restricted ASTC HDR 4x4 block (CEM 7 or 11, one
//! or two partitions, no dual plane, HDR void extents), so the
//! ASTC_HDR_4x4_RGBA target needs no codec at all: each block is copied
//! verbatim with no validation. The uncompressed targets (RGB/RGBA
//! half-float and RGB9E5) unpack, dequantize, and decode each physical ASTC
//! block through the shared block-size-generic machinery in [`crate::astc`].
//! The BC6H target unpacks the same way and repacks endpoints and weights
//! into a BC6H block (`bc6h`, `bc6h_tables`).

pub mod bc6h;
pub mod bc6h_tables;

use crate::astc::{decode, unpack};

/// Pass-through transcode to ASTC HDR 4x4:
/// copy each 16-byte source block into `out` unchanged. The only checks are
/// an input-size gate (the data must cover `num_blocks_x * num_blocks_y`
/// blocks) and the output-size gate; block contents are never inspected.
/// The output row pitch defaults to `num_blocks_x` blocks, which makes the
/// row-major block copy one contiguous byte copy.
pub fn transcode_astc_hdr_4x4(img: &[u8], bx: u32, by: u32, out: &mut [u8]) -> Option<()> {
    let total = (bx as usize).checked_mul(by as usize)?.checked_mul(16)?;
    if img.len() < total {
        return None;
    }
    if out.len() != total {
        return None;
    }
    out.copy_from_slice(&img[..total]);
    Some(())
}

/// Transcode to BC6H unsigned (target `cTFBC6H`, 16 bytes per 4x4 block,
/// block row pitch like the ASTC pass-through target): unpack each block and
/// repack its endpoints and weights into a BC6H block. Any block outside the
/// UASTC HDR envelope aborts the whole image.
pub fn transcode_bc6h(img: &[u8], bx: u32, by: u32, out: &mut [u8]) -> Option<()> {
    let total = (bx as usize).checked_mul(by as usize)?.checked_mul(16)?;
    if img.len() < total {
        return None;
    }
    if out.len() != total {
        return None;
    }
    out.fill(0);
    for (dst, src) in out.chunks_exact_mut(16).zip(img.chunks_exact(16)) {
        let src: &[u8; 16] = src.try_into().ok()?;
        let log = unpack::unpack_block(src, 4, 4)?;
        *<&mut [u8; 16]>::try_from(dst).ok()? = bc6h::transcode_block_bc6h(&log)?;
    }
    Some(())
}

/// Transcode to half-float texels with `ncomp` components (3 for RGB_HALF, 4
/// for RGBA_HALF; a UASTC HDR block always decodes an alpha of 1.0), written
/// into `out`. The output raster is `lw` x `lh` texels with default pitch
/// semantics (row pitch equal to the image width, one row per texel row), each
/// block clipped by `min(4, pitch - block_x * 4)` per axis. Any block that
/// fails to unpack or decode aborts the whole image.
fn transcode_half(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    ncomp: usize,
    out: &mut [u8],
) -> Option<()> {
    let total = (bx as usize).checked_mul(by as usize)?.checked_mul(16)?;
    if img.len() < total {
        return None;
    }
    if out.len() != (lw as usize) * (lh as usize) * 2 * ncomp {
        return None;
    }
    out.fill(0);
    for block_y in 0..by {
        for block_x in 0..bx {
            let b = ((block_x + block_y * bx) as usize) * 16;
            let src: &[u8; 16] = img[b..b + 16].try_into().ok()?;
            let log = unpack::unpack_block(src, 4, 4)?;
            let mut texels = [[0u16; 4]; 16];
            decode::decode_block_hdr16(&log, 4, 4, &mut texels)?;

            let max_x = 4.min(lw - block_x * 4);
            let max_y = 4.min(lh - block_y * 4);
            for y in 0..max_y {
                for x in 0..max_x {
                    let t = &texels[(x + y * 4) as usize];
                    let o = ((block_x * 4 + x) + (block_y * 4 + y) * lw) as usize * 2 * ncomp;
                    for c in 0..ncomp {
                        out[o + 2 * c..o + 2 * c + 2].copy_from_slice(&t[c].to_le_bytes());
                    }
                }
            }
        }
    }
    Some(())
}

/// Transcode to uncompressed RGB half floats (target `cTFRGB_HALF`, 6 bytes
/// per texel), written into `out`.
pub fn transcode_rgb_half(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    out: &mut [u8],
) -> Option<()> {
    transcode_half(img, bx, by, lw, lh, 3, out)
}

/// Transcode to uncompressed RGBA half floats (target `cTFRGBA_HALF`, 8 bytes
/// per texel; alpha decodes to 1.0 = 0x3C00 for the UASTC HDR CEMs), written
/// into `out`.
pub fn transcode_rgba_half(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    out: &mut [u8],
) -> Option<()> {
    transcode_half(img, bx, by, lw, lh, 4, out)
}

/// Transcode to uncompressed RGB9E5 (target `cTFRGB_9E5`, one little-endian
/// 32-bit word per texel), written into `out`, with the same pitch and clip
/// semantics as the half-float targets.
pub fn transcode_rgb_9e5(
    img: &[u8],
    bx: u32,
    by: u32,
    lw: u32,
    lh: u32,
    out: &mut [u8],
) -> Option<()> {
    let total = (bx as usize).checked_mul(by as usize)?.checked_mul(16)?;
    if img.len() < total {
        return None;
    }
    if out.len() != (lw as usize) * (lh as usize) * 4 {
        return None;
    }
    out.fill(0);
    for block_y in 0..by {
        for block_x in 0..bx {
            let b = ((block_x + block_y * bx) as usize) * 16;
            let src: &[u8; 16] = img[b..b + 16].try_into().ok()?;
            let log = unpack::unpack_block(src, 4, 4)?;
            let mut texels = [0u32; 16];
            decode::decode_block_9e5(&log, 4, 4, &mut texels)?;

            let max_x = 4.min(lw - block_x * 4);
            let max_y = 4.min(lh - block_y * 4);
            for y in 0..max_y {
                for x in 0..max_x {
                    let o = ((block_x * 4 + x) + (block_y * 4 + y) * lw) as usize * 4;
                    out[o..o + 4].copy_from_slice(&texels[(x + y * 4) as usize].to_le_bytes());
                }
            }
        }
    }
    Some(())
}
