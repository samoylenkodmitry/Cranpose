//! ETC1S to BC7 mode-5 slice transcode (`convert_etc1s_to_bc7_m5_color`). Maps a
//! decoded ETC1S endpoint and selector to a BC7 mode-5 block, opaque with the
//! alpha endpoints forced to 255. The BC7 mode-5 selector ranges and mappings
//! are identical to the ASTC ones, so the `astc_tables` versions are reused here.

use super::astc_tables::{selector_range_index, SELECTOR_MAPPINGS};
use super::bc7_chroma::{chroma_filter_bc7_mode5, new_endpoint_grid, record_endpoint};
use super::etc1s::{Endpoint, Etc1sTranscoder, Selector};
use crate::etc::block::DecoderEtcBlock;
use crate::tables::bc7_m5_alpha::G_ETC1_G_TO_BC7_M5A;
use crate::tables::bc7_m5_color::G_ETC1_TO_BC7_M5_COLOR;
use crate::tables::bc7_m5_equals_1::G_BC7_M5_EQUALS_1;
use crate::uastc::bc7::set_block_bits;
use alloc::vec::Vec;

/// Number of (low, high) selector ranges the conversion tables are indexed by.
const NUM_RANGES: usize = 6;
/// Number of candidate selector mappings per range.
const NUM_MAPPINGS: usize = 10;

/// Clear `num_bits` at `ofs` then set them to `val` (LSB-first). Used to
/// overwrite the alpha fields of an already-built BC7 color block.
fn write_bits_clear(out: &mut [u8; 16], mut val: u32, mut num_bits: u32, mut ofs: u32) {
    while num_bits > 0 {
        let byte = (ofs >> 3) as usize;
        let bit = ofs & 7;
        let take = (8 - bit).min(num_bits);
        let mask = ((1u32 << take) - 1) as u8;
        out[byte] &= !(mask << bit);
        out[byte] |= ((val as u8) & mask) << bit;
        val >>= take;
        num_bits -= take;
        ofs += take;
    }
}

/// Overwrite a BC7 mode-5 block's two 8-bit alpha endpoints (m_a0 @ bit 50,
/// m_a1_0 @ 58, m_a1_1 @ 64).
fn set_bc7_alpha_endpoints(block: &mut [u8; 16], a0: u32, a1: u32) {
    write_bits_clear(block, a0, 8, 50);
    write_bits_clear(block, a1 & 63, 6, 58);
    write_bits_clear(block, a1 >> 6, 2, 64);
}

