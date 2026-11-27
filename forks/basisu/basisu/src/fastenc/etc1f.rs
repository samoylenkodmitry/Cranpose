//! Real-time ETC1 packer for decoded RGBA pixels, used by the raw-ASTC ETC1
//! and ETC2-color targets. One `PackEtc1State` is threaded through every block
//! of a slice: it memoizes the previous solid block, so the output bytes depend
//! on block order.
//!
//! Everything is integer except the flip/table selection heuristics, which use
//! f32 `sqrt`/`ceil`. Those keep a fixed evaluation order and avoid fused
//! multiply-add, so the packed bytes come out the same on every platform.

// Index-based loops are kept where they map directly onto the multi-array
// indexing; iterator rewrites would obscure the bit layout.
#![allow(clippy::needless_range_loop)]

use super::etc1f_tables::ETC1_MOD_TABS;
use crate::etc::tables::{INTEN_TABLES, SELECTOR_INDEX_TO_ETC1};
use crate::mathf::{ceilf, sqrtf};
use crate::once::OnceBox;
use alloc::boxed::Box;

/// One decoded texel, RGBA byte order (matches `astc::slice::Rgba`).
type Rgba = [u8; 4];

/// Only the first four intensity tables are searched for solid blocks.
const NUM_SOLID_MODS: usize = 4;

/// Per-slice packer state: the previously packed solid color and its block,
/// reused when consecutive blocks are the same solid color.
pub struct PackEtc1State {
    prev_solid_block: [u8; 8],
    prev_solid_r8: i32,
    prev_solid_g8: i32,
    prev_solid_b8: i32,
}

impl PackEtc1State {
    /// A cleared state (no previous solid block).
    pub fn new() -> Self {
        Self {
            prev_solid_block: [0; 8],
            prev_solid_r8: -1,
            prev_solid_g8: -1,
            prev_solid_b8: -1,
        }
    }
}

