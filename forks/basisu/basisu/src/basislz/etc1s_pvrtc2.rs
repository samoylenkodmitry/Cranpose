//! `convert_etc1s_to_pvrtc2_rgb` and `convert_etc1s_to_pvrtc2_rgba`. Maps a
//! decoded ETC1S endpoint and selector to an 8-byte PVRTC2 4bpp block.
//!
//! Unlike PVRTC1, PVRTC2 here is **per-block** (no whole-image apply pass) and
//! supports arbitrary dimensions. The RGB path forces the "opaque/hard" PVRTC2
//! mode and looks like a slightly weaker ATC/BC1. The RGBA path uses the
//! transparent (4433/4443) mode and re-encodes the block at the pixel level
//! from the color + alpha ETC1S planes (incremental 4D PCA or, for the unclamped
//! luma case, a 2D LA projection).
//!
//! The PVRTC2 block bit layout:
//!   bytes 0..4  = `m_modulation[4]`
//!   bytes 4..8  = `m_color_data_bits` (uint32 LE; bitfields LSB-first)
//!     opaque:  mod_flag(1) blue_a(4) green_a(5) red_a(5) hard_flag(1)
//!              blue_b(5) green_b(5) red_b(5) opaque_flag(1)
//!     trans:   mod_flag(1) blue_a(3) green_a(4) red_a(4) alpha_a(3) hard_flag(1)
//!              blue_b(4) green_b(4) red_b(4) alpha_b(3) opaque_flag(1)

use super::etc1s::{Endpoint, Etc1sTranscoder, Selector};
use crate::color::Color32;
use crate::etc::block::DecoderEtcBlock;
use crate::etc::tables::INTEN_TABLES;
use crate::tables::atc_55::G_ETC1S_TO_ATC_55;
use crate::tables::pvrtc2_45::G_ETC1S_TO_PVRTC2_45;
use alloc::vec::Vec;

/// Number of (low, high) selector ranges the conversion tables are indexed by.
const NUM_RANGES: usize = 6;
/// Number of candidate selector mappings per range.
const NUM_MAPPINGS: usize = 10;
/// Index of the identity entry in `ATC_SELECTOR_MAPPINGS`, which lets the
/// selectors pass through untranslated.
const ATC_IDENTITY_SELECTOR_MAPPING_INDEX: usize = 6;

/// The (low, high) selector spans the ATC and PVRTC2 conversion tables are
/// indexed by.
static ATC_SELECTOR_RANGES: [[u32; 2]; NUM_RANGES] =
    [[0, 3], [1, 3], [0, 2], [1, 2], [2, 3], [0, 1]];

/// The 10 candidate 2-bit selector remaps scored by the conversion tables;
/// entry 6 is the identity.
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

/// Index of the `(low, high)` pair in `ATC_SELECTOR_RANGES`, 0 if not present.
fn atc_selector_range_index(low: u32, high: u32) -> usize {
    for (i, r) in ATC_SELECTOR_RANGES.iter().enumerate() {
        if low == r[0] && high == r[1] {
            return i;
        }
    }
    0
}

// Single-color match candidates, computed on demand by the helpers below
// instead of being built once at init. The loop order and the
// strictly-lower-error winner are load-bearing: ties keep the earliest
// candidate, which decides the exact endpoints emitted.

/// One `(m_lo, m_hi)` single-color match candidate.
#[derive(Clone, Copy)]
struct AtcMatchEntry {
    m_lo: u8,
    m_hi: u8,
}

/// Single-color endpoint match for the input value `i`: brute-force the
/// `(lo, hi)` endpoint pairs and keep the pair whose reconstructed color is
/// closest. `size0`/`size1` give each endpoint's quantized range and pick its
/// expansion to 8 bits (16 = 4-bit widened via 5-bit, 32 = 5-bit, 64 = 6-bit).
/// `sel == 1` scores the (5*lo + 3*hi)/8 interpolated point, `sel == 3` scores
/// `hi` alone. The earliest pair reaching the lowest error wins (seeded at 256).
fn prepare_atc_single_color(i: u32, size0: u32, size1: u32, sel: u32) -> AtcMatchEntry {
    let i = i as i32;
    let mut best_lo = 0u32;
    let mut best_hi = 0u32;
    let mut lowest_e = 256i32;
    for lo in 0..size0 {
        let lo_e = match size0 {
            16 => {
                let e = (lo << 1) | (lo >> 3);
                ((e << 3) | (e >> 2)) as i32
            }
            32 => ((lo << 3) | (lo >> 2)) as i32,
            _ => ((lo << 2) | (lo >> 4)) as i32,
        };
        for hi in 0..size1 {
            let hi_e = match size1 {
                16 => {
                    let e = (hi << 1) | (hi >> 3);
                    ((e << 3) | (e >> 2)) as i32
                }
                32 => ((hi << 3) | (hi >> 2)) as i32,
                _ => ((hi << 2) | (hi >> 4)) as i32,
            };
            let e = if sel == 1 {
                ((lo_e * 5 + hi_e * 3) / 8 - i).abs()
            } else {
                (hi_e - i).abs()
            };
            if e < lowest_e {
                best_lo = lo;
                best_hi = hi;
                lowest_e = e;
            }
        }
    }
    AtcMatchEntry {
        m_lo: best_lo as u8,
        m_hi: best_hi as u8,
    }
}