/// Overwrite the alpha plane of an existing BC7 mode-5 color block from a
/// decoded ETC1S (grayscale) endpoint+selector.
pub fn convert_etc1s_to_bc7_m5_alpha(block: &mut [u8; 16], ep: &Endpoint, sel: &Selector) {
    let low_selector = sel.lo_selector as usize;
    let high_selector = sel.hi_selector as u32;
    let inten_table = ep.inten5 as u32;

    if sel.num_unique_selectors == 1 {
        let r = DecoderEtcBlock::get_block_color5_r(ep.color5, inten_table, low_selector);
        set_bc7_alpha_endpoints(block, r, r);
        return;
    }

    if sel.num_unique_selectors == 2 {
        let bc = DecoderEtcBlock::get_block_colors5_g(ep.color5, inten_table);
        let mut a0 = bc[low_selector] as u32;
        let mut a1 = bc[high_selector as usize] as u32;
        let mut output_low_selector = 0u32;
        let mut output_bit_offset = 0u32;
        let mut output_bits = 0u32;
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = sel.get_selector(x, y);
                let mut os = if s == low_selector as u32 {
                    output_low_selector
                } else {
                    3 ^ output_low_selector
                };
                let mut num_bits = 2u32;
                if (x | y) == 0 {
                    // Texel (0,0) is the anchor and stores only one bit (its
                    // high bit is implied 0). If its selector would need that
                    // bit, swap the endpoints and invert the remaining
                    // selectors instead.
                    if os & 2 != 0 {
                        a0 = bc[high_selector as usize] as u32;
                        a1 = bc[low_selector] as u32;
                        output_low_selector = 3;
                        os = 0;
                    }
                    num_bits = 1;
                }
                output_bits |= os << output_bit_offset;
                output_bit_offset += num_bits;
            }
        }
        set_bc7_alpha_endpoints(block, a0, a1);
        write_bits_clear(block, output_bits, 31, 97);
        return;
    }

    let srt = selector_range_index()[low_selector][high_selector as usize] as usize;
    let idx = inten_table as usize * (32 * NUM_RANGES) + ep.color5.r() as usize * NUM_RANGES + srt;
    let t = G_ETC1_G_TO_BC7_M5A[idx];
    let mut a0 = t.m_lo as u32;
    let mut a1 = t.m_hi as u32;
    let mut selector_trans = t.m_trans as u32;
    let mut output_bit_offset = 0u32;
    let mut output_bits = 0u32;
    for y in 0..4u32 {
        for x in 0..4u32 {
            let s = sel.get_selector(x, y);
            let mut os = (selector_trans >> (s * 2)) & 3;
            let mut num_bits = 2u32;
            if (x | y) == 0 {
                // Anchor texel: only one stored bit, so a high bit here forces
                // an endpoint swap plus a full translation inversion.
                if os & 2 != 0 {
                    a0 = t.m_hi as u32;
                    a1 = t.m_lo as u32;
                    selector_trans ^= 0xFF;
                    os ^= 3;
                }
                num_bits = 1;
            }
            output_bits |= os << output_bit_offset;
            output_bit_offset += num_bits;
        }
    }
    set_bc7_alpha_endpoints(block, a0, a1);
    write_bits_clear(block, output_bits, 31, 97);
}

/// Assemble a BC7 mode-5 block from its 7-bit RGB endpoints (in struct field
/// order r0,r1,g0,g1,b0,b1), forcing opaque alpha, plus the 31-bit color index
/// field (written at bit 66).
fn build_bc7_m5(
    r0: u32,
    r1: u32,
    g0: u32,
    g1: u32,
    b0: u32,
    b1: u32,
    color_index_bits: u32,
) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut ofs = 0u32;
    set_block_bits(&mut out, 1 << 5, 6, &mut ofs); // m_mode (mode 5)
    set_block_bits(&mut out, 0, 2, &mut ofs); // m_rot
    set_block_bits(&mut out, r0, 7, &mut ofs);
    set_block_bits(&mut out, r1, 7, &mut ofs);
    set_block_bits(&mut out, g0, 7, &mut ofs);
    set_block_bits(&mut out, g1, 7, &mut ofs);
    set_block_bits(&mut out, b0, 7, &mut ofs);
    set_block_bits(&mut out, b1, 7, &mut ofs);
    set_block_bits(&mut out, 255, 8, &mut ofs); // m_a0
    set_block_bits(&mut out, 63, 6, &mut ofs); // m_a1_0
    set_block_bits(&mut out, 3, 2, &mut ofs); // m_a1_1 -> ofs now 66
    set_block_bits(&mut out, color_index_bits, 31, &mut ofs);
    out
}