impl Default for PackEtc1State {
    /// A cleared state, the same as [`PackEtc1State::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Expand a 5-bit value to 8 bits, replicating its top 3 bits into the new low
/// bits.
#[inline]
fn expand5(v: i32) -> i32 {
    (v << 3) | (v >> 2)
}

/// Expand a 4-bit value to 8 bits by nibble replication.
#[inline]
fn expand4(v: i32) -> i32 {
    (v << 4) | v
}

/// Dequantize a 5-bit ETC1 channel to 8 bits (bit replication, same math as
/// [`expand5`]).
#[inline]
fn dequant5(v: i32) -> i32 {
    (v << 3) | (v >> 2)
}

/// Dequantize a 4-bit ETC1 channel to 8 bits by nibble replication.
#[inline]
fn dequant4(v: i32) -> i32 {
    (v << 4) | v
}

/// Sign-extend a 3-bit ETC1 differential delta.
#[inline]
fn dequant_d3(v: i32) -> i32 {
    ((v as i8) << 5 >> 5) as i32
}

/// Clamp a signed value to the 0..=255 byte range.
#[inline]
fn clamp255(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// The tables built once on first use: nearest 5/4-bit quantizers, the
/// per-8-bit-value solid base/error tables over the first four intensity
/// tables, and the 256 precomputed grayscale solid blocks.
struct Tables {
    nearest5: [u8; 256],
    nearest4: [u8; 256],
    /// `[desired8][mod][sel]`.
    solid8_5_base: [[[u8; NUM_SOLID_MODS]; 4]; 256],
    solid8_5_err: [[[u8; NUM_SOLID_MODS]; 4]; 256],
    solid8_4_base: [[[u8; NUM_SOLID_MODS]; 4]; 256],
    solid8_4_err: [[[u8; NUM_SOLID_MODS]; 4]; 256],
    solid_grayscale_blocks: [[u8; 8]; 256],
}

// The solid tables are indexed `[desired8][mod][sel]`: a `[[u8; 4]; 4]` per
// desired 8-bit value, with the intensity table as the middle index and the
// selector as the inner index.

/// The [`Tables`], built on first call and shared for the process lifetime.
fn tables() -> &'static Tables {
    static TABLES: OnceBox<Tables> = OnceBox::new();
    TABLES.get_or_init(|| {
        let mut t = Box::new(Tables {
            nearest5: [0; 256],
            nearest4: [0; 256],
            solid8_5_base: [[[0; 4]; 4]; 256],
            solid8_5_err: [[[0; 4]; 4]; 256],
            solid8_4_base: [[[0; 4]; 4]; 256],
            solid8_4_err: [[[0; 4]; 4]; 256],
            solid_grayscale_blocks: [[0; 8]; 256],
        });
        for i in 0..256i32 {
            let mut best_e = i32::MAX;
            let mut best_idx = 0;
            for s in 0..32 {
                let recovered = (s << 3) | (s >> 2);
                let e = (recovered - i).abs();
                if e < best_e {
                    best_e = e;
                    best_idx = s;
                }
            }
            t.nearest5[i as usize] = best_idx as u8;

            let mut best_e = i32::MAX;
            let mut best_idx = 0;
            for s in 0..16 {
                let recovered = (s << 4) | s;
                let e = (recovered - i).abs();
                if e < best_e {
                    best_e = e;
                    best_idx = s;
                }
            }
            t.nearest4[i as usize] = best_idx as u8;
        }
        for desired8 in 0..256i32 {
            for m in 0..NUM_SOLID_MODS {
                for sel in 0..4 {
                    let mut best_err = i32::MAX;
                    let mut best_base = 0u32;
                    for b in 0..32 {
                        let val = (dequant5(b) + INTEN_TABLES[m][sel]).clamp(0, 255);
                        let err = (val - desired8).abs();
                        if err < best_err {
                            best_err = err;
                            best_base = b as u32;
                            if best_err == 0 {
                                break;
                            }
                        }
                    }
                    t.solid8_5_base[desired8 as usize][m][sel] = best_base as u8;
                    t.solid8_5_err[desired8 as usize][m][sel] =
                        255u32.min((best_err * best_err) as u32) as u8;

                    let mut best_err = i32::MAX;
                    let mut best_base = 0u32;
                    for b in 0..16 {
                        let val = (dequant4(b) + INTEN_TABLES[m][sel]).clamp(0, 255);
                        let err = (val - desired8).abs();
                        if err < best_err {
                            best_err = err;
                            best_base = b as u32;
                            if best_err == 0 {
                                break;
                            }
                        }
                    }
                    t.solid8_4_base[desired8 as usize][m][sel] = best_base as u8;
                    t.solid8_4_err[desired8 as usize][m][sel] =
                        255u32.min((best_err * best_err) as u32) as u8;
                }
            }
        }
        // The grayscale solid blocks are packed with the just-built tables;
        // init_flag skips the memo lookups, which are not populated yet.
        let mut state = PackEtc1State::new();
        for i in 0..256u32 {
            let mut blk = [0u8; 8];
            pack_etc1_solid_with(
                &t,
                &mut blk,
                [i as u8, i as u8, i as u8, 255],
                &mut state,
                true,
            );
            t.solid_grayscale_blocks[i as usize] = blk;
        }
        t
    })
}

/// Decode both subblocks' four candidate colors from a packed ETC1 header;
/// only RGB is produced (alpha is unused downstream).
fn get_block_colors(block: &[u8; 8]) -> [[[u8; 3]; 4]; 2] {
    let (b0, b1, b2, b3) = (
        block[0] as i32,
        block[1] as i32,
        block[2] as i32,
        block[3] as i32,
    );
    let mut base8 = [[0i32; 2]; 3]; // [comp][subblock]
    if b3 & 2 != 0 {
        // diff mode
        for (c, b) in [b0, b1, b2].into_iter().enumerate() {
            base8[c][0] = dequant5(b >> 3);
            base8[c][1] = dequant5(((b >> 3) + dequant_d3(b & 7)).clamp(0, 31));
        }
    } else {
        // abs mode
        for (c, b) in [b0, b1, b2].into_iter().enumerate() {
            base8[c][0] = dequant4(b >> 4);
            base8[c][1] = dequant4(b & 15);
        }
    }
    let tabs = [
        &INTEN_TABLES[(b3 >> 5) as usize],
        &INTEN_TABLES[((b3 >> 2) & 7) as usize],
    ];
    let mut out = [[[0u8; 3]; 4]; 2];
    for sb in 0..2 {
        for i in 0..4 {
            let d = tabs[sb][i];
            for c in 0..3 {
                out[sb][i][c] = clamp255(base8[c][sb] + d);
            }
        }
    }
    out
}

/// Grayscale variant of [`get_block_colors`]: only the luma (red-channel) base
/// is decoded.
fn get_block_colors_y(block: &[u8; 8]) -> [[u8; 4]; 2] {
    let (b0, b3) = (block[0] as i32, block[3] as i32);
    let base8_y = if b3 & 2 != 0 {
        [
            dequant5(b0 >> 3),
            dequant5(((b0 >> 3) + dequant_d3(b0 & 7)).clamp(0, 31)),
        ]
    } else {
        [dequant4(b0 >> 4), dequant4(b0 & 15)]
    };
    let tabs = [
        &INTEN_TABLES[(b3 >> 5) as usize],
        &INTEN_TABLES[((b3 >> 2) & 7) as usize],
    ];
    let mut out = [[0u8; 4]; 2];
    for sb in 0..2 {
        for i in 0..4 {
            out[sb][i] = clamp255(base8_y[sb] + tabs[sb][i]);
        }
    }
    out
}

/// Floor-quantize an 8-bit value to a 5-bit code.
#[inline]
fn q5_floor(x: i32) -> i32 {
    (x * 31) / 255
}

/// Floor-quantize an 8-bit value to a 4-bit code.
#[inline]
fn q4_floor(x: i32) -> i32 {
    (x * 15) / 255
}

/// `x` squared.
#[inline]
fn squarei(x: i32) -> i32 {
    x * x
}

/// Quantize an RGB mean to 5:5:5, minimizing the chroma (channel-difference)
/// energy over the eight floor/ceil combinations.
fn corr_round_555(r: i32, g: i32, b: i32) -> [i32; 3] {
    let (rl, gl, bl) = (q5_floor(r), q5_floor(g), q5_floor(b));
    let (rh, gh, bh) = (31.min(rl + 1), 31.min(gl + 1), 31.min(bl + 1));
    let r8 = [expand5(rl), expand5(rh)];
    let g8 = [expand5(gl), expand5(gh)];
    let b8 = [expand5(bl), expand5(bh)];

    let (mut br, mut bg, mut bb) = (r8[0], g8[0], b8[0]);
    let (er, eg, eb) = (r - br, g - bg, b - bb);
    let mut best_j = squarei(er - eg) + squarei(eg - eb) + squarei(eb - er);
    for m in 1..8 {
        let (tr, tg, tb) = (r8[m & 1], g8[(m >> 1) & 1], b8[(m >> 2) & 1]);
        let (er, eg, eb) = (r - tr, g - tg, b - tb);
        let j = squarei(er - eg) + squarei(eg - eb) + squarei(eb - er);
        if j < best_j {
            best_j = j;
            br = tr;
            bg = tg;
            bb = tb;
        }
    }
    [br >> 3, bg >> 3, bb >> 3]
}

/// The 4:4:4 twin of [`corr_round_555`].
fn corr_round_444(r: i32, g: i32, b: i32) -> [i32; 3] {
    let (rl, gl, bl) = (q4_floor(r), q4_floor(g), q4_floor(b));
    let (rh, gh, bh) = (15.min(rl + 1), 15.min(gl + 1), 15.min(bl + 1));
    let r8 = [expand4(rl), expand4(rh)];
    let g8 = [expand4(gl), expand4(gh)];
    let b8 = [expand4(bl), expand4(bh)];

    let (mut br, mut bg, mut bb) = (r8[0], g8[0], b8[0]);
    let (er, eg, eb) = (r - br, g - bg, b - bb);
    let mut best_j = squarei(er - eg) + squarei(eg - eb) + squarei(eb - er);
    for m in 1..8 {
        let (tr, tg, tb) = (r8[m & 1], g8[(m >> 1) & 1], b8[(m >> 2) & 1]);
        let (er, eg, eb) = (r - tr, g - tg, b - tb);
        let j = squarei(er - eg) + squarei(eg - eb) + squarei(eb - er);
        if j < best_j {
            best_j = j;
            br = tr;
            bg = tg;
            bb = tb;
        }
    }
    [br >> 4, bg >> 4, bb >> 4]
}

/// Pack one solid-color ETC1 block. `init_flag` skips the grayscale/memo
/// lookups while the tables themselves are being built.
fn pack_etc1_solid_with(
    t: &Tables,
    block: &mut [u8; 8],
    color: Rgba,
    state: &mut PackEtc1State,
    init_flag: bool,
) {
    let (r8, g8, b8) = (color[0] as usize, color[1] as usize, color[2] as usize);

    if !init_flag {
        if r8 == g8 && r8 == b8 {
            *block = t.solid_grayscale_blocks[r8];
            return;
        }
        if state.prev_solid_r8 == r8 as i32
            && state.prev_solid_g8 == g8 as i32
            && state.prev_solid_b8 == b8 as i32
        {
            *block = state.prev_solid_block;
            return;
        }
    }

    let mut best_err = u32::MAX;
    let (mut best_mod, mut best_sel) = (0usize, 0usize);
    let mut best4_flag = false;

    const RW: u32 = 2;
    const GW: u32 = 4;

    'search: for m in 0..NUM_SOLID_MODS {
        for sel in 0..4 {
            let total_err5 = RW * t.solid8_5_err[r8][m][sel] as u32
                + GW * t.solid8_5_err[g8][m][sel] as u32
                + t.solid8_5_err[b8][m][sel] as u32;
            if total_err5 < best_err {
                best_err = total_err5;
                best_mod = m;
                best_sel = sel;
                best4_flag = false;
                if best_err == 0 {
                    break 'search;
                }
            }
            let total_err4 = RW * t.solid8_4_err[r8][m][sel] as u32
                + GW * t.solid8_4_err[g8][m][sel] as u32
                + t.solid8_4_err[b8][m][sel] as u32;
            if total_err4 < best_err {
                best_err = total_err4;
                best_mod = m;
                best_sel = sel;
                best4_flag = true;
            }
        }
    }

