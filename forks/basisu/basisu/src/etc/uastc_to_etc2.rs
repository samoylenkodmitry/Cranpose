//! UASTC -> ETC2_RGBA transcode: the EAC alpha block
//! (`transcode_uastc_to_etc2_eac_a8`) followed by the ETC1 color block, packed
//! together into a 16-byte ETC2 block.

use super::block::DecoderEtcBlock;
use super::tables::{EAC_MODIFIER_TABLE, ETC2_EAC_A8_SEL4};
use super::uastc_to_etc1::transcode_to_etc1;
use crate::color::Color32;
use crate::uastc::tables::HAS_ALPHA;
use crate::uastc::unpack::{unpack_pixels, unpack_to_block, UnpackedUastcBlock};
use crate::uastc::UASTC_MODE_INDEX_SOLID_COLOR;

/// Index of the most negative modifier in every EAC modifier table row.
const ETC2_EAC_MIN_VALUE_SELECTOR: usize = 3;
/// Index of the most positive modifier in every EAC modifier table row.
const ETC2_EAC_MAX_VALUE_SELECTOR: usize = 7;

/// Clamp a signed value into the 0..=255 byte range.
#[inline]
fn clamp255(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// Encode the alpha plane to an 8-byte EAC block: the base byte, then
/// `multiplier << 4 | table`, then 6 selector bytes.
fn transcode_to_eac_a8(u: &UnpackedUastcBlock, pixels: &[Color32; 16]) -> [u8; 8] {
    let (base, table, multiplier, selectors);

    if HAS_ALPHA[u.mode as usize] == 0 || u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        let a = if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
            u.solid_color.a() as u32
        } else {
            255
        };
        // Constant alpha encodes exactly: entry 4 of modifier table 13 is 0,
        // so base + 0 * multiplier reproduces `a` at every texel.
        base = a;
        table = 13u32;
        multiplier = 1u32;
        selectors = ETC2_EAC_A8_SEL4;
    } else {
        let mut min_a = 255u32;
        let mut max_a = 0u32;
        for p in pixels {
            min_a = min_a.min(p.a() as u32);
            max_a = max_a.max(p.a() as u32);
        }
        if min_a == max_a {
            // same exact constant-value encoding as the no-alpha case above
            base = min_a;
            table = 13;
            multiplier = 1;
            selectors = ETC2_EAC_A8_SEL4;
        } else {
            // The UASTC block's ETC2 hint bits carry a precomputed modifier
            // table index (low nibble) and multiplier (high bits).
            table = u.etc2_hints & 0xF;
            multiplier = u.etc2_hints >> 4;
            let tbl = &EAC_MODIFIER_TABLE[table as usize];
            // Place the base value so the table's full modifier span maps onto
            // [min_a, max_a]: t is the fraction of the span below zero.
            let range = (tbl[ETC2_EAC_MAX_VALUE_SELECTOR] as i32
                - tbl[ETC2_EAC_MIN_VALUE_SELECTOR] as i32) as f32;
            let t = (0 - tbl[ETC2_EAC_MIN_VALUE_SELECTOR] as i32) as f32 / range;
            // Linear interpolation a + (b - a) * t, kept as separate f32
            // operations (no fused multiply-add) so the rounded result is the
            // same on every platform.
            let center =
                crate::mathf::roundf(min_a as f32 + (max_a as f32 - min_a as f32) * t) as i32;
            base = center as u32;

            let mut vals = [0u32; 8];
            for (j, v) in vals.iter_mut().enumerate() {
                *v = clamp255(center + tbl[j] as i32 * multiplier as i32) as u32;
            }

            // EAC stores selectors column-major, MSB-first in a 48-bit field;
            // walk the pixels in that order.
            let mut sels = 0u64;
            for i in 0..16u32 {
                let a = pixels[((i & 3) * 4 + (i >> 2)) as usize].a() as u32;
                let mut min_err = u32::MAX;
                for (j, &vj) in vals.iter().enumerate() {
                    // packing the error above the 3-bit index makes min() pick
                    // the lowest-error selector, ties going to the lower index
                    let err = ((vj as i32 - a as i32).unsigned_abs() << 3) | j as u32;
                    min_err = min_err.min(err);
                }
                let best = (min_err & 7) as u64;
                sels |= best << (45 - i * 3);
            }
            selectors = [
                (sels >> 40) as u8,
                (sels >> 32) as u8,
                (sels >> 24) as u8,
                (sels >> 16) as u8,
                (sels >> 8) as u8,
                sels as u8,
            ];
        }
    }

    let mut out = [0u8; 8];
    out[0] = base as u8;
    out[1] = ((table & 0xF) | ((multiplier & 0xF) << 4)) as u8;
    out[2..8].copy_from_slice(&selectors);
    out
}

/// Transcode a 16-byte UASTC block to a 16-byte ETC2 block (EAC alpha in bytes
/// 0..8, ETC1 color in bytes 8..16).
pub fn transcode_uastc_to_etc2_rgba(src: &[u8; 16]) -> Option<[u8; 16]> {
    let u = unpack_to_block(src, false, true)?;
    // Solid blocks never read the pixel array (both encoders take the color
    // from u.solid_color directly), so a zeroed placeholder suffices.
    let pixels = if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        [Color32::default(); 16]
    } else {
        unpack_pixels(u.mode, u.common_pattern, u.solid_color, &u.astc, false)
    };

    let eac = transcode_to_eac_a8(&u, &pixels);
    let mut etc1 = DecoderEtcBlock::new();
    transcode_to_etc1(&u, &pixels, &mut etc1);

    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&eac);
    out[8..16].copy_from_slice(&etc1.m_bytes);
    Some(out)
}