// Alpha and transparent-mode endpoint matchers. Each takes a value in 0..=255
// and exhaustively searches the (low, high) endpoint pairs; the first strictly
// lower error wins, so ties keep the earliest candidate.

/// Best 3-bit (low, high) alpha pair whose (le*5 + he*3)/8 interpolation
/// matches `v`, where le = (l<<1) and
/// he = ((h<<1)|1), each replicated into both nibbles.
fn pvrtc2_alpha_match33(v: u32) -> (u32, u32) {
    let v = v as i32;
    let (mut best_l, mut best_h, mut lowest) = (0u32, 0u32, i32::MAX);
    for l in 0..8u32 {
        let le = l << 1;
        let le = (le << 4) | le;
        for h in 0..8u32 {
            let he = (h << 1) | 1;
            let he = (he << 4) | he;
            let m = (le * 5 + he * 3) / 8;
            let err = (v - m as i32).abs();
            if err < lowest {
                lowest = err;
                best_l = l;
                best_h = h;
            }
        }
    }
    (best_l, best_h)
}

/// Best 3-bit `l` whose expansion le = (l<<1), replicated into both nibbles,
/// matches `v`.
fn pvrtc2_alpha_match33_0(v: u32) -> u32 {
    let v = v as i32;
    let (mut best_l, mut lowest) = (0u32, i32::MAX);
    for l in 0..8u32 {
        let le = l << 1;
        let le = (le << 4) | le;
        let err = (v - le as i32).abs();
        if err < lowest {
            lowest = err;
            best_l = l;
        }
    }
    best_l
}

/// Best 3-bit `h` whose expansion he = ((h<<1)|1),
/// replicated into both nibbles, matches `v`.
fn pvrtc2_alpha_match33_3(v: u32) -> u32 {
    let v = v as i32;
    let (mut best_l, mut lowest) = (0u32, i32::MAX);
    for h in 0..8u32 {
        let he = (h << 1) | 1;
        let he = (he << 4) | he;
        let err = (v - he as i32).abs();
        if err < lowest {
            lowest = err;
            best_l = h;
        }
    }
    best_l
}

/// Translucent match: 3-bit lo (<<2|>>1 then <<3|>>2), 4-bit hi (<<1|>>3
/// then <<3|>>2), m = (le*5 + he*3)/8.
fn pvrtc2_trans_match34(v: u32) -> (u32, u32) {
    let v = v as i32;
    let (mut best_l, mut best_h, mut lowest) = (0u32, 0u32, i32::MAX);
    for l in 0..8u32 {
        let le = (l << 2) | (l >> 1);
        let le = (le << 3) | (le >> 2);
        for h in 0..16u32 {
            let he = (h << 1) | (h >> 3);
            let he = (he << 3) | (he >> 2);
            let m = (le * 5 + he * 3) / 8;
            let err = (v - m as i32).abs();
            if err < lowest {
                lowest = err;
                best_l = l;
                best_h = h;
            }
        }
    }
    (best_l, best_h)
}

/// Translucent match: 4-bit lo & hi (<<1|>>3 then <<3|>>2), m = (le*5+he*3)/8.
fn pvrtc2_trans_match44(v: u32) -> (u32, u32) {
    let v = v as i32;
    let (mut best_l, mut best_h, mut lowest) = (0u32, 0u32, i32::MAX);
    for l in 0..16u32 {
        let le = (l << 1) | (l >> 3);
        let le = (le << 3) | (le >> 2);
        for h in 0..16u32 {
            let he = (h << 1) | (h >> 3);
            let he = (he << 3) | (he >> 2);
            let m = (le * 5 + he * 3) / 8;
            let err = (v - m as i32).abs();
            if err < lowest {
                lowest = err;
                best_l = l;
                best_h = h;
            }
        }
    }
    (best_l, best_h)
}

// Block bit packing.

/// Accumulator for the `m_color_data_bits` u32 (LSB-first bitfield emit).
struct Pvrtc2Block {
    /// The four modulation bytes, one row of 2-bit texel values each.
    modulation: [u8; 4],
    /// The packed color word being assembled.
    color_bits: u32,
    /// Next free bit position in `color_bits`.
    pos: u32,
}

impl Pvrtc2Block {
    /// Empty block with all modulation and color bits cleared.
    fn new() -> Self {
        Self {
            modulation: [0; 4],
            color_bits: 0,
            pos: 0,
        }
    }
    /// Append `n` low bits of `val` at the current bit position (LSB-first).
    fn push(&mut self, val: u32, n: u32) {
        let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        self.color_bits |= (val & mask) << self.pos;
        self.pos += n;
    }
    /// Serialize to the 8-byte block: modulation in bytes 0..4, color bits as
    /// little-endian u32 in bytes 4..8.
    fn into_bytes(self) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[0..4].copy_from_slice(&self.modulation);
        out[4..8].copy_from_slice(&self.color_bits.to_le_bytes());
        out
    }
}