    if best4_flag {
        let r4 = t.solid8_4_base[r8][best_mod][best_sel] as u32;
        let g4 = t.solid8_4_base[g8][best_mod][best_sel] as u32;
        let b4 = t.solid8_4_base[b8][best_mod][best_sel] as u32;
        block[0] = (r4 | (r4 << 4)) as u8;
        block[1] = (g4 | (g4 << 4)) as u8;
        block[2] = (b4 | (b4 << 4)) as u8;
    } else {
        block[0] = t.solid8_5_base[r8][best_mod][best_sel] << 3;
        block[1] = t.solid8_5_base[g8][best_mod][best_sel] << 3;
        block[2] = t.solid8_5_base[b8][best_mod][best_sel] << 3;
    }

    let flip = 0u32;
    let diff = u32::from(!best4_flag);
    block[3] = (flip | (diff << 1) | ((best_mod as u32) << 5) | ((best_mod as u32) << 2)) as u8;

    let etc1_sels = SELECTOR_INDEX_TO_ETC1[best_sel] as u32;
    let lb = if etc1_sels & 2 != 0 { 0xFF } else { 0 };
    block[4] = lb;
    block[5] = lb;
    let hb = if etc1_sels & 1 != 0 { 0xFF } else { 0 };
    block[6] = hb;
    block[7] = hb;