/// Convert an ETC1S color block to an opaque BC7 mode-5 block.
pub fn convert_etc1s_to_bc7_m5_color(ep: &Endpoint, sel: &Selector) -> [u8; 16] {
    let low_selector = sel.lo_selector as usize;
    let high_selector = sel.hi_selector as u32;
    let inten_table = ep.inten5 as u32;

    // Solid color: use the precomputed single-color endpoints, selectors all 1.
    if sel.num_unique_selectors == 1 {
        let (r, g, b) = DecoderEtcBlock::get_block_color5(ep.color5, inten_table, low_selector);
        // Table entries are [m_hi, m_lo]; m_r0 = m_lo, m_r1 = m_hi.
        let er = G_BC7_M5_EQUALS_1[r as usize];
        let eg = G_BC7_M5_EQUALS_1[g as usize];
        let eb = G_BC7_M5_EQUALS_1[b as usize];
        return build_bc7_m5(
            er[1] as u32,
            er[0] as u32,
            eg[1] as u32,
            eg[0] as u32,
            eb[1] as u32,
            eb[0] as u32,
            0x2aaa_aaab,
        );
    }

    // Two unique selectors: block truncation coding with anchor handling.
    if sel.num_unique_selectors == 2 {
        let bc = DecoderEtcBlock::get_block_colors5(ep.color5, inten_table);
        let (r0, g0, b0) = (
            bc[low_selector].r() as u32,
            bc[low_selector].g() as u32,
            bc[low_selector].b() as u32,
        );
        let (r1, g1, b1) = (
            bc[high_selector as usize].r() as u32,
            bc[high_selector as usize].g() as u32,
            bc[high_selector as usize].b() as u32,
        );

        let mut e0 = (r0 >> 1, g0 >> 1, b0 >> 1);
        let mut e1 = (r1 >> 1, g1 >> 1, b1 >> 1);
        let mut output_low_selector = 0u32;
        let mut output_bit_offset = 0u32;
        let mut output_bits = 0u32;

        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = sel.get_selector(x, y);
                let mut os = if s == low_selector as u32 {
                    output_low_selector
                } else {
                    3 ^ output_low_selector
                };
                let mut num_bits = 2u32;
                if (x | y) == 0 {
                    // Anchor texel (one stored bit): swap endpoints and invert
                    // the mapping if its selector's high bit would be set.
                    if os & 2 != 0 {
                        e0 = (r1 >> 1, g1 >> 1, b1 >> 1);
                        e1 = (r0 >> 1, g0 >> 1, b0 >> 1);
                        output_low_selector = 3;
                        os = 0;
                    }
                    num_bits = 1;
                }
                output_bits |= os << output_bit_offset;
                output_bit_offset += num_bits;
            }
        }
        return build_bc7_m5(e0.0, e1.0, e0.1, e1.1, e0.2, e1.2, output_bits);
    }

    // General case: pick the best selector mapping across R/G/B.
    let srt = selector_range_index()[low_selector][high_selector as usize] as usize;
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
        let total = G_ETC1_TO_BC7_M5_COLOR[base_r + m].m_err as u32
            + G_ETC1_TO_BC7_M5_COLOR[base_g + m].m_err as u32
            + G_ETC1_TO_BC7_M5_COLOR[base_b + m].m_err as u32;
        if total < best_err {
            best_err = total;
            best_mapping = m;
        }
    }

    let xlat = &SELECTOR_MAPPINGS[best_mapping];
    let (tr, tg, tb) = (
        G_ETC1_TO_BC7_M5_COLOR[base_r + best_mapping],
        G_ETC1_TO_BC7_M5_COLOR[base_g + best_mapping],
        G_ETC1_TO_BC7_M5_COLOR[base_b + best_mapping],
    );

    let mut s_inv = 0u32;
    let (r0, g0, b0, r1, g1, b1);
    // The anchor texel (0,0) stores one fewer selector bit, so its remapped
    // selector must have a 0 high bit; otherwise swap the endpoints and invert
    // every selector via the inverse map `xlat`.
    if xlat[sel.get_selector(0, 0) as usize] & 2 != 0 {
        r0 = tr.m_hi as u32;
        g0 = tg.m_hi as u32;
        b0 = tb.m_hi as u32;
        r1 = tr.m_lo as u32;
        g1 = tg.m_lo as u32;
        b1 = tb.m_lo as u32;
        s_inv = 3;
    } else {
        r0 = tr.m_lo as u32;
        g0 = tg.m_lo as u32;
        b0 = tb.m_lo as u32;
        r1 = tr.m_hi as u32;
        g1 = tg.m_hi as u32;
        b1 = tb.m_hi as u32;
    }

    let mut output_bits = 0u32;
    let mut output_bit_ofs = 0u32;
    for y in 0..4u32 {
        for x in 0..4u32 {
            let s = sel.get_selector(x, y) as usize;
            let os = (xlat[s] as u32) ^ s_inv;
            output_bits |= os << output_bit_ofs;
            output_bit_ofs += if (x | y) == 0 { 1 } else { 2 };
        }
    }
    build_bc7_m5(r0, r1, g0, g1, b0, b1, output_bits)
}

