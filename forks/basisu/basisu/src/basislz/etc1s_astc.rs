//! ETC1S -> ASTC 4x4 block conversion and slice emit. Maps a decoded ETC1S
//! endpoint+selector to an ASTC 4x4 block via the lazily built lookup tables
//! and the CEM packers. Both the opaque path and the color+alpha (RGBA) path
//! are covered.

use super::astc_pack::{
    pack_cem_12_weight_range0, pack_cem_12_weight_range2, pack_cem_4_weight_range2,
    pack_cem_8_weight_range2, AstcBlockParams,
};
use super::astc_tables::{
    best_grayscale_mapping_0_255, best_grayscale_mapping_47, ise_to_unquant, selector_range_index,
    single_color_encoding_0, single_color_encoding_1, SELECTOR_MAPPINGS,
};
use super::etc1s::{Endpoint, Etc1sTranscoder, Selector};
use crate::etc::block::DecoderEtcBlock;
use crate::tables::astc::G_ETC1_TO_ASTC;
use crate::tables::astc_0_255::G_ETC1_TO_ASTC_0_255;
use crate::uastc::astc_pack::set_bits;
use alloc::vec::Vec;

/// Selector-range count: with [`NUM_MAPPINGS`] it forms the two innermost
/// strides of the `G_ETC1_TO_ASTC*` tables, indexed as `(inten*32 + base) *
/// (NUM_RANGES * NUM_MAPPINGS) + range * NUM_MAPPINGS + mapping`.
const NUM_RANGES: usize = 6;
/// Selector-mapping count per range (see [`NUM_RANGES`] for the indexing).
const NUM_MAPPINGS: usize = 10;

/// ASTC void-extent (solid color) block with the given 8-bit RGBA.
fn astc_void_extent(r: u32, g: u32, b: u32, a: u32) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0] = 0xfc;
    out[1] = 0xfd;
    out[2] = 0xff;
    out[3] = 0xff;
    out[4..8].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
    let mut bit_pos = 64i32;
    set_bits(&mut out, &mut bit_pos, r | (r << 8), 16);
    set_bits(&mut out, &mut bit_pos, g | (g << 8), 16);
    set_bits(&mut out, &mut bit_pos, b | (b << 8), 16);
    set_bits(&mut out, &mut bit_pos, a | (a << 8), 16);
    out
}