    state.prev_solid_r8 = r8 as i32;
    state.prev_solid_g8 = g8 as i32;
    state.prev_solid_b8 = b8 as i32;
    state.prev_solid_block = *block;
}

/// Texel index -> vertical-split subblock (0 = left, 1 = right).
const S_VI: [usize; 16] = [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1];
/// Texel index -> horizontal-split accumulator slot (2 = top, 3 = bottom).
const S_HI: [usize; 16] = [2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3];
/// `[flip][texel]` -> subblock.
const S_SUBSETS: [[usize; 16]; 2] = [
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1],
];
/// `[flip*8 + subblock*4 + sel]` -> (low, high) selector bitmasks for a whole
/// subblock of one uniform selector.
const S_SEL_BITMASKS: [[u16; 2]; 16] = [
    [0xff, 0xff],
    [0x0, 0xff],
    [0x0, 0x0],
    [0xff, 0x0],
    [0xff00, 0xff00],
    [0x0, 0xff00],
    [0x0, 0x0],
    [0xff00, 0x0],
    [0x3333, 0x3333],
    [0x0, 0x3333],
    [0x0, 0x0],
    [0x3333, 0x0],
    [0xcccc, 0xcccc],
    [0x0, 0xcccc],
    [0x0, 0x0],
    [0xcccc, 0x0],
];

/// Pack a two-flat-subblock ETC1 block: each subblock is one solid color, with
/// selectors uniform per subblock. The mod/sel search packs candidate errors as
/// `(err << 5) | (mod << 2) | sel` so a single `min` tracks the winner.
fn pack_etc1_solid_subblocks(t: &Tables, block: &mut [u8; 8], means: [[u8; 4]; 2], flip: u32) {
    let mut best_mod5 = [0usize; 2];
    let mut best_sel5 = [0usize; 2];
    let mut best_base5 = [[0u32; 3]; 2];
    let mut best_err5 = [u32::MAX; 2];
    let mut best_mod4 = [0usize; 2];
    let mut best_sel4 = [0usize; 2];
    let mut best_base4 = [[0u32; 3]; 2];
    let mut best_err4 = [u32::MAX; 2];

    const RW: u32 = 2;
    const GW: u32 = 4;

    for sb in 0..2 {
        let (r8, g8, b8) = (
            means[sb][0] as usize,
            means[sb][1] as usize,
            means[sb][2] as usize,
        );
        for m in 0..NUM_SOLID_MODS {
            let mod4 = (m as u32) << 2;
            for sel in 0..4 {
                let e5 = ((RW * t.solid8_5_err[r8][m][sel] as u32
                    + GW * t.solid8_5_err[g8][m][sel] as u32
                    + t.solid8_5_err[b8][m][sel] as u32)
                    << 5)
                    + (mod4 + sel as u32);
                best_err5[sb] = best_err5[sb].min(e5);
                let e4 = ((RW * t.solid8_4_err[r8][m][sel] as u32
                    + GW * t.solid8_4_err[g8][m][sel] as u32
                    + t.solid8_4_err[b8][m][sel] as u32)
                    << 5)
                    + (mod4 + sel as u32);
                best_err4[sb] = best_err4[sb].min(e4);
            }
        }
        best_mod5[sb] = ((best_err5[sb] >> 2) & 7) as usize;
        best_sel5[sb] = (best_err5[sb] & 3) as usize;
        best_err5[sb] >>= 5;
        best_mod4[sb] = ((best_err4[sb] >> 2) & 7) as usize;
        best_sel4[sb] = (best_err4[sb] & 3) as usize;
        best_err4[sb] >>= 5;

        best_base5[sb] = [
            t.solid8_5_base[r8][best_mod5[sb]][best_sel5[sb]] as u32,
            t.solid8_5_base[g8][best_mod5[sb]][best_sel5[sb]] as u32,
            t.solid8_5_base[b8][best_mod5[sb]][best_sel5[sb]] as u32,
        ];
        best_base4[sb] = [
            t.solid8_4_base[r8][best_mod4[sb]][best_sel4[sb]] as u32,
            t.solid8_4_base[g8][best_mod4[sb]][best_sel4[sb]] as u32,
            t.solid8_4_base[b8][best_mod4[sb]][best_sel4[sb]] as u32,
        ];
    }

    let total_err4 = best_err4[0] + best_err4[1];
    let total_err5 = best_err5[0] + best_err5[1];
    let mut use_abs = total_err4 < total_err5;
    if !use_abs {
        let dr = best_base5[1][0] as i32 - best_base5[0][0] as i32;
        let dg = best_base5[1][1] as i32 - best_base5[0][1] as i32;
        let db = best_base5[1][2] as i32 - best_base5[0][2] as i32;
        if !(-4..=3).contains(&dr) || !(-4..=3).contains(&dg) || !(-4..=3).contains(&db) {
            use_abs = true;
        }
    }

    let best_sels = if use_abs {
        for c in 0..3 {
            block[c] = (best_base4[1][c] | (best_base4[0][c] << 4)) as u8;
        }
        let diff = 0u32;
        block[3] =
            (flip | (diff << 1) | ((best_mod4[0] as u32) << 5) | ((best_mod4[1] as u32) << 2))
                as u8;
        best_sel4
    } else {
        for c in 0..3 {
            let delta = (best_base5[1][c] as i32 - best_base5[0][c] as i32) & 7;
            block[c] = (delta as u32 | (best_base5[0][c] << 3)) as u8;
        }
        let diff = 1u32;
        block[3] =
            (flip | (diff << 1) | ((best_mod5[0] as u32) << 5) | ((best_mod5[1] as u32) << 2))
                as u8;
        best_sel5
    };

    let mut l_bitmask = 0u16;
    let mut h_bitmask = 0u16;
    for sb in 0..2 {
        let idx = (flip as usize) * 8 + sb * 4 + best_sels[sb];
        l_bitmask |= S_SEL_BITMASKS[idx][0];
        h_bitmask |= S_SEL_BITMASKS[idx][1];
    }
    block[7] = l_bitmask as u8;
    block[6] = (l_bitmask >> 8) as u8;
    block[5] = h_bitmask as u8;
    block[4] = (h_bitmask >> 8) as u8;
}

