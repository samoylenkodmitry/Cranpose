//! `convert_etc1s_to_dxt1` and the ETC1S -> BC1 (DXT1) slice emit. Maps a
//! decoded ETC1S endpoint+selector to an 8-byte BC1 block
//! `{ u16 low_color, u16 high_color, u8 selectors[4] }`.
//!
//! When `use_threecolor_blocks` is false, a solid block is nudged so its low
//! color word is greater than its high color word, which selects BC1's 4-color
//! mode. BC3 relies on this: its 3-color punchthrough mode reuses the alpha
//! slot and is unsupported on some GPUs.

use super::astc_tables::{selector_range_index, SELECTOR_MAPPINGS};
use super::etc1s::{Endpoint, Etc1sTranscoder, Selector};
use crate::etc::block::DecoderEtcBlock;
use crate::once::OnceBox;
use crate::tables::bc1::tables as bc1_tables;
use crate::tables::dxt1_5::G_ETC1_TO_DXT_5;
use crate::tables::dxt1_6::G_ETC1_TO_DXT_6;
use alloc::boxed::Box;
use alloc::vec::Vec;

const NUM_RANGES: usize = 6;
const NUM_MAPPINGS: usize = 10;

/// Pack a BC1 (DXT1) block: a u16 low color, a u16 high color, then the four
/// selector bytes, laid out as 8 little-endian bytes.
fn pack_block(low: u16, high: u16, selectors: [u8; 4]) -> [u8; 8] {
    [
        low as u8,
        (low >> 8) as u8,
        high as u8,
        (high >> 8) as u8,
        selectors[0],
        selectors[1],
        selectors[2],
        selectors[3],
    ]
}

/// Pack a 5/6/5-bit endpoint into a 565 color word.
#[inline]
fn pack_unscaled_color(r: u32, g: u32, b: u32) -> u32 {
    b | (g << 5) | (r << 11)
}

/// The init-built forward and inverse DXT1 selector-mapping tables
/// (`[NUM_MAPPINGS][256]`). Each remaps a packed 4x2-bit ETC1S selector byte
/// into the DXT1 selector ordering (and its inverse).
struct DxtSelectorXlat {
    fwd: [[u8; 256]; NUM_MAPPINGS],
    inv: [[u8; 256]; NUM_MAPPINGS],
}

/// Populate the forward and inverse selector remap tables for all 10 mappings.
/// For each mapping, the 4 per-selector remaps are composed: first the mapping's
/// own remap, then the linear-to-DXT1 selector order, with `DXT1_INVERTED_XLAT`
/// giving the inverse used when the endpoints had to be swapped. The 256-entry
/// inner loop precomputes the remap for every packed 4x2-bit selector byte so
/// the hot path is a single table index per byte.
fn build_selector_xlat() -> DxtSelectorXlat {
    const LINEAR_DXT1_TO_DXT1: [u8; 4] = [0, 2, 3, 1];
    const DXT1_INVERTED_XLAT: [u8; 4] = [1, 0, 3, 2];

    let mut out = DxtSelectorXlat {
        fwd: [[0u8; 256]; NUM_MAPPINGS],
        inv: [[0u8; 256]; NUM_MAPPINGS],
    };
    for sm in 0..NUM_MAPPINGS {
        let mut raw = [0u8; 4];
        let mut raw_inv = [0u8; 4];
        for j in 0..4usize {
            raw[j] = LINEAR_DXT1_TO_DXT1[SELECTOR_MAPPINGS[sm][j] as usize];
            raw_inv[j] = DXT1_INVERTED_XLAT[raw[j] as usize];
        }
        for i in 0..256usize {
            let mut k = 0u32;
            let mut k_inv = 0u32;
            for s in 0..4u32 {
                let sel = (i >> (s * 2)) & 3;
                k |= (raw[sel] as u32) << (s * 2);
                k_inv |= (raw_inv[sel] as u32) << (s * 2);
            }
            out.fwd[sm][i] = k as u8;
            out.inv[sm][i] = k_inv as u8;
        }
    }
    out
}

/// The selector remap tables, built once at first use and cached.
fn selector_xlat() -> &'static DxtSelectorXlat {
    static T: OnceBox<DxtSelectorXlat> = OnceBox::new();
    T.get_or_init(|| Box::new(build_selector_xlat()))
}