/// Convert one opaque ETC1S block (constant alpha 255) to an ASTC 4x4 block.
/// Cases, in priority order: void-extent (solid color), BTC (<= 2 unique
/// selectors), grayscale luminance+alpha, and the CEM 8 8-bit-endpoint path.
pub fn convert_etc1s_to_astc_4x4(ep: &Endpoint, sel: &Selector) -> [u8; 16] {
    let base_color = ep.color5;
    let inten_table = ep.inten5 as u32;
    let low_selector = sel.lo_selector as usize;
    let high_selector = sel.hi_selector as u32;

    // Solid color -> ASTC void-extent block with constant alpha 255.
    if sel.num_unique_selectors == 1 {
        let (r, g, b) = DecoderEtcBlock::get_block_color5(base_color, inten_table, low_selector);
        return astc_void_extent(r, g, b, 255);
    }

    // Block truncation coding: <= 2 unique selectors, lossless to ASTC.
    if sel.num_unique_selectors <= 2 {
        let mut blk = AstcBlockParams::default();
        let bc = DecoderEtcBlock::get_block_colors5(base_color, inten_table);
        blk.endpoints[0] = bc[low_selector].r();
        blk.endpoints[2] = bc[low_selector].g();
        blk.endpoints[4] = bc[low_selector].b();
        blk.endpoints[1] = bc[high_selector as usize].r();
        blk.endpoints[3] = bc[high_selector as usize].g();
        blk.endpoints[5] = bc[high_selector as usize].b();

        let s0 = blk.endpoints[0] as i32 + blk.endpoints[2] as i32 + blk.endpoints[4] as i32;
        let s1 = blk.endpoints[1] as i32 + blk.endpoints[3] as i32 + blk.endpoints[5] as i32;
        let invert = s1 < s0;
        if invert {
            blk.endpoints.swap(0, 1);
            blk.endpoints.swap(2, 3);
            blk.endpoints.swap(4, 5);
        }
        blk.endpoints[6] = 255;
        blk.endpoints[7] = 255;
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = sel.get_selector(x, y);
                let mut s = if s == high_selector { 1 } else { 0 };
                if invert {
                    s = 1 - s;
                }
                blk.weights[((x + y * 4) * 2) as usize] = s as u8;
            }
        }
        return pack_cem_12_weight_range0(&blk);
    }

    // Grayscale -> CEM 4 (Luminance+Alpha). Only blocks with more than two
    // unique selectors reach here; the BTC case above returns for the rest.
    if base_color.r() == base_color.g() && base_color.r() == base_color.b() {
        let mut blk = AstcBlockParams::default();
        blk.endpoints[2] = 255; // opaque alpha
        blk.endpoints[3] = 255;

        let g = base_color.g() as usize;
        let srt = selector_range_index()[low_selector][high_selector as usize] as usize;
        let base_idx =
            (inten_table as usize * 32 + g) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
        let best = best_grayscale_mapping_0_255()[g][inten_table as usize][srt] as usize;
        blk.endpoints[0] = G_ETC1_TO_ASTC_0_255[base_idx + best].m_lo;
        blk.endpoints[1] = G_ETC1_TO_ASTC_0_255[base_idx + best].m_hi;
        let xlat = &SELECTOR_MAPPINGS[best];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = sel.get_selector(x, y) as usize;
                blk.weights[((x + y * 4) * 2) as usize] = xlat[s];
            }
        }
        return pack_cem_4_weight_range2(&blk);
    }

    // Opaque, non-grayscale, > 2 selectors -> CEM 8 with 8-bit endpoints.
    let srt = selector_range_index()[low_selector][high_selector as usize] as usize;
    let it = inten_table as usize;
    let base_r =
        (it * 32 + base_color.r() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
    let base_g =
        (it * 32 + base_color.g() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
    let base_b =
        (it * 32 + base_color.b() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;

    let mut best_err = u32::MAX;
    let mut best_mapping = 0usize;
    for m in 0..NUM_MAPPINGS {
        let total = G_ETC1_TO_ASTC_0_255[base_r + m].m_err as u32
            + G_ETC1_TO_ASTC_0_255[base_g + m].m_err as u32
            + G_ETC1_TO_ASTC_0_255[base_b + m].m_err as u32;
        if total < best_err {
            best_err = total;
            best_mapping = m;
        }
    }

    let mut blk = AstcBlockParams::default();
    blk.endpoints[0] = G_ETC1_TO_ASTC_0_255[base_r + best_mapping].m_lo;
    blk.endpoints[1] = G_ETC1_TO_ASTC_0_255[base_r + best_mapping].m_hi;
    blk.endpoints[2] = G_ETC1_TO_ASTC_0_255[base_g + best_mapping].m_lo;
    blk.endpoints[3] = G_ETC1_TO_ASTC_0_255[base_g + best_mapping].m_hi;
    blk.endpoints[4] = G_ETC1_TO_ASTC_0_255[base_b + best_mapping].m_lo;
    blk.endpoints[5] = G_ETC1_TO_ASTC_0_255[base_b + best_mapping].m_hi;

    let s0 = blk.endpoints[0] as i32 + blk.endpoints[2] as i32 + blk.endpoints[4] as i32;
    let s1 = blk.endpoints[1] as i32 + blk.endpoints[3] as i32 + blk.endpoints[5] as i32;
    let invert = s1 < s0;
    if invert {
        blk.endpoints.swap(0, 1);
        blk.endpoints.swap(2, 3);
        blk.endpoints.swap(4, 5);
    }
    let xlat = &SELECTOR_MAPPINGS[best_mapping];
    for y in 0..4u32 {
        for x in 0..4u32 {
            let s = sel.get_selector(x, y) as usize;
            let mut a = xlat[s];
            if invert {
                a = 3 - a;
            }
            blk.weights[(x + y * 4) as usize] = a;
        }
    }
    pack_cem_8_weight_range2(&blk)
}

/// Convert an ETC1S color block plus an ETC1S grayscale alpha block into an
/// ASTC 4x4 RGBA block. Cases, in priority order: void-extent (solid color and
/// solid alpha), BTC (both have <= 2 unique selectors), grayscale
/// luminance+alpha, fully opaque (reuses the opaque RGB path), and the CEM 12
/// dual-plane fallback.
pub fn convert_etc1s_to_astc_4x4_rgba(
    cep: &Endpoint,
    csel: &Selector,
    aep: &Endpoint,
    asel: &Selector,
) -> [u8; 16] {
    let base_color = cep.color5;
    let inten_table = cep.inten5 as u32;
    let low_selector = csel.lo_selector as usize;
    let high_selector = csel.hi_selector as u32;

    let alpha_base = aep.color5;
    let alpha_inten = aep.inten5 as u32;
    let alpha_low = asel.lo_selector as usize;
    let alpha_high = asel.hi_selector as u32;
    let num_unique_alpha = asel.num_unique_selectors;

    let mut constant_alpha_val = 255u32;
    if num_unique_alpha == 1 {
        constant_alpha_val =
            DecoderEtcBlock::get_block_colors5_g(alpha_base, alpha_inten)[alpha_low] as u32;
    }

    // Case 1: solid color + solid alpha -> void-extent.
    if csel.num_unique_selectors == 1 && num_unique_alpha == 1 {
        let (r, g, b) = DecoderEtcBlock::get_block_color5(base_color, inten_table, low_selector);
        return astc_void_extent(r, g, b, constant_alpha_val);
    }

    // Case 2: BTC, both <= 2 unique selectors.
    if csel.num_unique_selectors <= 2 && num_unique_alpha <= 2 {
        let mut blk = AstcBlockParams::default();
        let bc = DecoderEtcBlock::get_block_colors5(base_color, inten_table);
        blk.endpoints[0] = bc[low_selector].r();
        blk.endpoints[2] = bc[low_selector].g();
        blk.endpoints[4] = bc[low_selector].b();
        blk.endpoints[1] = bc[high_selector as usize].r();
        blk.endpoints[3] = bc[high_selector as usize].g();
        blk.endpoints[5] = bc[high_selector as usize].b();
        let s0 = blk.endpoints[0] as i32 + blk.endpoints[2] as i32 + blk.endpoints[4] as i32;
        let s1 = blk.endpoints[1] as i32 + blk.endpoints[3] as i32 + blk.endpoints[5] as i32;
        let invert = s1 < s0;
        if invert {
            blk.endpoints.swap(0, 1);
            blk.endpoints.swap(2, 3);
            blk.endpoints.swap(4, 5);
        }
        let ac = DecoderEtcBlock::get_block_colors5_g(alpha_base, alpha_inten);
        blk.endpoints[6] = ac[alpha_low] as u8;
        blk.endpoints[7] = ac[alpha_high as usize] as u8;
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = asel.get_selector(x, y);
                blk.weights[((x + y * 4) * 2 + 1) as usize] = if s == alpha_high { 1 } else { 0 };
            }
        }
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = csel.get_selector(x, y);
                let mut cs = if s == high_selector { 1 } else { 0 };
                if invert {
                    cs = 1 - cs;
                }
                blk.weights[((x + y * 4) * 2) as usize] = cs as u8;
            }
        }
        return pack_cem_12_weight_range0(&blk);
    }

    // Case 3: grayscale color -> CEM 4 (Luminance+Alpha).
    if base_color.r() == base_color.g() && base_color.r() == base_color.b() {
        let mut blk = AstcBlockParams::default();
        // Alpha plane (odd weights, endpoints 2/3).
        if num_unique_alpha <= 2 {
            let ac = DecoderEtcBlock::get_block_colors5_g(alpha_base, alpha_inten);
            blk.endpoints[2] = ac[alpha_low] as u8;
            blk.endpoints[3] = ac[alpha_high as usize] as u8;
            for y in 0..4u32 {
                for x in 0..4u32 {
                    let s = asel.get_selector(x, y);
                    blk.weights[((x + y * 4) * 2 + 1) as usize] =
                        if s == alpha_high { 3 } else { 0 };
                }
            }
        } else {
            let ag = alpha_base.g() as usize;
            let srt = selector_range_index()[alpha_low][alpha_high as usize] as usize;
            let bi =
                (alpha_inten as usize * 32 + ag) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
            let best = best_grayscale_mapping_0_255()[ag][alpha_inten as usize][srt] as usize;
            blk.endpoints[2] = G_ETC1_TO_ASTC_0_255[bi + best].m_lo;
            blk.endpoints[3] = G_ETC1_TO_ASTC_0_255[bi + best].m_hi;
            let xlat = &SELECTOR_MAPPINGS[best];
            for y in 0..4u32 {
                for x in 0..4u32 {
                    let s = asel.get_selector(x, y) as usize;
                    blk.weights[((x + y * 4) * 2 + 1) as usize] = xlat[s];
                }
            }
        }
        // Color plane (even weights, endpoints 0/1).
        let g = base_color.g() as usize;
        if csel.num_unique_selectors <= 2 {
            let bc = DecoderEtcBlock::get_block_colors5_g(base_color, inten_table);
            blk.endpoints[0] = bc[low_selector] as u8;
            blk.endpoints[1] = bc[high_selector as usize] as u8;
            for i in 0..16u32 {
                let s = csel.get_selector(i & 3, i >> 2);
                blk.weights[(i * 2) as usize] = if s == high_selector { 3 } else { 0 };
            }
        } else {
            let srt = selector_range_index()[low_selector][high_selector as usize] as usize;
            let bi =
                (inten_table as usize * 32 + g) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
            let best = best_grayscale_mapping_0_255()[g][inten_table as usize][srt] as usize;
            blk.endpoints[0] = G_ETC1_TO_ASTC_0_255[bi + best].m_lo;
            blk.endpoints[1] = G_ETC1_TO_ASTC_0_255[bi + best].m_hi;
            let xlat = &SELECTOR_MAPPINGS[best];
            for y in 0..4u32 {
                for x in 0..4u32 {
                    let s = csel.get_selector(x, y) as usize;
                    blk.weights[((x + y * 4) * 2) as usize] = xlat[s];
                }
            }
        }
        return pack_cem_4_weight_range2(&blk);
    }

    // Case 4: fully opaque (constant alpha 255) -> reuse the opaque 8-bit path.
    if num_unique_alpha == 1 && constant_alpha_val == 255 {
        return convert_etc1s_to_astc_4x4(cep, csel);
    }

    // Case 5: CEM 12 dual-plane fallback ([0,47] endpoints).
    let mut blk = AstcBlockParams::default();
    let ise = ise_to_unquant();
    let sc1 = single_color_encoding_1();
    let sc0 = single_color_encoding_0();

    // alpha plane (endpoints 6/7, odd weights).
    if alpha_low == alpha_high as usize {
        let g = DecoderEtcBlock::get_block_colors5_g(alpha_base, alpha_inten)[alpha_low] as usize;
        blk.endpoints[6] = sc1[g][0];
        blk.endpoints[7] = sc1[g][1];
        for i in 0..16usize {
            blk.weights[i * 2 + 1] = 1;
        }
    } else if alpha_inten >= 7 && num_unique_alpha == 2 && alpha_low == 0 && alpha_high == 3 {
        let ac = DecoderEtcBlock::get_block_colors5(alpha_base, alpha_inten);
        blk.endpoints[6] = sc0[ac[0].g() as usize];
        blk.endpoints[7] = sc0[ac[3].g() as usize];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = asel.get_selector(x, y);
                blk.weights[((x + y * 4) * 2 + 1) as usize] = if s == alpha_high { 3 } else { 0 };
            }
        }
    } else {
        let ag = alpha_base.g() as usize;
        let srt = selector_range_index()[alpha_low][alpha_high as usize] as usize;
        let bi =
            (alpha_inten as usize * 32 + ag) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
        let best = best_grayscale_mapping_47()[ag][alpha_inten as usize][srt] as usize;
        blk.endpoints[6] = G_ETC1_TO_ASTC[bi + best].m_lo;
        blk.endpoints[7] = G_ETC1_TO_ASTC[bi + best].m_hi;
        let xlat = &SELECTOR_MAPPINGS[best];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = asel.get_selector(x, y) as usize;
                blk.weights[((x + y * 4) * 2 + 1) as usize] = xlat[s];
            }
        }
    }

    // color plane (endpoints 0..6, even weights).
    if low_selector == high_selector as usize {
        let bc = DecoderEtcBlock::get_block_colors5(base_color, inten_table);
        let (r, g, b) = (
            bc[low_selector].r() as usize,
            bc[low_selector].g() as usize,
            bc[low_selector].b() as usize,
        );
        blk.endpoints[0] = sc1[r][0];
        blk.endpoints[1] = sc1[r][1];
        blk.endpoints[2] = sc1[g][0];
        blk.endpoints[3] = sc1[g][1];
        blk.endpoints[4] = sc1[b][0];
        blk.endpoints[5] = sc1[b][1];
        let s0 = ise[blk.endpoints[0] as usize]
            + ise[blk.endpoints[2] as usize]
            + ise[blk.endpoints[4] as usize];
        let s1 = ise[blk.endpoints[1] as usize]
            + ise[blk.endpoints[3] as usize]
            + ise[blk.endpoints[5] as usize];
        let invert = s1 < s0;
        if invert {
            blk.endpoints.swap(0, 1);
            blk.endpoints.swap(2, 3);
            blk.endpoints.swap(4, 5);
        }
        for i in 0..16usize {
            blk.weights[i * 2] = if invert { 2 } else { 1 };
        }
    } else if inten_table >= 7
        && csel.num_unique_selectors == 2
        && low_selector == 0
        && high_selector == 3
    {
        let bc = DecoderEtcBlock::get_block_colors5(base_color, inten_table);
        blk.endpoints[0] = sc0[bc[0].r() as usize];
        blk.endpoints[1] = sc0[bc[3].r() as usize];
        blk.endpoints[2] = sc0[bc[0].g() as usize];
        blk.endpoints[3] = sc0[bc[3].g() as usize];
        blk.endpoints[4] = sc0[bc[0].b() as usize];
        blk.endpoints[5] = sc0[bc[3].b() as usize];
        let s0 = ise[blk.endpoints[0] as usize]
            + ise[blk.endpoints[2] as usize]
            + ise[blk.endpoints[4] as usize];
        let s1 = ise[blk.endpoints[1] as usize]
            + ise[blk.endpoints[3] as usize]
            + ise[blk.endpoints[5] as usize];
        let invert = s1 < s0;
        if invert {
            blk.endpoints.swap(0, 1);
            blk.endpoints.swap(2, 3);
            blk.endpoints.swap(4, 5);
        }
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = csel.get_selector(x, y);
                let mut a = if s == low_selector as u32 { 0 } else { 3 };
                if invert {
                    a = 3 - a;
                }
                blk.weights[((x + y * 4) * 2) as usize] = a as u8;
            }
        }
    } else {
        let srt = selector_range_index()[low_selector][high_selector as usize] as usize;
        let it = inten_table as usize;
        let base_r =
            (it * 32 + base_color.r() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
        let base_g =
            (it * 32 + base_color.g() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
        let base_b =
            (it * 32 + base_color.b() as usize) * (NUM_RANGES * NUM_MAPPINGS) + srt * NUM_MAPPINGS;
        let mut best_err = u32::MAX;
        let mut best_mapping = 0usize;
        for m in 0..NUM_MAPPINGS {
            let total = G_ETC1_TO_ASTC[base_r + m].m_err as u32
                + G_ETC1_TO_ASTC[base_g + m].m_err as u32
                + G_ETC1_TO_ASTC[base_b + m].m_err as u32;
            if total < best_err {
                best_err = total;
                best_mapping = m;
            }
        }
        blk.endpoints[0] = G_ETC1_TO_ASTC[base_r + best_mapping].m_lo;
        blk.endpoints[1] = G_ETC1_TO_ASTC[base_r + best_mapping].m_hi;
        blk.endpoints[2] = G_ETC1_TO_ASTC[base_g + best_mapping].m_lo;
        blk.endpoints[3] = G_ETC1_TO_ASTC[base_g + best_mapping].m_hi;
        blk.endpoints[4] = G_ETC1_TO_ASTC[base_b + best_mapping].m_lo;
        blk.endpoints[5] = G_ETC1_TO_ASTC[base_b + best_mapping].m_hi;
        let s0 = ise[blk.endpoints[0] as usize]
            + ise[blk.endpoints[2] as usize]
            + ise[blk.endpoints[4] as usize];
        let s1 = ise[blk.endpoints[1] as usize]
            + ise[blk.endpoints[3] as usize]
            + ise[blk.endpoints[5] as usize];
        let invert = s1 < s0;
        if invert {
            blk.endpoints.swap(0, 1);
            blk.endpoints.swap(2, 3);
            blk.endpoints.swap(4, 5);
        }
        let xlat = &SELECTOR_MAPPINGS[best_mapping];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let s = csel.get_selector(x, y) as usize;
                let mut a = xlat[s];
                if invert {
                    a = 3 - a;
                }
                blk.weights[((x + y * 4) * 2) as usize] = a;
            }
        }
    }

    pack_cem_12_weight_range2(&blk)
}

impl Etc1sTranscoder {
    /// Combined ETC1S -> ASTC 4x4 RGBA: color slice + alpha slice, written
    /// into `out` (exactly `num_blocks_x*num_blocks_y*16` bytes). `video`, if
    /// present, is the previous frame's (color, alpha) index slots for ETC1S
    /// video. `None` on a corrupt stream.
    pub fn transcode_image_astc_rgba(
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
            let block = convert_etc1s_to_astc_4x4_rgba(
                &self.endpoints[rei as usize],
                &self.selectors[rsi as usize],
                &self.endpoints[aei as usize],
                &self.selectors[asi as usize],
            );
            out[b * 16..b * 16 + 16].copy_from_slice(&block);
        }
        Some(())
    }

    /// Transcode an opaque ETC1S slice to ASTC 4x4 (16 bytes per block),
    /// written into `out` (exactly `num_blocks_x*num_blocks_y*16` bytes).
    /// `None` on a corrupt stream.
    pub fn transcode_slice_astc(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != indices.len() * 16 {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block = convert_etc1s_to_astc_4x4(
                &self.endpoints[ei as usize],
                &self.selectors[si as usize],
            );
            out[b * 16..b * 16 + 16].copy_from_slice(&block);
        }
        Some(())
    }
}