/// BT.709 luma of an RGBA texel, rounded half-up.
#[inline]
fn get_709_luma(c: Rgba) -> u32 {
    (13938 * c[0] as u32 + 46869 * c[1] as u32 + 4729 * c[2] as u32 + 32768) >> 16
}

/// Pack one 4x4 RGBA block as ETC1. Solid and low-chroma blocks short-circuit
/// to the solid/grayscale packers; otherwise a flip is chosen by per-half
/// deviation, each subblock's mean is correlated-rounded, and an intensity
/// table is picked from the span/stddev heuristic table.
pub fn pack_etc1(block: &mut [u8; 8], pixels: &[Rgba; 16], state: &mut PackEtc1State) {
    let t = tables();

    // Solid block check, ignoring alpha.
    let rgb = |c: Rgba| [c[0], c[1], c[2]];
    if rgb(pixels[0]) == rgb(pixels[15]) && pixels[1..15].iter().all(|&p| rgb(p) == rgb(pixels[0]))
    {
        pack_etc1_solid_with(t, block, pixels[0], state, false);
        return;
    }

    // [0]=left, [1]=right, [2]=top, [3]=bottom accumulators.
    let mut accum_y = [0i32; 4];
    let mut accum_y2 = [0i32; 4];
    let mut accum_c2 = [0i32; 4];
    let mut total_c2 = 0i32;
    let mut max_c2 = 0i32;
    for i in 0..16 {
        let (r, g, b) = (
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
        );
        let (rg, bg) = (r - g, b - g);
        let y = (r + g + b + 1) / 3;
        let y2 = y * y;
        let c2 = rg * rg + bg * bg;
        total_c2 += c2;
        max_c2 = max_c2.max(c2);
        accum_y[S_VI[i]] += y;
        accum_y2[S_VI[i]] += y2;
        accum_c2[S_VI[i]] += c2;
        accum_y[S_HI[i]] += y;
        accum_y2[S_HI[i]] += y2;
        accum_c2[S_HI[i]] += c2;
    }

    // Low-chroma blocks route to the grayscale packer on BT.709 luma.
    const CHROMA_ENERGY_SUM_THRESH: i32 = 300;
    const CHROMA_ENERGY_MAX_THRESH: i32 = 32;
    if total_c2 < CHROMA_ENERGY_SUM_THRESH && max_c2 < CHROMA_ENERGY_MAX_THRESH {
        let mut y_pixels = [0u8; 16];
        if total_c2 == 0 {
            for i in 0..16 {
                y_pixels[i] = pixels[i][0];
            }
        } else {
            for i in 0..16 {
                y_pixels[i] = get_709_luma(pixels[i]) as u8;
            }
        }
        pack_etc1_grayscale(block, &y_pixels, state);
        return;
    }

    let mut std_luma = [0f32; 4];
    let mut std_chroma = [0f32; 4];
    for i in 0..4 {
        let var_y_scaled = 0.max((accum_y2[i] << 3) - accum_y[i] * accum_y[i]);
        std_luma[i] = sqrtf(var_y_scaled as f32 * (1.0 / 64.0));
        std_chroma[i] = sqrtf(accum_c2[i] as f32 * (1.0 / 8.0));
    }
    const LUMA_SCALE: f32 = 2.0;
    const CHROMA_SCALE: f32 = 1.0;
    let flip0_score =
        (std_luma[0] + std_luma[1]) * LUMA_SCALE + (std_chroma[0] + std_chroma[1]) * CHROMA_SCALE;
    let flip1_score =
        (std_luma[2] + std_luma[3]) * LUMA_SCALE + (std_chroma[2] + std_chroma[3]) * CHROMA_SCALE;
    let flip = u32::from(flip1_score < flip0_score);

    let mut var8_y = [0i32; 2];
    let mut mean8_y = [0i32; 2];
    let mut mean8_r = [0i32; 2];
    let mut mean8_g = [0i32; 2];
    let mut mean8_b = [0i32; 2];
    let mut min_y = [i32::MAX; 2];
    let mut max_y = [i32::MIN; 2];
    for i in 0..16 {
        let (r, g, b) = (
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
        );
        let y = (r + g + b + 1) / 3;
        let s = S_SUBSETS[flip as usize][i];
        var8_y[s] += y * y;
        mean8_y[s] += y;
        mean8_r[s] += r;
        mean8_g[s] += g;
        mean8_b[s] += b;
        min_y[s] = min_y[s].min(y);
        max_y[s] = max_y[s].max(y);
    }

    if max_y[0] - min_y[0] < 8 && max_y[1] - min_y[1] < 8 {
        let means = [
            [
                ((mean8_r[0] + 4) / 8) as u8,
                ((mean8_g[0] + 4) / 8) as u8,
                ((mean8_b[0] + 4) / 8) as u8,
                255,
            ],
            [
                ((mean8_r[1] + 4) / 8) as u8,
                ((mean8_g[1] + 4) / 8) as u8,
                ((mean8_b[1] + 4) / 8) as u8,
                255,
            ],
        ];
        if means[0] == means[1] {
            pack_etc1_solid_with(t, block, means[0], state, false);
        } else {
            pack_etc1_solid_subblocks(t, block, means, flip);
        }
        return;
    }

    let mut half_span8_y = [0i32; 2];
    let mut stddev_y = [0f32; 2];
    for i in 0..2 {
        var8_y[i] = 0.max((var8_y[i] << 3) - mean8_y[i] * mean8_y[i]);
        stddev_y[i] = sqrtf(var8_y[i] as f32) * (1.0 / 8.0);
        mean8_y[i] = (mean8_y[i] + 4) >> 3;
        mean8_r[i] = (mean8_r[i] + 4) >> 3;
        mean8_g[i] = (mean8_g[i] + 4) >> 3;
        mean8_b[i] = (mean8_b[i] + 4) >> 3;
        half_span8_y[i] = (max_y[i] - mean8_y[i]).max(mean8_y[i] - min_y[i]);
    }

    let stddev = [
        ((ceilf(9.0 * (stddev_y[0] / 1.max(half_span8_y[0]) as f32)) as i32) - 1).clamp(0, 7),
        ((ceilf(9.0 * (stddev_y[1] / 1.max(half_span8_y[1]) as f32)) as i32) - 1).clamp(0, 7),
    ];
    let mod_tab = [
        ETC1_MOD_TABS[half_span8_y[0].clamp(1, 255) as usize][stddev[0] as usize] as u32,
        ETC1_MOD_TABS[half_span8_y[1].clamp(1, 255) as usize][stddev[1] as usize] as u32,
    ];

    let mean5: [[i32; 3]; 2] = [
        corr_round_555(mean8_r[0], mean8_g[0], mean8_b[0]),
        corr_round_555(mean8_r[1], mean8_g[1], mean8_b[1]),
    ];
    let delta5 = [
        mean5[1][0] - mean5[0][0],
        mean5[1][1] - mean5[0][1],
        mean5[1][2] - mean5[0][2],
    ];
    let z = ((delta5[0] + 4) | (delta5[1] + 4) | (delta5[2] + 4)) as u32;
    let use_abs_colors4 = z > 7;

    if use_abs_colors4 {
        let mean4: [[i32; 3]; 2] = [
            corr_round_444(mean8_r[0], mean8_g[0], mean8_b[0]),
            corr_round_444(mean8_r[1], mean8_g[1], mean8_b[1]),
        ];
        for c in 0..3 {
            block[c] = (mean4[1][c] | (mean4[0][c] << 4)) as u8;
        }
        let diff = 0u32;
        block[3] = (flip | (diff << 1) | (mod_tab[0] << 5) | (mod_tab[1] << 2)) as u8;
    } else {
        for c in 0..3 {
            block[c] = (((delta5[c] & 7) as u32) | ((mean5[0][c] as u32) << 3)) as u8;
        }
        let diff = 1u32;
        block[3] = (flip | (diff << 1) | (mod_tab[0] << 5) | (mod_tab[1] << 2)) as u8;
    }

    let mut l_bitmask = 0u16;
    let mut h_bitmask = 0u16;
    const S_TRAN: [u16; 4] = [1, 0, 2, 3];

    let subblock_colors = get_block_colors(block);
    for subblock in 0..2usize {
        let bc = &subblock_colors[subblock];
        let mut block_y = [0u32; 4];
        for i in 0..4 {
            block_y[i] = bc[i][0] as u32 * 54 + bc[i][1] as u32 * 183 + bc[i][2] as u32 * 19;
        }
        let y01 = block_y[0] + block_y[1];
        let y12 = block_y[1] + block_y[2];
        let y23 = block_y[2] + block_y[3];

        if flip != 0 {
            let mut ofs = subblock * 2;
            for y in 0..2 {
                for x in 0..4 {
                    let c = pixels[x + (subblock * 2 + y) * 4];
                    let l = c[0] as u32 * 108 + c[1] as u32 * 366 + c[2] as u32 * 38;
                    let t4 =
                        S_TRAN[usize::from(l < y01) + usize::from(l < y12) + usize::from(l < y23)];
                    l_bitmask |= (t4 & 1) << ofs;
                    h_bitmask |= (t4 >> 1) << ofs;
                    ofs += 4;
                }
                ofs = ofs + 1 - 16;
            }
        } else {
            let mut ofs = subblock * 2 * 4;
            for x in 0..2 {
                for y in 0..4 {
                    let c = pixels[subblock * 2 + x + y * 4];
                    let l = c[0] as u32 * 108 + c[1] as u32 * 366 + c[2] as u32 * 38;
                    let t4 =
                        S_TRAN[usize::from(l < y01) + usize::from(l < y12) + usize::from(l < y23)];
                    l_bitmask |= (t4 & 1) << ofs;
                    h_bitmask |= (t4 >> 1) << ofs;
                    ofs += 1;
                }
            }
        }

        block[7] = l_bitmask as u8;
        block[6] = (l_bitmask >> 8) as u8;
        block[5] = h_bitmask as u8;
        block[4] = (h_bitmask >> 8) as u8;
    }
}