/// Convert an ETC1S color block to an 8-byte BC1 (DXT1) block. When
/// `use_threecolor_blocks` is false, solid blocks are kept in BC1's 4-color
/// mode rather than the 3-color mode.
pub fn convert_etc1s_to_dxt1(
    ep: &Endpoint,
    sel: &Selector,
    use_threecolor_blocks: bool,
) -> [u8; 8] {
    let low_selector = sel.lo_selector as usize;
    let high_selector = sel.hi_selector as usize;
    let inten_table = ep.inten5 as u32;
    let bc1 = bc1_tables();

    // Solid block.
    if low_selector == high_selector {
        let (r, g, b) = DecoderEtcBlock::get_block_color5(ep.color5, inten_table, low_selector);
        let (r, g, b) = (r as usize, g as usize, b as usize);
        let mut mask = 0xAAu32;
        let mut max16 = ((bc1.match5_equals_1[r].m_hi as u32) << 11)
            | ((bc1.match6_equals_1[g].m_hi as u32) << 5)
            | bc1.match5_equals_1[b].m_hi as u32;
        let mut min16 = ((bc1.match5_equals_1[r].m_lo as u32) << 11)
            | ((bc1.match6_equals_1[g].m_lo as u32) << 5)
            | bc1.match5_equals_1[b].m_lo as u32;

        if !use_threecolor_blocks && min16 == max16 {
            mask = 0;
            if min16 > 0 {
                min16 -= 1;
            } else {
                max16 = 1;
                min16 = 0;
                mask = 0x55;
            }
        }

        if max16 < min16 {
            core::mem::swap(&mut max16, &mut min16);
            mask ^= 0x55;
        }

        let m = mask as u8;
        return pack_block(max16 as u16, min16 as u16, [m, m, m, m]);
    }

    // Special two-color, high-intensity, full-range case.
    if inten_table >= 7
        && sel.num_unique_selectors == 2
        && sel.lo_selector == 0
        && sel.hi_selector == 3
    {
        let block_colors = DecoderEtcBlock::get_block_colors5(ep.color5, inten_table);
        let (r0, g0, b0) = (
            block_colors[0].r() as usize,
            block_colors[0].g() as usize,
            block_colors[0].b() as usize,
        );
        let (r1, g1, b1) = (
            block_colors[3].r() as usize,
            block_colors[3].g() as usize,
            block_colors[3].b() as usize,
        );

        let mut max16 = ((bc1.match5_equals_0[r0].m_hi as u32) << 11)
            | ((bc1.match6_equals_0[g0].m_hi as u32) << 5)
            | bc1.match5_equals_0[b0].m_hi as u32;
        let mut min16 = ((bc1.match5_equals_0[r1].m_hi as u32) << 11)
            | ((bc1.match6_equals_0[g1].m_hi as u32) << 5)
            | bc1.match5_equals_0[b1].m_hi as u32;

        let mut l = 0u32;
        let mut h = 1u32;

        if min16 == max16 {
            if min16 > 0 {
                min16 -= 1;
                l = 0;
                h = 0;
            } else {
                max16 = 1;
                min16 = 0;
                l = 1;
                h = 1;
            }
        }

        if max16 < min16 {
            core::mem::swap(&mut max16, &mut min16);
            l = 1;
            h = 0;
        }

        let mut selectors = [0u8; 4];
        for y in 0..4u32 {
            let mut byte = 0u32;
            for x in 0..4u32 {
                let s = sel.get_selector(x, y);
                let v = if s == 3 { h } else { l };
                byte |= v << (x * 2);
            }
            selectors[y as usize] = byte as u8;
        }
        return pack_block(max16 as u16, min16 as u16, selectors);
    }

    // General case: pick the best selector mapping across R/G/B.
    let srt = selector_range_index()[low_selector][high_selector] as usize;
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
        let total = G_ETC1_TO_DXT_5[base_r + m].m_err as u32
            + G_ETC1_TO_DXT_6[base_g + m].m_err as u32
            + G_ETC1_TO_DXT_5[base_b + m].m_err as u32;
        if total < best_err {
            best_err = total;
            best_mapping = m;
        }
    }

    let tr = G_ETC1_TO_DXT_5[base_r + best_mapping];
    let tg = G_ETC1_TO_DXT_6[base_g + best_mapping];
    let tb = G_ETC1_TO_DXT_5[base_b + best_mapping];

    let mut l = pack_unscaled_color(tr.m_lo as u32, tg.m_lo as u32, tb.m_lo as u32);
    let mut h = pack_unscaled_color(tr.m_hi as u32, tg.m_hi as u32, tb.m_hi as u32);

    let xlat = selector_xlat();
    let mut use_inv = false;
    if l < h {
        core::mem::swap(&mut l, &mut h);
        use_inv = true;
    }

    if l == h {
        let mut mask = 0u8;
        if !use_threecolor_blocks {
            if h > 0 {
                h -= 1;
            } else {
                h = 0;
                l = 1;
                mask = 0x55;
            }
        }
        return pack_block(l as u16, h as u16, [mask, mask, mask, mask]);
    }

    let xtab = if use_inv {
        &xlat.inv[best_mapping]
    } else {
        &xlat.fwd[best_mapping]
    };
    let s = sel.selectors;
    pack_block(
        l as u16,
        h as u16,
        [
            xtab[s[0] as usize],
            xtab[s[1] as usize],
            xtab[s[2] as usize],
            xtab[s[3] as usize],
        ],
    )
}

impl Etc1sTranscoder {
    /// Transcode a slice to BC1 (opaque): 8-byte BC1 blocks, written into
    /// `out` (exactly `num_blocks_x*num_blocks_y*8` bytes).
    /// `forbid_three_color` disables the three-color BC1 block encoding.
    /// `video`, if present, is the previous frame's per-block indices for this
    /// slice (ETC1S video).
    pub fn transcode_slice_bc1(
        &self,
        slice_data: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        forbid_three_color: bool,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        let indices = self.decode_slice_indices(slice_data, num_blocks_x, num_blocks_y, video)?;
        if out.len() != indices.len() * 8 {
            return None;
        }
        out.fill(0);
        for (b, &(ei, si)) in indices.iter().enumerate() {
            let block = convert_etc1s_to_dxt1(
                &self.endpoints[ei as usize],
                &self.selectors[si as usize],
                !forbid_three_color,
            );
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        Some(())
    }
}