impl Etc1sTranscoder {
    /// Transcode an ETC1S color slice to opaque 16-byte BC7 mode-5 blocks,
    /// written into `out` (exactly `num_blocks_x*num_blocks_y*16` bytes),
    /// optionally running the cross-block chroma filter afterwards. `None` on a
    /// corrupt stream.
    pub fn transcode_slice_bc7(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        chroma_filter: bool,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != indices.len() * 16 {
            return None;
        }
        out.fill(0);
        let mut grid = chroma_filter.then(|| new_endpoint_grid(num_blocks_x, num_blocks_y));
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block = convert_etc1s_to_bc7_m5_color(
                &self.endpoints[ei as usize],
                &self.selectors[si as usize],
            );
            out[b * 16..b * 16 + 16].copy_from_slice(&block);
            if let Some(g) = grid.as_mut() {
                let bx = b as u32 % num_blocks_x;
                let by = b as u32 / num_blocks_x;
                record_endpoint(g, bx, by, ei);
            }
        }
        if let Some(g) = grid.as_ref() {
            chroma_filter_bc7_mode5(g, out, num_blocks_x, num_blocks_y, &self.endpoints);
        }
        Some(())
    }

    /// Combined ETC1S -> BC7 mode 5 RGBA: color block from the RGB slice, then
    /// the alpha plane overwritten from the alpha slice, written into `out`
    /// (exactly `num_blocks_x*num_blocks_y*16` bytes). `video`, if present,
    /// is the previous frame's (color, alpha) index slots for ETC1S video.
    #[allow(clippy::too_many_arguments)]
    pub fn transcode_image_bc7_rgba(
        &self,
        rgb_slice: &[u8],
        alpha_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        chroma_filter: bool,
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
        let mut grid = chroma_filter.then(|| new_endpoint_grid(num_blocks_x, num_blocks_y));

        // Color pass first (mode-5 opaque blocks), recording endpoints.
        for (b, &(rei, rsi)) in rgb.iter().enumerate() {
            let block = convert_etc1s_to_bc7_m5_color(
                &self.endpoints[rei as usize],
                &self.selectors[rsi as usize],
            );
            out[b * 16..b * 16 + 16].copy_from_slice(&block);
            if let Some(g) = grid.as_mut() {
                let bx = b as u32 % num_blocks_x;
                let by = b as u32 / num_blocks_x;
                record_endpoint(g, bx, by, rei);
            }
        }

        // Cross-block chroma filter (rewrites color, resets alpha to opaque).
        // It runs before the alpha plane is composed in, so the alpha pass below
        // must come after this.
        if let Some(g) = grid.as_ref() {
            chroma_filter_bc7_mode5(g, &mut *out, num_blocks_x, num_blocks_y, &self.endpoints);
        }

        // Alpha pass: overwrite each block's alpha plane.
        for (b, &(aei, asi)) in alpha.iter().enumerate() {
            let block: &mut [u8; 16] = (&mut out[b * 16..b * 16 + 16]).try_into().ok()?;
            convert_etc1s_to_bc7_m5_alpha(
                block,
                &self.endpoints[aei as usize],
                &self.selectors[asi as usize],
            );
        }
        Some(())
    }
}