/// Grayscale twin of [`pack_etc1_solid_subblocks`].
fn pack_etc1_grayscale_solid_subblocks(t: &Tables, block: &mut [u8; 8], means: [u8; 2], flip: u32) {
    let mut best_mod5 = [0usize; 2];
    let mut best_sel5 = [0usize; 2];
    let mut best_base5 = [0u32; 2];
    let mut best_err5 = [u32::MAX; 2];
    let mut best_mod4 = [0usize; 2];
    let mut best_sel4 = [0usize; 2];
    let mut best_base4 = [0u32; 2];
    let mut best_err4 = [u32::MAX; 2];

    for sb in 0..2 {
        let y8 = means[sb] as usize;
        for m in 0..NUM_SOLID_MODS {
            let mod4 = (m as u32) << 2;
            for sel in 0..4 {
                let e5 = ((t.solid8_5_err[y8][m][sel] as u32) << 5) + (mod4 + sel as u32);
                best_err5[sb] = best_err5[sb].min(e5);
                let e4 = ((t.solid8_4_err[y8][m][sel] as u32) << 5) + (mod4 + sel as u32);
                best_err4[sb] = best_err4[sb].min(e4);
            }
        }
        best_mod5[sb] = ((best_err5[sb] >> 2) & 7) as usize;
        best_sel5[sb] = (best_err5[sb] & 3) as usize;
        best_err5[sb] >>= 5;
        best_mod4[sb] = ((best_err4[sb] >> 2) & 7) as usize;
        best_sel4[sb] = (best_err4[sb] & 3) as usize;
        best_err4[sb] >>= 5;
        best_base5[sb] = t.solid8_5_base[y8][best_mod5[sb]][best_sel5[sb]] as u32;
        best_base4[sb] = t.solid8_4_base[y8][best_mod4[sb]][best_sel4[sb]] as u32;
    }

    let mut use_abs = best_err4[0] + best_err4[1] < best_err5[0] + best_err5[1];
    if !use_abs {
        let dy = best_base5[1] as i32 - best_base5[0] as i32;
        if !(-4..=3).contains(&dy) {
            use_abs = true;
        }
    }

    let best_sels = if use_abs {
        let v = (best_base4[1] | (best_base4[0] << 4)) as u8;
        block[0] = v;
        block[1] = v;
        block[2] = v;
        let diff = 0u32;
        block[3] =
            (flip | (diff << 1) | ((best_mod4[0] as u32) << 5) | ((best_mod4[1] as u32) << 2))
                as u8;
        best_sel4
    } else {
        let delta = (best_base5[1] as i32 - best_base5[0] as i32) & 7;
        let v = (delta as u32 | (best_base5[0] << 3)) as u8;
        block[0] = v;
        block[1] = v;
        block[2] = v;
        let diff = 1u32;
        block[3] =
            (flip | (diff << 1) | ((best_mod5[0] as u32) << 5) | ((best_mod5[1] as u32) << 2))
                as u8;
        best_sel5
    };

    let mut l_bitmask = 0u16;
    let mut h_bitmask = 0u16;
    for sb in 0..2 {
        let idx = (flip as usize) * 8 + sb * 4 + best_sels[sb];
        l_bitmask |= S_SEL_BITMASKS[idx][0];
        h_bitmask |= S_SEL_BITMASKS[idx][1];
    }
    block[7] = l_bitmask as u8;
    block[6] = (l_bitmask >> 8) as u8;
    block[5] = h_bitmask as u8;
    block[4] = (h_bitmask >> 8) as u8;
}