/// Build the opaque-mode color bits: mod_flag=0, then blue_a:4 green_a:5
/// red_a:5, hard_flag=1, blue_b:5 green_b:5 red_b:5, opaque_flag=1.
#[allow(clippy::too_many_arguments)]
fn emit_opaque(blk: &mut Pvrtc2Block, ra: u32, ga: u32, ba: u32, rb: u32, gb: u32, bb: u32) {
    blk.push(0, 1); // mod_flag
    blk.push(ba, 4); // blue_a (554 low)
    blk.push(ga, 5); // green_a
    blk.push(ra, 5); // red_a
    blk.push(1, 1); // hard_flag
    blk.push(bb, 5); // blue_b (555 high)
    blk.push(gb, 5); // green_b
    blk.push(rb, 5); // red_b
    blk.push(1, 1); // opaque_flag
}

/// Build the transparent-mode color bits: mod_flag=0, then blue_a:3 green_a:4
/// red_a:4 alpha_a:3, hard_flag=1, blue_b:4 green_b:4 red_b:4 alpha_b:3,
/// opaque_flag=0.
#[allow(clippy::too_many_arguments)]
fn emit_trans(
    blk: &mut Pvrtc2Block,
    ra: u32,
    ga: u32,
    ba: u32,
    aa: u32,
    rb: u32,
    gb: u32,
    bb: u32,
    ab: u32,
) {
    blk.push(0, 1); // mod_flag
    blk.push(ba, 3); // blue_a (4433 low)
    blk.push(ga, 4); // green_a
    blk.push(ra, 4); // red_a
    blk.push(aa, 3); // alpha_a
    blk.push(1, 1); // hard_flag
    blk.push(bb, 4); // blue_b (4443 high)
    blk.push(gb, 4); // green_b
    blk.push(rb, 4); // red_b
    blk.push(ab, 3); // alpha_b
    blk.push(0, 1); // opaque_flag
}

/// Convert an ETC1S color block to an 8-byte opaque-mode PVRTC2 block.
pub fn convert_etc1s_to_pvrtc2_rgb(ep: &Endpoint, sel: &Selector) -> [u8; 8] {
    let low_selector = sel.lo_selector as u32;
    let high_selector = sel.hi_selector as u32;
    let base_color = ep.color5;
    let inten_table = ep.inten5 as u32;

    let mut blk = Pvrtc2Block::new();

    if low_selector == high_selector {
        let (r, g, b) =
            DecoderEtcBlock::get_block_color5(base_color, inten_table, low_selector as usize);
        // Blue's low endpoint is 4-bit in PVRTC2's opaque mode (size0 = 16);
        // red and green low endpoints are 5-bit (size0 = 32). All use sel==1.
        let lr = prepare_atc_single_color(r, 32, 32, 1);
        let lg = prepare_atc_single_color(g, 32, 32, 1);
        let lb = prepare_atc_single_color(b, 16, 32, 1);
        emit_opaque(
            &mut blk,
            lr.m_lo as u32,
            lg.m_lo as u32,
            lb.m_lo as u32,
            lr.m_hi as u32,
            lg.m_hi as u32,
            lb.m_hi as u32,
        );
        blk.modulation = [0x55, 0x55, 0x55, 0x55];
        return blk.into_bytes();
    }

    if inten_table >= 7 && sel.num_unique_selectors == 2 && low_selector == 0 && high_selector == 3
    {
        let bc = DecoderEtcBlock::get_block_colors5(base_color, inten_table);
        let (r0, g0, b0) = (bc[0].r() as u32, bc[0].g() as u32, bc[0].b() as u32);
        let (r1, g1, b1) = (bc[3].r() as u32, bc[3].g() as u32, bc[3].b() as u32);
        // sel==3 single-color match (low endpoint fixed at 0, only the high
        // value used). The low color's blue is 4-bit (size1 = 16) to match
        // PVRTC2's 554 low packing; every other channel, and the high color's
        // blue, is 5-bit (size1 = 32).
        let lo_r = prepare_atc_single_color(r0, 1, 32, 3).m_hi as u32;
        let lo_g = prepare_atc_single_color(g0, 1, 32, 3).m_hi as u32;
        let lo_b = prepare_atc_single_color(b0, 1, 16, 3).m_hi as u32;
        let hi_r = prepare_atc_single_color(r1, 1, 32, 3).m_hi as u32;
        let hi_g = prepare_atc_single_color(g1, 1, 32, 3).m_hi as u32;
        let hi_b = prepare_atc_single_color(b1, 1, 32, 3).m_hi as u32;
        emit_opaque(&mut blk, lo_r, lo_g, lo_b, hi_r, hi_g, hi_b);
        blk.modulation = sel.selectors;
        return blk.into_bytes();
    }

    let srt = atc_selector_range_index(low_selector, high_selector);
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
        let total = G_ETC1S_TO_ATC_55[base_r + m].m_err as u32
            + G_ETC1S_TO_ATC_55[base_g + m].m_err as u32
            + G_ETC1S_TO_PVRTC2_45[base_b + m].m_err as u32;
        if total < best_err {
            best_err = total;
            best_mapping = m;
        }
    }

    let tr = G_ETC1S_TO_ATC_55[base_r + best_mapping];
    let tg = G_ETC1S_TO_ATC_55[base_g + best_mapping];
    let tb = G_ETC1S_TO_PVRTC2_45[base_b + best_mapping];
    emit_opaque(
        &mut blk,
        tr.m_lo as u32,
        tg.m_lo as u32,
        tb.m_lo as u32,
        tr.m_hi as u32,
        tg.m_hi as u32,
        tb.m_hi as u32,
    );

    if best_mapping == ATC_IDENTITY_SELECTOR_MAPPING_INDEX {
        blk.modulation = sel.selectors;
    } else {
        let xlat = &ATC_SELECTOR_MAPPINGS[best_mapping];
        for (i, &sel_bits) in sel.selectors.iter().enumerate() {
            let sel_bits = sel_bits as u32;
            let mut sels = 0u32;
            for x in 0..4u32 {
                let x_shift = x * 2;
                sels |= (xlat[((sel_bits >> x_shift) & 3) as usize] as u32) << x_shift;
            }
            blk.modulation[i] = sels as u8;
        }
    }

    blk.into_bytes()
}

