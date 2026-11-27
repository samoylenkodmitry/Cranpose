//! `convert_etc1s_to_atc` plus the ETC1S to ATC slice/image emit. Maps a
//! decoded ETC1S endpoint+selector to an 8-byte ATC color block. ATC packs its
//! endpoints differently from BC1: an RGB555 low color whose spare top bit is
//! the block mode flag (left 0 here, ATC's standard 4-color interpolated
//! mode), then an RGB565 high color (see `set_low_color`/`set_high_color`).
//!
//! `ATC_RGBA` (16 bytes/block) = a DXT5A-style alpha block (bytes 0..8) followed
//! by the ATC color block (bytes 8..16). When the source has no alpha slice,
//! the alpha half is an opaque DXT5A block (endpoints 255/255, all-zero
//! selectors).

use super::etc1s::{Endpoint, Etc1sTranscoder, Selector};
use crate::etc::block::DecoderEtcBlock;
use crate::tables::atc_55::G_ETC1S_TO_ATC_55;
use crate::tables::atc_56::G_ETC1S_TO_ATC_56;
use crate::tables::dxt5a::G_ETC1_G_TO_DXT5A;
use alloc::vec::Vec;

const NUM_RANGES: usize = 6;
const NUM_MAPPINGS: usize = 10;
const ATC_IDENTITY_SELECTOR_MAPPING_INDEX: usize = 6;

/// The `{low, high}` selector ranges (6 entries).
static ATC_SELECTOR_RANGES: [[u32; 2]; NUM_RANGES] =
    [[0, 3], [1, 3], [0, 2], [1, 2], [2, 3], [0, 1]];

/// The candidate 2-bit selector remaps.
static ATC_SELECTOR_MAPPINGS: [[u8; 4]; NUM_MAPPINGS] = [
    [0, 0, 1, 1],
    [0, 0, 1, 2],
    [0, 0, 1, 3],
    [0, 0, 2, 3],
    [0, 1, 1, 1],
    [0, 1, 2, 2],
    [0, 1, 2, 3], // 6 - identity
    [0, 2, 3, 3],
    [1, 2, 2, 2],
    [1, 2, 3, 3],
];

/// Set the low color word: RGB555, r at bit 10, g at bit 5, b at bit 0.
/// Bit 15 is the block mode flag; leaving it 0 selects the 4-color
/// interpolated mode.
fn set_low_color(out: &mut [u8; 8], r: u32, g: u32, b: u32) {
    let x = (r << 10) | (g << 5) | b;
    out[0] = (x & 0xFF) as u8;
    out[1] = ((x >> 8) & 0xFF) as u8;
}

/// Set the high color word: RGB565, r(5) at bit 11, g(6) at bit 5, b(5)
/// at bit 0. All 16 bits carry color; the mode flag lives in the low word.
fn set_high_color(out: &mut [u8; 8], r: u32, g: u32, b: u32) {
    let x = (r << 11) | (g << 5) | b;
    out[2] = (x & 0xFF) as u8;
    out[3] = ((x >> 8) & 0xFF) as u8;
}

/// Single-color match for a block whose texels all use selector 1: for one
/// color channel, find the `(lo, hi)` endpoint pair whose sel==1 interpolated
/// value best approximates the 8-bit value `i`. `size1` is the high endpoint's
/// quantization count (32 for a 5-bit channel, 64 for a 6-bit one); the low
/// endpoint is always 5-bit, expanded to 8 bits by `<<3|>>2`. Brute-force with
/// lo outer over `0..32` and hi inner over `0..size1`; the earliest pair
/// reaching the lowest absolute error wins (error seeded at 256). Returns
/// `(lo, hi)`.
fn atc_match_equals_1(i: u32, size1: u32) -> (u32, u32) {
    let mut best_lo = 0u32;
    let mut best_hi = 0u32;
    let mut lowest_e = 256i32;
    let i = i as i32;
    for lo in 0..32u32 {
        let lo_e = ((lo << 3) | (lo >> 2)) as i32;
        for hi in 0..size1 {
            let hi_e = if size1 == 32 {
                ((hi << 3) | (hi >> 2)) as i32
            } else {
                ((hi << 2) | (hi >> 4)) as i32
            };
            // sel == 1: e = |(lo_e*5 + hi_e*3)/8 - i|
            let e = ((lo_e * 5 + hi_e * 3) / 8 - i).abs();
            if e < lowest_e {
                best_lo = lo;
                best_hi = hi;
                lowest_e = e;
            }
        }
    }
    (best_lo, best_hi)
}