/// Pack one 4x4 grayscale block as ETC1: the alpha-to-opaque route and the
/// low-chroma route of [`pack_etc1`].
pub fn pack_etc1_grayscale(block: &mut [u8; 8], pixels: &[u8; 16], state: &mut PackEtc1State) {
    // `state` is accepted for signature symmetry with `pack_etc1`; this path
    // has no solid-block memo to update, so it is left untouched.
    let _ = &state;
    let t = tables();

    if pixels[0] == pixels[15] && pixels[1..15].iter().all(|&p| p == pixels[0]) {
        *block = t.solid_grayscale_blocks[pixels[0] as usize];
        return;
    }

    let mut accum_y = [0i32; 4];
    let mut accum_y2 = [0i32; 4];
    for i in 0..16 {
        let y = pixels[i] as i32;
        accum_y[S_VI[i]] += y;
        accum_y2[S_VI[i]] += y * y;
        accum_y[S_HI[i]] += y;
        accum_y2[S_HI[i]] += y * y;
    }

    let mut std_luma = [0f32; 4];
    for i in 0..4 {
        let var_y_scaled = 0.max((accum_y2[i] << 3) - accum_y[i] * accum_y[i]);
        std_luma[i] = sqrtf(var_y_scaled as f32 * (1.0 / 64.0));
    }
    let flip0_score = std_luma[0] + std_luma[1];
    let flip1_score = std_luma[2] + std_luma[3];
    let flip = u32::from(flip1_score < flip0_score);

    let mut var8_y = [0i32; 2];
    let mut mean8_y = [0i32; 2];
    let mut min_y = [i32::MAX; 2];
    let mut max_y = [i32::MIN; 2];
    for i in 0..16 {
        let y = pixels[i] as i32;
        let s = S_SUBSETS[flip as usize][i];
        var8_y[s] += y * y;
        mean8_y[s] += y;
        min_y[s] = min_y[s].min(y);
        max_y[s] = max_y[s].max(y);
    }

    if max_y[0] - min_y[0] < 8 && max_y[1] - min_y[1] < 8 {
        let means = [((mean8_y[0] + 4) / 8) as u8, ((mean8_y[1] + 4) / 8) as u8];
        if means[0] == means[1] {
            *block = t.solid_grayscale_blocks[means[0] as usize];
        } else {
            pack_etc1_grayscale_solid_subblocks(t, block, means, flip);
        }
        return;
    }

    let mut half_span8_y = [0i32; 2];
    let mut stddev_y = [0f32; 2];
    for i in 0..2 {
        var8_y[i] = 0.max((var8_y[i] << 3) - mean8_y[i] * mean8_y[i]);
        stddev_y[i] = sqrtf(var8_y[i] as f32) * (1.0 / 8.0);
        mean8_y[i] = (mean8_y[i] + 4) >> 3;
        half_span8_y[i] = (max_y[i] - mean8_y[i]).max(mean8_y[i] - min_y[i]);
    }

    let stddev = [
        ((ceilf(9.0 * (stddev_y[0] / 1.max(half_span8_y[0]) as f32)) as i32) - 1).clamp(0, 7),
        ((ceilf(9.0 * (stddev_y[1] / 1.max(half_span8_y[1]) as f32)) as i32) - 1).clamp(0, 7),
    ];
    let mod_tab = [
        ETC1_MOD_TABS[half_span8_y[0].clamp(1, 255) as usize][stddev[0] as usize] as u32,
        ETC1_MOD_TABS[half_span8_y[1].clamp(1, 255) as usize][stddev[1] as usize] as u32,
    ];

    let mean5_y = [
        t.nearest5[mean8_y[0] as usize] as i32,
        t.nearest5[mean8_y[1] as usize] as i32,
    ];
    let delta5_y = mean5_y[1] - mean5_y[0];
    let use_abs_colors4 = (delta5_y + 4) as u32 > 7;

    if use_abs_colors4 {
        let mean4_y = [
            t.nearest4[mean8_y[0] as usize] as u32,
            t.nearest4[mean8_y[1] as usize] as u32,
        ];
        let v = (mean4_y[1] | (mean4_y[0] << 4)) as u8;
        block[0] = v;
        block[1] = v;
        block[2] = v;
        let diff = 0u32;
        block[3] = (flip | (diff << 1) | (mod_tab[0] << 5) | (mod_tab[1] << 2)) as u8;
    } else {
        let v = (((delta5_y & 7) as u32) | ((mean5_y[0] as u32) << 3)) as u8;
        block[0] = v;
        block[1] = v;
        block[2] = v;
        let diff = 1u32;
        block[3] = (flip | (diff << 1) | (mod_tab[0] << 5) | (mod_tab[1] << 2)) as u8;
    }

    let mut l_bitmask = 0u16;
    let mut h_bitmask = 0u16;
    const S_TRAN: [u16; 4] = [1, 0, 2, 3];

    let subblock_colors = get_block_colors_y(block);
    for subblock in 0..2usize {
        let by = &subblock_colors[subblock];
        let y01 = by[0] as u32 + by[1] as u32;
        let y12 = by[1] as u32 + by[2] as u32;
        let y23 = by[2] as u32 + by[3] as u32;

        if flip != 0 {
            let mut ofs = subblock * 2;
            for y in 0..2 {
                for x in 0..4 {
                    let l = pixels[x + (subblock * 2 + y) * 4] as u32 * 2;
                    let t4 =
                        S_TRAN[usize::from(l < y01) + usize::from(l < y12) + usize::from(l < y23)];
                    l_bitmask |= (t4 & 1) << ofs;
                    h_bitmask |= (t4 >> 1) << ofs;
                    ofs += 4;
                }
                ofs = ofs + 1 - 16;
            }
        } else {
            let mut ofs = subblock * 2 * 4;
            for x in 0..2 {
                for y in 0..4 {
                    let l = pixels[subblock * 2 + x + y * 4] as u32 * 2;
                    let t4 =
                        S_TRAN[usize::from(l < y01) + usize::from(l < y12) + usize::from(l < y23)];
                    l_bitmask |= (t4 & 1) << ofs;
                    h_bitmask |= (t4 >> 1) << ofs;
                    ofs += 1;
                }
            }
        }

        block[7] = l_bitmask as u8;
        block[6] = (l_bitmask >> 8) as u8;
        block[5] = h_bitmask as u8;
        block[4] = (h_bitmask >> 8) as u8;
    }
}