/// Expand 5:5:5:4 channels to 8 bits by bit replication.
fn convert_rgba_5554_to_8888(r: u32, g: u32, b: u32, a: u32) -> (u32, u32, u32, u32) {
    (
        (r << 3) | (r >> 2),
        (g << 3) | (g >> 2),
        (b << 3) | (b >> 2),
        (a << 4) | a,
    )
}

/// Square, used to accumulate squared error.
#[inline]
fn sq(x: i32) -> i32 {
    x * x
}

/// Clamp to the [0, 1] range.
#[inline]
fn saturate(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Clamp to the [0, 255] range and widen to u32.
#[inline]
fn clamp_to_u8(x: i32) -> u32 {
    x.clamp(0, 255) as u32
}

/// Convert a color ETC1S block plus an alpha ETC1S block to an 8-byte
/// transparent-mode PVRTC2 block. `aep`/`asel` are the endpoint and selector
/// the alpha slice decoded for this block.
pub fn convert_etc1s_to_pvrtc2_rgba(
    ep: &Endpoint,
    sel: &Selector,
    aep: &Endpoint,
    asel: &Selector,
) -> [u8; 8] {
    let num_unique_alpha_selectors = asel.num_unique_selectors as u32;
    let alpha_base_color = aep.color5;
    let alpha_inten_table = aep.inten5 as u32;

    let alpha_block_colors =
        DecoderEtcBlock::get_block_colors5_g(alpha_base_color, alpha_inten_table);

    let mut constant_alpha_val: i32;
    if num_unique_alpha_selectors == 1 {
        constant_alpha_val = alpha_block_colors[asel.lo_selector as usize];
    } else {
        constant_alpha_val = alpha_block_colors[asel.lo_selector as usize];
        for i in (asel.lo_selector as u32 + 1)..=(asel.hi_selector as u32) {
            if constant_alpha_val != alpha_block_colors[i as usize] {
                constant_alpha_val = -1;
                break;
            }
        }
    }

    if constant_alpha_val >= 250 {
        // Opaque enough -> emit as an opaque RGB block.
        return convert_etc1s_to_pvrtc2_rgb(ep, sel);
    }

    let base_color = ep.color5;
    let inten_table = ep.inten5 as u32;
    let low_selector = sel.lo_selector as u32;
    let high_selector = sel.hi_selector as u32;
    let num_unique_color_selectors = sel.num_unique_selectors as u32;

    // Re-encode block at the pixel level from the two ETC1S planes.
    let br = ((base_color.r() as i32) << 3) | ((base_color.r() as i32) >> 2);
    let bg = ((base_color.g() as i32) << 3) | ((base_color.g() as i32) >> 2);
    let bb = ((base_color.b() as i32) << 3) | ((base_color.b() as i32) >> 2);

    let inten = &INTEN_TABLES[inten_table as usize];
    let mut block_cols = [Color32::default(); 4];
    for i in 0..4usize {
        let ci = inten[i];
        block_cols[i] = Color32::new_clamped(br + ci, bg + ci, bb + ci, alpha_block_colors[i]);
    }

    let mut solid_color_block = true;
    if num_unique_color_selectors > 1 {
        for i in (low_selector + 1)..=high_selector {
            if block_cols[low_selector as usize].r() != block_cols[i as usize].r()
                || block_cols[low_selector as usize].g() != block_cols[i as usize].g()
                || block_cols[low_selector as usize].b() != block_cols[i as usize].b()
            {
                solid_color_block = false;
                break;
            }
        }
    }

    let mut blk = Pvrtc2Block::new();

    if solid_color_block && constant_alpha_val >= 0 {
        // Constant color/alpha block: evaluate mod0, mod1, mod3 encodings.
        let (r, g, b) =
            DecoderEtcBlock::get_block_color5(base_color, inten_table, low_selector as usize);
        let r = r as i32;
        let g = g as i32;
        let b = b as i32;
        let ca = constant_alpha_val;

        // Mod 0
        let lr0 = (r * 15 + 128) / 255;
        let lg0 = (g * 15 + 128) / 255;
        let lb0 = (b * 7 + 128) / 255;
        let la0 = pvrtc2_alpha_match33_0(ca as u32) as i32;

        let mut cr0 = (lr0 << 1) | (lr0 >> 3);
        let mut cg0 = (lg0 << 1) | (lg0 >> 3);
        let mut cb0 = (lb0 << 2) | (lb0 >> 1);
        let mut ca0 = la0 << 1;
        cr0 = (cr0 << 3) | (cr0 >> 2);
        cg0 = (cg0 << 3) | (cg0 >> 2);
        cb0 = (cb0 << 3) | (cb0 >> 2);
        ca0 = (ca0 << 4) | ca0;

        let err0 = sq(cr0 - r) + sq(cg0 - g) + sq(cb0 - b) + sq(ca0 - ca) * 2;

        if err0 == 0 || ca < 3 {
            emit_trans(
                &mut blk, lr0 as u32, lg0 as u32, lb0 as u32, la0 as u32, 0, 0, 0, 0,
            );
            blk.modulation = [0, 0, 0, 0];
            return blk.into_bytes();
        }

        // Mod 3
        let lr3 = (r * 15 + 128) / 255;
        let lg3 = (g * 15 + 128) / 255;
        let lb3 = (b * 15 + 128) / 255;
        let la3 = pvrtc2_alpha_match33_3(ca as u32) as i32;

        let mut cr3 = (lr3 << 1) | (lr3 >> 3);
        let mut cg3 = (lg3 << 1) | (lg3 >> 3);
        let mut cb3 = (lb3 << 1) | (lb3 >> 3);
        let mut ca3 = (la3 << 1) | 1;
        cr3 = (cr3 << 3) | (cr3 >> 2);
        cg3 = (cg3 << 3) | (cg3 >> 2);
        cb3 = (cb3 << 3) | (cb3 >> 2);
        ca3 = (ca3 << 4) | ca3;

        let err3 = sq(cr3 - r) + sq(cg3 - g) + sq(cb3 - b) + sq(ca3 - ca) * 2;

        // Mod 1
        let (lr1, hr1) = pvrtc2_trans_match44(r as u32);
        let (lg1, hg1) = pvrtc2_trans_match44(g as u32);
        let (lb1, hb1) = pvrtc2_trans_match34(b as u32);
        let (la1, ha1) = pvrtc2_alpha_match33(ca as u32);
        let (lr1, hr1) = (lr1 as i32, hr1 as i32);
        let (lg1, hg1) = (lg1 as i32, hg1 as i32);
        let (lb1, hb1) = (lb1 as i32, hb1 as i32);
        let (la1, ha1) = (la1 as i32, ha1 as i32);

        let mut clr1 = (lr1 << 1) | (lr1 >> 3);
        let mut clg1 = (lg1 << 1) | (lg1 >> 3);
        let mut clb1 = (lb1 << 2) | (lb1 >> 1);
        let mut cla1 = la1 << 1;
        clr1 = (clr1 << 3) | (clr1 >> 2);
        clg1 = (clg1 << 3) | (clg1 >> 2);
        clb1 = (clb1 << 3) | (clb1 >> 2);
        cla1 = (cla1 << 4) | cla1;

        let mut chr1 = (hr1 << 1) | (hr1 >> 3);
        let mut chg1 = (hg1 << 1) | (hg1 >> 3);
        let mut chb1 = (hb1 << 1) | (hb1 >> 3);
        let mut cha1 = (ha1 << 1) | 1;
        chr1 = (chr1 << 3) | (chr1 >> 2);
        chg1 = (chg1 << 3) | (chg1 >> 2);
        chb1 = (chb1 << 3) | (chb1 >> 2);
        cha1 = (cha1 << 4) | cha1;

        let r1 = (clr1 * 5 + chr1 * 3) / 8;
        let g1 = (clg1 * 5 + chg1 * 3) / 8;
        let b1 = (clb1 * 5 + chb1 * 3) / 8;
        let a1 = (cla1 * 5 + cha1 * 3) / 8;

        let err1 = sq(r1 - r) + sq(g1 - g) + sq(b1 - b) + sq(a1 - ca) * 2;

        if err1 < err0 && err1 < err3 {
            emit_trans(
                &mut blk, lr1 as u32, lg1 as u32, lb1 as u32, la1 as u32, hr1 as u32, hg1 as u32,
                hb1 as u32, ha1 as u32,
            );
            blk.modulation = [0x55, 0x55, 0x55, 0x55];
        } else if err0 < err3 {
            emit_trans(
                &mut blk, lr0 as u32, lg0 as u32, lb0 as u32, la0 as u32, 0, 0, 0, 0,
            );
            blk.modulation = [0, 0, 0, 0];
        } else {
            emit_trans(
                &mut blk, 0, 0, 0, 0, lr3 as u32, lg3 as u32, lb3 as u32, la3 as u32,
            );
            blk.modulation = [0xFF, 0xFF, 0xFF, 0xFF];
        }
        return blk.into_bytes();
    }

    // Complex block: non-solid color and/or alpha pixels.
    let mut min_color = [0.0f32; 4];
    let mut max_color = [0.0f32; 4];

    if solid_color_block {
        // Solid color, varying alpha: the endpoints differ only in alpha.
        let low_a = block_cols[asel.lo_selector as usize].a() as f32;
        let high_a = block_cols[asel.hi_selector as usize].a() as f32;
        let s = 1.0f32 / 255.0;
        let lc = block_cols[low_selector as usize];
        min_color = [
            lc.r() as f32 * s,
            lc.g() as f32 * s,
            lc.b() as f32 * s,
            low_a * s,
        ];
        max_color = [
            lc.r() as f32 * s,
            lc.g() as f32 * s,
            lc.b() as f32 * s,
            high_a * s,
        ];
    } else if constant_alpha_val >= 0 {
        // Varying color, constant alpha: the endpoints differ only in color.
        let s = 1.0f32 / 255.0;
        let lc = block_cols[low_selector as usize];
        let hc = block_cols[high_selector as usize];
        let ca = constant_alpha_val as f32;
        min_color = [
            lc.r() as f32 * s,
            lc.g() as f32 * s,
            lc.b() as f32 * s,
            ca * s,
        ];
        max_color = [
            hc.r() as f32 * s,
            hc.g() as f32 * s,
            hc.b() as f32 * s,
            ca * s,
        ];
    } else if block_cols[low_selector as usize].r() == 0
        || block_cols[high_selector as usize].r() == 255
        || block_cols[low_selector as usize].g() == 0
        || block_cols[high_selector as usize].g() == 255
        || block_cols[low_selector as usize].b() == 0
        || block_cols[high_selector as usize].b() == 255
        || block_cols[asel.lo_selector as usize].a() == 0
        || block_cols[asel.hi_selector as usize].a() == 255
    {
        // Some channel hit 0 or 255, so luma is no longer a reliable color
        // axis. Full 4D incremental PCA: power iteration over the covariance
        // rows, the running axis estimate seeded with the first pixel's offset.
        let mut pixels = [[0i32; 4]; 16];
        let (mut sum_r, mut sum_g, mut sum_b, mut sum_a) = (0u32, 0u32, 0u32, 0u32);
        for i in 0..16usize {
            let rgb = block_cols[sel.get_selector((i & 3) as u32, (i >> 2) as u32) as usize];
            let a = block_cols[asel.get_selector((i & 3) as u32, (i >> 2) as u32) as usize].a();
            pixels[i] = [rgb.r() as i32, rgb.g() as i32, rgb.b() as i32, a as i32];
            sum_r += rgb.r() as u32;
            sum_g += rgb.g() as u32;
            sum_b += rgb.b() as u32;
            sum_a += a as u32;
        }

        let mean_color_scaled = [
            sum_r as f32 / 16.0,
            sum_g as f32 / 16.0,
            sum_b as f32 / 16.0,
            sum_a as f32 / 16.0,
        ];
        let mut mean_color = [
            sum_r as f32 / (16.0 * 255.0),
            sum_g as f32 / (16.0 * 255.0),
            sum_b as f32 / (16.0 * 255.0),
            sum_a as f32 / (16.0 * 255.0),
        ];
        for c in mean_color.iter_mut() {
            *c = saturate(*c);
        }

        let mut axis = [0.0f32; 4];
        for (i, px) in pixels.iter().enumerate() {
            let mut color = [px[0] as f32, px[1] as f32, px[2] as f32, px[3] as f32];
            for k in 0..4 {
                color[k] -= mean_color_scaled[k];
            }
            let a = [
                color[0] * color[0],
                color[1] * color[0],
                color[2] * color[0],
                color[3] * color[0],
            ];
            let b = [
                color[0] * color[1],
                color[1] * color[1],
                color[2] * color[1],
                color[3] * color[1],
            ];
            let c = [
                color[0] * color[2],
                color[1] * color[2],
                color[2] * color[2],
                color[3] * color[2],
            ];
            let d = [
                color[0] * color[3],
                color[1] * color[3],
                color[2] * color[3],
                color[3] * color[3],
            ];
            let mut n = if i != 0 { axis } else { color };
            normalize(&mut n);
            axis[0] += dot4(&a, &n);
            axis[1] += dot4(&b, &n);
            axis[2] += dot4(&c, &n);
            axis[3] += dot4(&d, &n);
        }

        normalize(&mut axis);
        if dot4(&axis, &axis) < 0.5 {
            // Degenerate axis (all pixels at the mean): fall back to a diagonal.
            axis = [0.5, 0.5, 0.5, 0.5];
        }

        let mut l = 1e9f32;
        let mut h = -1e9f32;
        for px in pixels.iter() {
            let color = [px[0] as f32, px[1] as f32, px[2] as f32, px[3] as f32];
            let q = [
                color[0] - mean_color_scaled[0],
                color[1] - mean_color_scaled[1],
                color[2] - mean_color_scaled[2],
                color[3] - mean_color_scaled[3],
            ];
            let d = dot4(&q, &axis);
            l = l.min(d);
            h = h.max(d);
        }

        l *= 1.0 / 255.0;
        h *= 1.0 / 255.0;

        let mut c0 = [0.0f32; 4];
        let mut c1 = [0.0f32; 4];
        for k in 0..4 {
            c0[k] = mean_color[k] + axis[k] * l;
            c1[k] = mean_color[k] + axis[k] * h;
        }
        for k in 0..4 {
            min_color[k] = saturate(c0[k]);
            max_color[k] = saturate(c1[k]);
        }
        if min_color[3] > max_color[3] {
            // The PCA axis sign is arbitrary, so the projection can produce the
            // endpoints with alpha descending. Swap all four channels so that
            // endpoint A carries the lower alpha, which the transparent-mode
            // encode below assumes.
            let snapshot = min_color;
            min_color[0] = max_color[0];
            min_color[1] = max_color[1];
            min_color[2] = max_color[2];
            min_color[3] = max_color[3];
            max_color[0] = snapshot[0];
            max_color[1] = snapshot[1];
            max_color[2] = snapshot[2];
            max_color[3] = snapshot[3];
        }
    } else {
        // No channel clamped, so the RGB axis of an ETC1S block is luma and a
        // 2D (luma, alpha) projection suffices: project each LA pair onto the
        // (1,1) and (1,-1) axes and keep whichever spreads wider to decide
        // whether alpha runs opposite to luma.
        let mut block_cols_l = [0i32; 4];
        let mut block_cols_a = [0i32; 4];
        for i in 0..4usize {
            block_cols_l[i] =
                block_cols[i].r() as i32 + block_cols[i].g() as i32 + block_cols[i].b() as i32;
            block_cols_a[i] = block_cols[i].a() as i32 * 3;
        }

        let (mut p0_min, mut p0_max) = (i32::MAX, i32::MIN);
        let (mut p1_min, mut p1_max) = (i32::MAX, i32::MIN);
        for y in 0..4usize {
            let cs = sel.selectors[y] as u32;
            let as_ = asel.selectors[y] as u32;
            for shift in [0u32, 2, 4, 6] {
                let l = block_cols_l[((cs >> shift) & 3) as usize];
                let a = block_cols_a[((as_ >> shift) & 3) as usize];
                let p0 = l + a;
                p0_min = p0_min.min(p0);
                p0_max = p0_max.max(p0);
                let p1 = l - a;
                p1_min = p1_min.min(p1);
                p1_max = p1_max.max(p1);
            }
        }

        let dist0 = p0_max - p0_min;
        let dist1 = p1_max - p1_min;

        let s = 1.0f32 / 255.0;
        let lc = block_cols[low_selector as usize];
        let hc = block_cols[high_selector as usize];
        min_color = [
            lc.r() as f32 * s,
            lc.g() as f32 * s,
            lc.b() as f32 * s,
            block_cols[asel.lo_selector as usize].a() as f32 * s,
        ];
        max_color = [
            hc.r() as f32 * s,
            hc.g() as f32 * s,
            hc.b() as f32 * s,
            block_cols[asel.hi_selector as usize].a() as f32 * s,
        ];

        if dist1 > dist0 {
            // The (l - a) axis fits better: alpha runs opposite to luma, so
            // flip the RGB ends while the alpha ends stay put.
            core::mem::swap(&mut min_color[0], &mut max_color[0]);
            core::mem::swap(&mut min_color[1], &mut max_color[1]);
            core::mem::swap(&mut min_color[2], &mut max_color[2]);
        }
    }

    // 4433 / 4443 trial endpoints.
    let tmin_r = clamp_to_u8((min_color[0] * 15.0 + 0.5) as i32);
    let tmin_g = clamp_to_u8((min_color[1] * 15.0 + 0.5) as i32);
    let tmin_b = clamp_to_u8((min_color[2] * 7.0 + 0.5) as i32);
    let tmin_a = clamp_to_u8((min_color[3] * 7.0 + 0.5) as i32);
    let tmax_r = clamp_to_u8((max_color[0] * 15.0 + 0.5) as i32);
    let tmax_g = clamp_to_u8((max_color[1] * 15.0 + 0.5) as i32);
    let tmax_b = clamp_to_u8((max_color[2] * 15.0 + 0.5) as i32);
    let tmax_a = clamp_to_u8((max_color[3] * 7.0 + 0.5) as i32);

    emit_trans(
        &mut blk, tmin_r, tmin_g, tmin_b, tmin_a, tmax_r, tmax_g, tmax_b, tmax_a,
    );

    // Reconstruct the two endpoint colors at 8-bit to compute modulation.
    let color_a = (
        (tmin_r << 1) | (tmin_r >> 3),
        (tmin_g << 1) | (tmin_g >> 3),
        (tmin_b << 2) | (tmin_b >> 1),
        tmin_a << 1,
    );
    let color_b = (
        (tmax_r << 1) | (tmax_r >> 3),
        (tmax_g << 1) | (tmax_g >> 3),
        (tmax_b << 1) | (tmax_b >> 3),
        (tmax_a << 1) | 1,
    );
    let (lr, lg, lb, la) = convert_rgba_5554_to_8888(color_a.0, color_a.1, color_a.2, color_a.3);
    let (hr, hg, hb, ha) = convert_rgba_5554_to_8888(color_b.0, color_b.1, color_b.2, color_b.3);
    let (lr, lg, lb, la) = (lr as i32, lg as i32, lb as i32, la as i32);

    let axis_r = hr as i32 - lr;
    let axis_g = hg as i32 - lg;
    let axis_b = hb as i32 - lb;
    let axis_a = ha as i32 - la;
    let len_a = sq(axis_r) + sq(axis_g) + sq(axis_b) + sq(axis_a);

    let thresh01 = (len_a * 3) / 16;
    let thresh12 = len_a >> 1;
    let thresh23 = (len_a * 13) / 16;

    if (axis_r | axis_g | axis_b) == 0 {
        let mut ca_sel = [0i32; 4];
        for i in 0..4usize {
            let ca = (block_cols[i].a() as i32 - la) * axis_a;
            ca_sel[i] = (ca >= thresh23) as i32 + (ca >= thresh12) as i32 + (ca >= thresh01) as i32;
        }
        for y in 0..4usize {
            let a_sels = asel.selectors[y] as u32;
            let sel_v = ca_sel[(a_sels & 3) as usize]
                | (ca_sel[((a_sels >> 2) & 3) as usize] << 2)
                | (ca_sel[((a_sels >> 4) & 3) as usize] << 4)
                | (ca_sel[(a_sels >> 6) as usize] << 6);
            blk.modulation[y] = sel_v as u8;
        }
    } else {
        let mut cy = [0i32; 4];
        let mut ca = [0i32; 4];
        for i in 0..4usize {
            cy[i] = (block_cols[i].r() as i32 - lr) * axis_r
                + (block_cols[i].g() as i32 - lg) * axis_g
                + (block_cols[i].b() as i32 - lb) * axis_b;
            ca[i] = (block_cols[i].a() as i32 - la) * axis_a;
        }
        for y in 0..4usize {
            let c_sels = sel.selectors[y] as u32;
            let a_sels = asel.selectors[y] as u32;
            let d0 = cy[(c_sels & 3) as usize] + ca[(a_sels & 3) as usize];
            let d1 = cy[((c_sels >> 2) & 3) as usize] + ca[((a_sels >> 2) & 3) as usize];
            let d2 = cy[((c_sels >> 4) & 3) as usize] + ca[((a_sels >> 4) & 3) as usize];
            let d3 = cy[(c_sels >> 6) as usize] + ca[(a_sels >> 6) as usize];
            let sel_v = ((d0 >= thresh23) as i32
                + (d0 >= thresh12) as i32
                + (d0 >= thresh01) as i32)
                | (((d1 >= thresh23) as i32 + (d1 >= thresh12) as i32 + (d1 >= thresh01) as i32)
                    << 2)
                | (((d2 >= thresh23) as i32 + (d2 >= thresh12) as i32 + (d2 >= thresh01) as i32)
                    << 4)
                | (((d3 >= thresh23) as i32 + (d3 >= thresh12) as i32 + (d3 >= thresh01) as i32)
                    << 6);
            blk.modulation[y] = sel_v as u8;
        }
    }

    blk.into_bytes()
}

/// 4-component dot product.
#[inline]
fn dot4(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// Normalize a 4-vector in place; a zero vector is left unchanged.
#[inline]
fn normalize(v: &mut [f32; 4]) {
    let s = v[0] * v[0] + v[1] * v[1] + v[2] * v[2] + v[3] * v[3];
    if s != 0.0 {
        let s = 1.0 / crate::mathf::sqrtf(s);
        v[0] *= s;
        v[1] *= s;
        v[2] *= s;
        v[3] *= s;
    }
}

impl Etc1sTranscoder {
    /// Transcode an ETC1S color slice to 8-byte opaque PVRTC2 blocks, written
    /// into `out` (exactly `num_blocks_x*num_blocks_y*8` bytes). `None` on
    /// a corrupt stream.
    pub fn transcode_slice_pvrtc2_rgb(
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
            let block = convert_etc1s_to_pvrtc2_rgb(
                &self.endpoints[ei as usize],
                &self.selectors[si as usize],
            );
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        Some(())
    }

    /// ETC1S -> PVRTC2_4_RGBA when the source has no alpha slice: every block
    /// is fully opaque, so the RGBA target reduces to the opaque RGB convert.
    pub fn transcode_slice_pvrtc2_rgba_opaque(
        &self,
        rgb_slice: &[u8],
        num_blocks_x: u32,
        num_blocks_y: u32,
        video: Option<&mut Vec<u32>>,
        out: &mut [u8],
    ) -> Option<()> {
        self.transcode_slice_pvrtc2_rgb(rgb_slice, num_blocks_x, num_blocks_y, video, out)
    }

    /// Combined ETC1S -> PVRTC2_4_RGBA: color slice + alpha slice, written
    /// into `out` (exactly `num_blocks_x*num_blocks_y*8` bytes). Each block
    /// pairs its color endpoint/selector with the alpha slice's for the
    /// pixel-level re-encode. `video`, if present, is the previous frame's
    /// (color, alpha) index slots for ETC1S video.
    pub fn transcode_image_pvrtc2_rgba(
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
        if out.len() != rgb.len() * 8 {
            return None;
        }
        out.fill(0);
        for (b, (&(rei, rsi), &(aei, asi))) in rgb.iter().zip(alpha.iter()).enumerate() {
            let block = convert_etc1s_to_pvrtc2_rgba(
                &self.endpoints[rei as usize],
                &self.selectors[rsi as usize],
                &self.endpoints[aei as usize],
                &self.selectors[asi as usize],
            );
            out[b * 8..b * 8 + 8].copy_from_slice(&block);
        }
        Some(())
    }
}