/// Convert one ETC1S color block to an 8-byte ATC color block.
pub fn convert_etc1s_to_atc(ep: &Endpoint, sel: &Selector) -> [u8; 8] {
    let low_selector = sel.lo_selector as u32;
    let high_selector = sel.hi_selector as u32;
    let inten_table = ep.inten5 as u32;
    let mut out = [0u8; 8];

    if low_selector == high_selector {
        // Solid color: single-color match endpoints, selectors all 0x55 (= 1).
        let (r, g, b) =
            DecoderEtcBlock::get_block_color5(ep.color5, inten_table, low_selector as usize);
        let (r_lo, r_hi) = atc_match_equals_1(r, 32);
        let (g_lo, g_hi) = atc_match_equals_1(g, 64);
        let (b_lo, b_hi) = atc_match_equals_1(b, 32);
        set_low_color(&mut out, r_lo, g_lo, b_lo);
        set_high_color(&mut out, r_hi, g_hi, b_hi);
        out[4] = 0x55;
        out[5] = 0x55;
        out[6] = 0x55;
        out[7] = 0x55;
        return out;
    }

    if inten_table >= 7 && sel.num_unique_selectors == 2 && low_selector == 0 && high_selector == 3
    {
        // High-intensity 0/3 special case: endpoints from the sel==3 match
        // single-color match tables, selectors verbatim.
        let bc = DecoderEtcBlock::get_block_colors5(ep.color5, inten_table);
        let (r0, g0, b0) = (bc[0].r() as u32, bc[0].g() as u32, bc[0].b() as u32);
        let (r1, g1, b1) = (bc[3].r() as u32, bc[3].g() as u32, bc[3].b() as u32);
        set_low_color(
            &mut out,
            atc_match3(r0, 32),
            atc_match3(g0, 32),
            atc_match3(b0, 32),
        );
        set_high_color(
            &mut out,
            atc_match3(r1, 32),
            atc_match3(g1, 64),
            atc_match3(b1, 32),
        );
        out[4] = sel.selectors[0];
        out[5] = sel.selectors[1];
        out[6] = sel.selectors[2];
        out[7] = sel.selectors[3];
        return out;
    }

    let srt = atc_selector_range_index(low_selector, high_selector);
    let it = inten_table as usize;
    let base_r =
        (it * 32 + ep.color5.r() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
    let base_g =
        (it * 32 + ep.color5.g() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
    let base_b =
        (it * 32 + ep.color5.b() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;

    let mut best_err = u32::MAX;
    let mut best_mapping = 0usize;
    for m in 0..NUM_MAPPINGS {
        let total = G_ETC1S_TO_ATC_55[base_r + m].m_err as u32
            + G_ETC1S_TO_ATC_56[base_g + m].m_err as u32
            + G_ETC1S_TO_ATC_55[base_b + m].m_err as u32;
        if total < best_err {
            best_err = total;
            best_mapping = m;
        }
    }

    let tr = G_ETC1S_TO_ATC_55[base_r + best_mapping];
    let tg = G_ETC1S_TO_ATC_56[base_g + best_mapping];
    let tb = G_ETC1S_TO_ATC_55[base_b + best_mapping];
    set_low_color(&mut out, tr.m_lo as u32, tg.m_lo as u32, tb.m_lo as u32);
    set_high_color(&mut out, tr.m_hi as u32, tg.m_hi as u32, tb.m_hi as u32);

    if best_mapping == ATC_IDENTITY_SELECTOR_MAPPING_INDEX {
        out[4] = sel.selectors[0];
        out[5] = sel.selectors[1];
        out[6] = sel.selectors[2];
        out[7] = sel.selectors[3];
    } else {
        let xlat = &ATC_SELECTOR_MAPPINGS[best_mapping];
        for (i, &sel_bits) in sel.selectors.iter().enumerate() {
            let sel_bits = sel_bits as u32;
            let mut atc_sels = 0u32;
            for x in 0..4u32 {
                let x_shift = x * 2;
                atc_sels |= (xlat[((sel_bits >> x_shift) & 3) as usize] as u32) << x_shift;
            }
            out[4 + i] = atc_sels as u8;
        }
    }

    out
}

/// The selector range index, `[low][high]`.
fn atc_selector_range_index(low: u32, high: u32) -> usize {
    for (i, r) in ATC_SELECTOR_RANGES.iter().enumerate() {
        if low == r[0] && high == r[1] {
            return i;
        }
    }
    0
}

/// Single-color match for the sel==3 case: the low endpoint is fixed at 0, so
/// only the high endpoint is searched for the closest match to the 8-bit value
/// `i`. `size1` is the high endpoint's quantization count (32 for a 5-bit
/// channel, 64 for a 6-bit one). Error is `|hi_e - i|`, the earliest high
/// endpoint reaching the lowest error wins (seeded at 256), and it is returned.
fn atc_match3(i: u32, size1: u32) -> u32 {
    let mut best_hi = 0u32;
    let mut lowest_e = 256i32;
    let i = i as i32;
    for hi in 0..size1 {
        let hi_e = if size1 == 32 {
            ((hi << 3) | (hi >> 2)) as i32
        } else {
            ((hi << 2) | (hi >> 4)) as i32
        };
        // sel == 3: e = |hi_e - i|
        let e = (hi_e - i).abs();
        if e < lowest_e {
            best_hi = hi;
            lowest_e = e;
        }
    }
    best_hi
}

/// The `{low, high}` selector ranges (4 entries).
static DXT5A_SELECTOR_RANGES: [[u32; 2]; 4] = [[0, 3], [1, 3], [0, 2], [1, 2]];

/// Write the 3-bit selector for texel (`x`, `y`) into the 6-byte selector
/// buffer of a DXT5A block. Selectors are packed contiguously at bit
/// `(y*4 + x) * 3`, so a value can straddle two bytes.
fn dxt5a_set_selector(sels: &mut [u8; 6], x: u32, y: u32, val: u32) {
    let selector_index = y * 4 + x;
    let bit_index = selector_index * 3;
    let byte_index = (bit_index >> 3) as usize;
    let bit_ofs = bit_index & 7;

    let mut v = sels[byte_index] as u32;
    if byte_index < 5 {
        v |= (sels[byte_index + 1] as u32) << 8;
    }
    v &= !(7 << bit_ofs);
    v |= val << bit_ofs;
    sels[byte_index] = v as u8;
    if byte_index < 5 {
        sels[byte_index + 1] = (v >> 8) as u8;
    }
}

/// Convert an ETC1S grayscale (red channel) block to an 8-byte DXT5A/BC4 alpha
/// block, used here as the alpha half of an ATC_RGBA block: two 8-bit endpoints
/// (low and high alpha) followed by sixteen 3-bit selectors.
fn convert_etc1s_to_dxt5a(ep: &Endpoint, sel: &Selector) -> [u8; 8] {
    let low_selector = sel.lo_selector as u32;
    let high_selector = sel.hi_selector as u32;
    let inten_table = ep.inten5 as u32;
    let mut out = [0u8; 8];

    if low_selector == high_selector {
        let r = DecoderEtcBlock::get_block_color5_r(ep.color5, inten_table, low_selector as usize);
        out[0] = r as u8; // low alpha endpoint
        out[1] = r as u8; // high alpha endpoint; selectors stay all zero
        return out;
    }

    let mut sels = [0u8; 6];

    if sel.num_unique_selectors == 2 {
        let bc = DecoderEtcBlock::get_block_colors5(ep.color5, inten_table);
        let r0 = bc[low_selector as usize].r() as u32;
        let r1 = bc[high_selector as usize].r() as u32;
        out[0] = r0 as u8;
        out[1] = r1 as u8;
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = sel.get_selector(x, y);
                let ds = if s == high_selector { 1 } else { 0 };
                dxt5a_set_selector(&mut sels, x, y, ds);
            }
        }
        out[2..8].copy_from_slice(&sels);
        return out;
    }

    let mut srt = 0usize;
    for (i, r) in DXT5A_SELECTOR_RANGES.iter().enumerate() {
        if low_selector == r[0] && high_selector == r[1] {
            srt = i;
            break;
        }
    }

    let entry = G_ETC1_G_TO_DXT5A[(ep.color5.r() as usize + inten_table as usize * 32) * 4 + srt];
    out[0] = entry.m_lo;
    out[1] = entry.m_hi;
    for y in 0..4u32 {
        for x in 0..4u32 {
            let s = sel.get_selector(x, y);
            let ds = (entry.m_trans as u32 >> (s * 3)) & 7;
            dxt5a_set_selector(&mut sels, x, y, ds);
        }
    }
    out[2..8].copy_from_slice(&sels);
    out
}

/// A fully opaque DXT5A block: both endpoints 255, all selectors 0, so every
/// texel decodes to 255.
const OPAQUE_DXT5A: [u8; 8] = [255, 255, 0, 0, 0, 0, 0, 0];

impl Etc1sTranscoder {
    /// Transcode a slice to ATC RGB: 8-byte ATC color blocks, written into
    /// `out` (exactly `num_blocks_x*num_blocks_y*8` bytes).
    /// `None` on a corrupt stream.
    pub fn transcode_slice_atc(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != indices.len() * 8 {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block =
                convert_etc1s_to_atc(&self.endpoints[ei as usize], &self.selectors[si as usize]);
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        Some(())
    }

    /// ETC1S -> ATC_RGBA with no alpha slice: opaque DXT5A alpha block (bytes
    /// 0..8) + ATC color (bytes 8..16), written into `out` (exactly
    /// `num_blocks_x*num_blocks_y*16` bytes).
    pub fn transcode_slice_atc_rgba_opaque(
        &self,
        rgb_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let rgb = self.decode_slice_indices(rgb_slice, num_blocks_x, num_blocks_y, video)?;
        if out.len() != rgb.len() * 16 {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in rgb.iter().enumerate() {
            out[b * 16..b * 16 + 8].copy_from_slice(&OPAQUE_DXT5A);
            let color =
                convert_etc1s_to_atc(&self.endpoints[ei as usize], &self.selectors[si as usize]);
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&color);
        }
        Some(())
    }

    /// Combined ETC1S -> ATC_RGBA: 16-byte blocks of DXT5A alpha (from the alpha
    /// slice, bytes 0..8) + ATC color (from the RGB slice, bytes 8..16),
    /// written into `out` (exactly `num_blocks_x*num_blocks_y*16` bytes).
    /// `video`, if present, is the previous frame's (color, alpha) index slots
    /// for ETC1S video.
    pub fn transcode_image_atc_rgba(
        &self,
        rgb_slice: &[u8],
        alpha_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<(&mut Vec<u32>, &mut Vec<u32>)>,
        out: &mut [u8],
    ) -> Option<()> {
        let (vc, va) = match video {
            Some((c, a)) => (Some(c), Some(a)),
            None => (None, None),
        };
        let rgb = self.decode_slice_indices(rgb_slice, num_blocks_x, num_blocks_y, vc)?;
        let alpha = self.decode_slice_indices(alpha_slice, num_blocks_x, num_blocks_y, va)?;
        if out.len() != rgb.len() * 16 {
            return None;
        }
        out.fill(0);
        for (b, (&(rei, rsi), &(aei, asi))) in rgb.iter().zip(alpha.iter()).enumerate() {
            let a = convert_etc1s_to_dxt5a(
                &self.endpoints[aei as usize],
                &self.selectors[asi as usize],
            );
            let color =
                convert_etc1s_to_atc(&self.endpoints[rei as usize], &self.selectors[rsi as usize]);
            out[b * 16..b * 16 + 8].copy_from_slice(&a);
            out[b * 16 + 8..b * 16 + 16].copy_from_slice(&color);
        }
        Some(())
    }
}
