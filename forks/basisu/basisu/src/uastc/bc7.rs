//! UASTC to BC7 transcode. `build_bc7_results` turns an unpacked UASTC block
//! into a BC7 block description (the BC7 mode, endpoints, and selectors for that
//! UASTC mode) and `encode_bc7_block` packs it into 16 bytes. The mode-5 and
//! mode-6 optimal single-color endpoint tables (built once and cached) feed
//! the solid-color path.

use alloc::boxed::Box;

/// One optimal-endpoint table entry: the best (lo, hi) endpoint pair found for
/// a target value, plus its squared error. `#[repr(C)]` with this field order
/// gives a padding-free 4-byte entry, so a table of these has a stable byte
/// layout.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct EndpointErr {
    pub m_error: u16,
    pub m_lo: u8,
    pub m_hi: u8,
}

/// Both single-color searches interpolate at weight 21 of 64: the standard BC7
/// 4-bit weight at index 5 (`BC7ENC_MODE_6_OPTIMAL_INDEX`) and 2-bit weight at
/// index 1 (`BC7ENC_MODE_5_OPTIMAL_INDEX`) are both 21.
const OPTIMAL_WEIGHT: u32 = 21;

/// Build the mode-6 (BC7 777.1) optimal single-color endpoint table, indexed
/// `[target][pbit]`: for each target `c` and pbit `lp` (applied to both
/// endpoints), the (lo, hi) 7+pbit endpoints minimizing squared error at the
/// mode-6 interpolation weight.
pub fn build_mode6_optimal() -> Box<[[EndpointErr; 2]; 256]> {
    let mut t = Box::new([[EndpointErr::default(); 2]; 256]);
    for c in 0..256u32 {
        for lp in 0..2u32 {
            let mut best = EndpointErr {
                m_error: u16::MAX,
                m_lo: 0,
                m_hi: 0,
            };
            for l in 0..128u32 {
                let low = (l << 1) | lp;
                for h in 0..128u32 {
                    let high = (h << 1) | lp;
                    let k =
                        ((low * (64 - OPTIMAL_WEIGHT) + high * OPTIMAL_WEIGHT + 32) >> 6) as i32;
                    let d = k - c as i32;
                    let err = d * d;
                    if err < best.m_error as i32 {
                        best.m_error = err as u16;
                        best.m_lo = l as u8;
                        best.m_hi = h as u8;
                    }
                }
            }
            t[c as usize][lp as usize] = best;
        }
    }
    t
}

/// Build the mode-5 (BC7 777) optimal single-color endpoint table: for each
/// target, the (lo, hi) 7-bit endpoints minimizing squared error at the mode-5
/// interpolation weight.
pub fn build_mode5_optimal() -> Box<[EndpointErr; 256]> {
    let mut t = Box::new([EndpointErr::default(); 256]);
    for c in 0..256u32 {
        let mut best = EndpointErr {
            m_error: u16::MAX,
            m_lo: 0,
            m_hi: 0,
        };
        for l in 0..128u32 {
            let low = (l << 1) | (l >> 6);
            for h in 0..128u32 {
                let high = (h << 1) | (h >> 6);
                let k = ((low * (64 - OPTIMAL_WEIGHT) + high * OPTIMAL_WEIGHT + 32) >> 6) as i32;
                let d = k - c as i32;
                let err = d * d;
                if err < best.m_error as i32 {
                    best.m_error = err as u16;
                    best.m_lo = l as u8;
                    best.m_hi = h as u8;
                }
            }
        }
        t[c as usize] = best;
    }
    t
}

/// Cached mode-6 optimal single-color endpoint table.
pub fn mode6_optimal() -> &'static [[EndpointErr; 2]; 256] {
    static T: OnceBox<[[EndpointErr; 2]; 256]> = OnceBox::new();
    T.get_or_init(build_mode6_optimal)
}

/// Cached mode-5 optimal single-color endpoint table.
pub fn mode5_optimal() -> &'static [EndpointErr; 256] {
    static T: OnceBox<[EndpointErr; 256]> = OnceBox::new();
    T.get_or_init(build_mode5_optimal)
}

use super::bc7_tables::*;
use super::bise::astc_unquant;
use super::partitions::{
    ASTC_BC7_COMMON_PARTITIONS2, ASTC_BC7_COMMON_PARTITIONS3, BC7_3_ASTC2_COMMON_PARTITIONS,
};
use super::tables::{COMPS, ENDPOINT_RANGES};
use super::unpack::{unpack_to_block, UnpackedUastcBlock};
use super::UASTC_MODE_INDEX_SOLID_COLOR;
use crate::once::OnceBox;

/// Selector written for every texel of a mode-5 solid block; its 2-bit weight
/// is the 21 the optimal-endpoint table assumes.
const BC7ENC_MODE_5_OPTIMAL_INDEX: u8 = 1;
/// Selector written for every texel of a mode-6 solid block; its 4-bit weight
/// is the 21 the optimal-endpoint table assumes.
const BC7ENC_MODE_6_OPTIMAL_INDEX: u8 = 5;

/// A high-level BC7 block description.
#[derive(Clone, Copy, Default)]
struct Bc7Results {
    /// BC7 output mode, 0..=7.
    mode: u32,
    /// Partition index for the multi-subset modes; 0 for single-subset modes.
    partition: u32,
    /// Per-texel color (or shared color+alpha) weight index, raster order.
    selectors: [u8; 16],
    /// Per-texel alpha weight index, used only by the separate-alpha modes
    /// (BC7 modes 4 and 5).
    alpha_selectors: [u8; 16],
    /// Low endpoint per subset, indexed `[subset][channel]` (RGBA).
    low: [[u8; 4]; 3],
    /// High endpoint per subset, indexed `[subset][channel]` (RGBA).
    high: [[u8; 4]; 3],
    /// Per-subset p-bits, `[subset][endpoint]`; both entries hold the shared
    /// value on the shared-pbit modes.
    pbits: [[u32; 2]; 3],
    /// Mode-4 index selector: when set, the alpha index set is primary and the
    /// color index set secondary (their bit widths swap).
    index_selector: u32,
    /// Mode 4/5 rotation: which color channel is swapped with alpha.
    rotation: u32,
}

/// Clamp `v` to the inclusive range `[lo, hi]`.
#[inline]
fn clampi(v: i32, lo: i32, hi: i32) -> i32 {
    v.max(lo).min(hi)
}
/// `x` squared.
#[inline]
fn squaref(x: f32) -> f32 {
    x * x
}

/// Map a 3-subset partition index to the 2-subset index BC7 mode 7 uses, for
/// merge variant `k`: `k >> 1` selects which subsets collapse together and
/// `k & 1` optionally swaps the two resulting subsets.
fn convert_partition_index_3_to_2(mut p: u32, k: u32) -> u32 {
    match k >> 1 {
        0 => p = if p <= 1 { 0 } else { 1 },
        1 => p = if p == 0 { 0 } else { 1 },
        2 => p = if p == 0 || p == 2 { 0 } else { 1 },
        _ => {}
    }
    if k & 1 != 0 {
        p = 1 - p;
    }
    p
}

/// Quantize the endpoint pair `xl`/`xh` to `comp_bits` plus one pbit shared by
/// both endpoints, picking the pbit that minimizes round-trip error. Returns
/// (min_color, max_color, pbits).
fn determine_shared_pbits(
    total_comps: u32,
    comp_bits: u32,
    xl: &[f32; 4],
    xh: &[f32; 4],
) -> ([u8; 4], [u8; 4], [u32; 2]) {
    let total_bits = comp_bits + 1;
    let iscalep = (1i32 << total_bits) - 1;
    let scalep = iscalep as f32;
    let mut best_err = 1e9f32;
    let (mut best_min, mut best_max, mut best_pbits) = ([0u8; 4], [0u8; 4], [0u32; 2]);

    for p in 0..2i32 {
        let (mut xmin, mut xmax) = ([0u8; 4], [0u8; 4]);
        for c in 0..4 {
            xmin[c] = clampi(
                (((xl[c] * scalep - p as f32) / 2.0f32 + 0.5f32) as i32) * 2 + p,
                p,
                iscalep - 1 + p,
            ) as u8;
            xmax[c] = clampi(
                (((xh[c] * scalep - p as f32) / 2.0f32 + 0.5f32) as i32) * 2 + p,
                p,
                iscalep - 1 + p,
            ) as u8;
        }
        let (mut slow, mut shigh) = ([0u8; 4], [0u8; 4]);
        for i in 0..4 {
            // expand back to 8 bits by folding the field's top bits into its
            // low bits. total_bits can be 8, and a u8 shifted right by 8 would
            // overflow, so the shift is done in u32 (where it yields 0).
            let vl = xmin[i] << (8 - total_bits);
            slow[i] = vl | (((vl as u32) >> total_bits) as u8);
            let vh = xmax[i] << (8 - total_bits);
            shigh[i] = vh | (((vh as u32) >> total_bits) as u8);
        }
        let mut err = 0f32;
        for i in 0..total_comps as usize {
            err += squaref(slow[i] as f32 / 255.0f32 - xl[i])
                + squaref(shigh[i] as f32 / 255.0f32 - xh[i]);
        }
        if err < best_err {
            best_err = err;
            best_pbits = [p as u32, p as u32];
            for j in 0..4 {
                best_min[j] = xmin[j] >> 1;
                best_max[j] = xmax[j] >> 1;
            }
        }
    }
    (best_min, best_max, best_pbits)
}

/// Like `determine_shared_pbits`, but each endpoint gets its own pbit chosen
/// independently. Returns (min_color, max_color, pbits).
fn determine_unique_pbits(
    total_comps: u32,
    comp_bits: u32,
    xl: &[f32; 4],
    xh: &[f32; 4],
) -> ([u8; 4], [u8; 4], [u32; 2]) {
    let total_bits = comp_bits + 1;
    let iscalep = (1i32 << total_bits) - 1;
    let scalep = iscalep as f32;
    let (mut best_err0, mut best_err1) = (1e9f32, 1e9f32);
    let (mut best_min, mut best_max, mut best_pbits) = ([0u8; 4], [0u8; 4], [0u32; 2]);

    for p in 0..2i32 {
        let (mut xmin, mut xmax) = ([0u8; 4], [0u8; 4]);
        for c in 0..4 {
            xmin[c] = clampi(
                (((xl[c] * scalep - p as f32) / 2.0f32 + 0.5f32) as i32) * 2 + p,
                p,
                iscalep - 1 + p,
            ) as u8;
            xmax[c] = clampi(
                (((xh[c] * scalep - p as f32) / 2.0f32 + 0.5f32) as i32) * 2 + p,
                p,
                iscalep - 1 + p,
            ) as u8;
        }
        let (mut slow, mut shigh) = ([0u8; 4], [0u8; 4]);
        for i in 0..4 {
            // expand back to 8 bits by folding the field's top bits into its
            // low bits. total_bits can be 8, and a u8 shifted right by 8 would
            // overflow, so the shift is done in u32 (where it yields 0).
            let vl = xmin[i] << (8 - total_bits);
            slow[i] = vl | (((vl as u32) >> total_bits) as u8);
            let vh = xmax[i] << (8 - total_bits);
            shigh[i] = vh | (((vh as u32) >> total_bits) as u8);
        }
        let (mut err0, mut err1) = (0f32, 0f32);
        for i in 0..total_comps as usize {
            err0 += squaref(slow[i] as f32 - xl[i] * 255.0f32);
            err1 += squaref(shigh[i] as f32 - xh[i] * 255.0f32);
        }
        if err0 < best_err0 {
            best_err0 = err0;
            best_pbits[0] = p as u32;
            for j in 0..4 {
                best_min[j] = xmin[j] >> 1;
            }
        }
        if err1 < best_err1 {
            best_err1 = err1;
            best_pbits[1] = p as u32;
            for j in 0..4 {
                best_max[j] = xmax[j] >> 1;
            }
        }
    }
    (best_min, best_max, best_pbits)
}

/// Write `num_bits` of `val` at `*ofs` (LSB-first), advancing `*ofs`.
pub(crate) fn set_block_bits(bytes: &mut [u8; 16], mut val: u32, mut num_bits: u32, ofs: &mut u32) {
    while num_bits != 0 {
        let n = (8 - (*ofs & 7)).min(num_bits);
        bytes[(*ofs >> 3) as usize] |= (val << (*ofs & 7)) as u8;
        val >>= n;
        num_bits -= n;
        *ofs += n;
    }
}

/// `encode_bc7_block`: pack a `Bc7Results` into 16 bytes.
fn encode_bc7_block(r: &Bc7Results) -> [u8; 16] {
    let best_mode = r.mode as usize;
    let total_subsets = G_BC7_NUM_SUBSETS[best_mode] as usize;
    let total_partitions = 1usize << G_BC7_PARTITION_BITS[best_mode];

    let partition: &[u8] = if total_subsets == 1 {
        &G_BC7_PARTITION1
    } else if total_subsets == 2 {
        &G_BC7_PARTITION2[r.partition as usize * 16..][..16]
    } else {
        &G_BC7_PARTITION3[r.partition as usize * 16..][..16]
    };

    let mut color_selectors = r.selectors;
    let mut alpha_selectors = r.alpha_selectors;
    let mut low = r.low;
    let mut high = r.high;
    let mut pbits = r.pbits;
    let mut anchor = [-1i32; 3];
    let sep_alpha = best_mode == 4 || best_mode == 5;

    for k in 0..total_subsets {
        let anchor_index = if k == 0 {
            0usize
        } else if total_subsets == 3 && k == 1 {
            G_BC7_ANCHOR_THIRD_1[r.partition as usize] as usize
        } else if total_subsets == 3 && k == 2 {
            G_BC7_ANCHOR_THIRD_2[r.partition as usize] as usize
        } else {
            G_BC7_ANCHOR_SECOND[r.partition as usize] as usize
        };
        anchor[k] = anchor_index as i32;

        let color_index_bits = G_BC7_COLOR_INDEX_BITCOUNT[best_mode] as u32 + r.index_selector;
        let num_color_indices = 1u32 << color_index_bits;

        if color_selectors[anchor_index] as u32 & (num_color_indices >> 1) != 0 {
            for i in 0..16 {
                if partition[i] as usize == k {
                    color_selectors[i] =
                        (num_color_indices as u8).wrapping_sub(1) - color_selectors[i];
                }
            }
            if sep_alpha {
                for q in 0..3 {
                    core::mem::swap(&mut low[k][q], &mut high[k][q]);
                }
            } else {
                core::mem::swap(&mut low[k], &mut high[k]);
            }
            if G_BC7_MODE_HAS_SHARED_P_BITS[best_mode] == 0 {
                pbits[k].swap(0, 1);
            }
        }

        if sep_alpha {
            let alpha_index_bits =
                G_BC7_ALPHA_INDEX_BITCOUNT[best_mode] as i32 - r.index_selector as i32;
            let num_alpha_indices = 1u32 << alpha_index_bits;
            if alpha_selectors[anchor_index] as u32 & (num_alpha_indices >> 1) != 0 {
                for i in 0..16 {
                    if partition[i] as usize == k {
                        alpha_selectors[i] =
                            (num_alpha_indices as u8).wrapping_sub(1) - alpha_selectors[i];
                    }
                }
                core::mem::swap(&mut low[k][3], &mut high[k][3]);
            }
        }
    }

    let mut block = [0u8; 16];
    let mut ofs = 0u32;
    set_block_bits(&mut block, 1 << best_mode, best_mode as u32 + 1, &mut ofs);
    if best_mode == 4 || best_mode == 5 {
        set_block_bits(&mut block, r.rotation, 2, &mut ofs);
    }
    if best_mode == 4 {
        set_block_bits(&mut block, r.index_selector, 1, &mut ofs);
    }
    if total_partitions > 1 {
        set_block_bits(
            &mut block,
            r.partition,
            if total_partitions == 64 { 6 } else { 4 },
            &mut ofs,
        );
    }

    let total_comps = if best_mode >= 4 { 4 } else { 3 };
    for comp in 0..total_comps {
        let prec = if comp == 3 {
            G_BC7_ALPHA_PRECISION[best_mode] as u32
        } else {
            G_BC7_COLOR_PRECISION[best_mode] as u32
        };
        for subset in 0..total_subsets {
            set_block_bits(&mut block, low[subset][comp] as u32, prec, &mut ofs);
            set_block_bits(&mut block, high[subset][comp] as u32, prec, &mut ofs);
        }
    }

    if G_BC7_MODE_HAS_P_BITS[best_mode] != 0 {
        for pb in pbits.iter().take(total_subsets) {
            set_block_bits(&mut block, pb[0], 1, &mut ofs);
            if G_BC7_MODE_HAS_SHARED_P_BITS[best_mode] == 0 {
                set_block_bits(&mut block, pb[1], 1, &mut ofs);
            }
        }
    }

    for idx in 0..16usize {
        let mut n = if r.index_selector != 0 {
            G_BC7_ALPHA_INDEX_BITCOUNT[best_mode] as i32 - r.index_selector as i32
        } else {
            G_BC7_COLOR_INDEX_BITCOUNT[best_mode] as i32 + r.index_selector as i32
        };
        if idx as i32 == anchor[0] || idx as i32 == anchor[1] || idx as i32 == anchor[2] {
            n -= 1;
        }
        let val = if r.index_selector != 0 {
            alpha_selectors[idx]
        } else {
            color_selectors[idx]
        };
        set_block_bits(&mut block, val as u32, n as u32, &mut ofs);
    }

    if sep_alpha {
        for idx in 0..16usize {
            let mut n = if r.index_selector != 0 {
                G_BC7_COLOR_INDEX_BITCOUNT[best_mode] as i32 + r.index_selector as i32
            } else {
                G_BC7_ALPHA_INDEX_BITCOUNT[best_mode] as i32 - r.index_selector as i32
            };
            if idx as i32 == anchor[0] || idx as i32 == anchor[1] || idx as i32 == anchor[2] {
                n -= 1;
            }
            let val = if r.index_selector != 0 {
                color_selectors[idx]
            } else {
                alpha_selectors[idx]
            };
            set_block_bits(&mut block, val as u32, n as u32, &mut ofs);
        }
    }

    block
}

/// Build the BC7 block description (mode, endpoints, selectors, partition) from
/// an unpacked UASTC block. Returns `None` for an unknown mode.
fn build_bc7_results(u: &UnpackedUastcBlock) -> Option<Bc7Results> {
    let mut d = Bc7Results::default();
    let mode = u.mode;
    let endpoint_range = ENDPOINT_RANGES[mode as usize] as usize;
    let total_comps = COMPS[mode as usize] as u32;
    let unq = astc_unquant();
    let e = &u.astc.endpoints;
    let w = &u.astc.weights;
    let uq = |i: usize| unq[endpoint_range][e[i] as usize].m_unquant as u32;

    // Each UASTC mode maps to one BC7 mode. The arm unquantizes this block's
    // endpoints, rescales them to the BC7 component precision (running the pbit
    // search where the mode has pbits), picks the partition, and remaps the
    // weights to BC7 selectors.
    match mode {
        0 | 5 | 10 | 12 | 14 | 15 | 18 => {
            d.mode = 6;
            let (mut xl, mut xh) = ([0f32; 4], [0f32; 4]);
            if total_comps == 2 {
                xl[0] = uq(0) as f32 / 255.0;
                xh[0] = uq(1) as f32 / 255.0;
                xl[1] = xl[0];
                xh[1] = xh[0];
                xl[2] = xl[0];
                xh[2] = xh[0];
                xl[3] = uq(2) as f32 / 255.0;
                xh[3] = uq(3) as f32 / 255.0;
            } else {
                xl[0] = uq(0) as f32 / 255.0;
                xl[1] = uq(2) as f32 / 255.0;
                xl[2] = uq(4) as f32 / 255.0;
                xh[0] = uq(1) as f32 / 255.0;
                xh[1] = uq(3) as f32 / 255.0;
                xh[2] = uq(5) as f32 / 255.0;
                if total_comps == 4 {
                    xl[3] = uq(6) as f32 / 255.0;
                    xh[3] = uq(7) as f32 / 255.0;
                } else {
                    xl[3] = 1.0;
                    xh[3] = 1.0;
                }
            }
            let (bmin, bmax, pb) =
                determine_unique_pbits(if total_comps == 2 { 4 } else { total_comps }, 7, &xl, &xh);
            d.low[0] = bmin;
            d.high[0] = bmax;
            if total_comps == 3 {
                d.low[0][3] = 127;
                d.high[0][3] = 127;
            }
            d.pbits[0] = pb;
            if mode == 18 {
                const M: [u8; 32] = [
                    0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 6, 7, 8, 9, 9, 9, 10, 10, 11, 11, 12,
                    12, 13, 13, 14, 14, 15, 15,
                ];
                for i in 0..16 {
                    d.selectors[i] = M[w[i] as usize];
                }
            } else if mode == 14 {
                const M: [u8; 4] = [0, 5, 10, 15];
                for i in 0..16 {
                    d.selectors[i] = M[w[i] as usize];
                }
            } else if mode == 5 || mode == 12 {
                const M: [u8; 8] = [0, 2, 4, 6, 9, 11, 13, 15];
                for i in 0..16 {
                    d.selectors[i] = M[w[i] as usize];
                }
            } else {
                d.selectors[..16].copy_from_slice(&w[..16]);
            }
        }
        1 => {
            d.mode = 3;
            let xl = [
                e[0] as f32 / 255.0,
                e[2] as f32 / 255.0,
                e[4] as f32 / 255.0,
                1.0,
            ];
            let xh = [
                e[1] as f32 / 255.0,
                e[3] as f32 / 255.0,
                e[5] as f32 / 255.0,
                1.0,
            ];
            let (bmin, bmax, pb) = determine_unique_pbits(3, 7, &xl, &xh);
            d.low[0][..3].copy_from_slice(&bmin[..3]);
            d.high[0][..3].copy_from_slice(&bmax[..3]);
            d.low[1][..3].copy_from_slice(&bmin[..3]);
            d.high[1][..3].copy_from_slice(&bmax[..3]);
            d.pbits[0] = pb;
            d.pbits[1] = pb;
            d.selectors[..16].copy_from_slice(&w[..16]);
        }
        2 => {
            d.mode = 1;
            let desc = ASTC_BC7_COMMON_PARTITIONS2[u.common_pattern as usize];
            d.partition = desc.bc7 as u32;
            let invert = desc.invert;
            let (mut xl, mut xh) = ([0f32, 0.0, 0.0, 1.0], [0f32, 0.0, 0.0, 1.0]);
            for subset in 0..2usize {
                for i in 0..3usize {
                    let mut v = e[i * 2 + subset * 6] as u32;
                    v = (v << 4) | v;
                    xl[i] = v as f32 / 255.0;
                    v = e[i * 2 + subset * 6 + 1] as u32;
                    v = (v << 4) | v;
                    xh[i] = v as f32 / 255.0;
                }
                let (bmin, bmax, pb) = determine_shared_pbits(3, 6, &xl, &xh);
                let bi = if invert { 1 - subset } else { subset };
                d.low[bi][..3].copy_from_slice(&bmin[..3]);
                d.high[bi][..3].copy_from_slice(&bmax[..3]);
                d.pbits[bi][0] = pb[0];
            }
            d.selectors[..16].copy_from_slice(&w[..16]);
        }
        3 => {
            d.mode = 2;
            let desc = ASTC_BC7_COMMON_PARTITIONS3[u.common_pattern as usize];
            d.partition = desc.bc7 as u32;
            let perm = desc.astc_to_bc7_perm as usize;
            for subset in 0..3usize {
                for comp in 0..3usize {
                    let mut lo = uq(comp * 2 + subset * 6);
                    let mut hi = uq(comp * 2 + 1 + subset * 6);
                    lo = (lo * 31 + 127) / 255;
                    hi = (hi * 31 + 127) / 255;
                    let bi = super::tables::ASTC_TO_BC7_PERM[perm][subset] as usize;
                    d.low[bi][comp] = lo as u8;
                    d.high[bi][comp] = hi as u8;
                }
            }
            d.selectors[..16].copy_from_slice(&w[..16]);
        }
        4 => {
            d.mode = 3;
            let desc = ASTC_BC7_COMMON_PARTITIONS2[u.common_pattern as usize];
            d.partition = desc.bc7 as u32;
            let invert = desc.invert;
            let (mut xl, mut xh) = ([0f32, 0.0, 0.0, 1.0], [0f32, 0.0, 0.0, 1.0]);
            for subset in 0..2usize {
                for i in 0..3usize {
                    xl[i] = uq(i * 2 + subset * 6) as f32 / 255.0;
                    xh[i] = uq(i * 2 + subset * 6 + 1) as f32 / 255.0;
                }
                let (bmin, bmax, pb) = determine_unique_pbits(3, 7, &xl, &xh);
                let bi = if invert { 1 - subset } else { subset };
                d.low[bi][..3].copy_from_slice(&bmin[..3]);
                d.high[bi][..3].copy_from_slice(&bmax[..3]);
                d.low[bi][3] = 127;
                d.high[bi][3] = 127;
                d.pbits[bi] = pb;
            }
            d.selectors[..16].copy_from_slice(&w[..16]);
        }
        6 | 11 | 13 | 17 => {
            d.mode = 5;
            let ccs = u.astc.ccs;
            d.rotation = ((ccs + 1) & 3) as u32;
            if total_comps == 2 {
                d.low[0][0] = ((uq(0) * 127 + 127) / 255) as u8;
                d.high[0][0] = ((uq(1) * 127 + 127) / 255) as u8;
                d.low[0][1] = d.low[0][0];
                d.high[0][1] = d.high[0][0];
                d.low[0][2] = d.low[0][0];
                d.high[0][2] = d.high[0][0];
                d.low[0][3] = uq(2) as u8;
                d.high[0][3] = uq(3) as u8;
            } else {
                for astc_comp in 0..4usize {
                    let mut bc7_comp = astc_comp;
                    if astc_comp == ccs as usize {
                        bc7_comp = 3;
                    } else if astc_comp == 3 {
                        bc7_comp = ccs as usize;
                    }
                    let (mut l, mut h) = (255u32, 255u32);
                    if (astc_comp as u32) < total_comps {
                        l = uq(astc_comp * 2);
                        h = uq(astc_comp * 2 + 1);
                    }
                    if bc7_comp < 3 {
                        l = (l * 127 + 127) / 255;
                        h = (h * 127 + 127) / 255;
                    }
                    d.low[0][bc7_comp] = l as u8;
                    d.high[0][bc7_comp] = h as u8;
                }
            }
            if mode == 13 {
                for i in 0..16 {
                    d.selectors[i] = if w[i * 2] != 0 { 3 } else { 0 };
                    d.alpha_selectors[i] = if w[i * 2 + 1] != 0 { 3 } else { 0 };
                }
            } else {
                for i in 0..16 {
                    d.selectors[i] = w[i * 2];
                    d.alpha_selectors[i] = w[i * 2 + 1];
                }
            }
        }
        7 => {
            d.mode = 2;
            let desc = BC7_3_ASTC2_COMMON_PARTITIONS[u.common_pattern as usize];
            d.partition = desc.bc73 as u32;
            let k = desc.k as u32;
            for bc7_part in 0..3u32 {
                let astc_part = convert_partition_index_3_to_2(bc7_part, k) as usize;
                for c in 0..3usize {
                    d.low[bc7_part as usize][c] =
                        ((uq(c * 2 + astc_part * 6) * 31 + 127) / 255) as u8;
                    d.high[bc7_part as usize][c] =
                        ((uq(c * 2 + 1 + astc_part * 6) * 31 + 127) / 255) as u8;
                }
            }
            d.selectors[..16].copy_from_slice(&w[..16]);
        }
        UASTC_MODE_INDEX_SOLID_COLOR => {
            let sc = u.solid_color;
            let m6 = mode6_optimal();
            let m5 = mode5_optimal();
            let be0 = m6[sc.r() as usize][0].m_error as u32
                + m6[sc.g() as usize][0].m_error as u32
                + m6[sc.b() as usize][0].m_error as u32
                + m6[sc.a() as usize][0].m_error as u32;
            let be1 = m6[sc.r() as usize][1].m_error as u32
                + m6[sc.g() as usize][1].m_error as u32
                + m6[sc.b() as usize][1].m_error as u32
                + m6[sc.a() as usize][1].m_error as u32;
            if be0 > 0 && be1 > 0 {
                d.mode = 5;
                for c in 0..3usize {
                    d.low[0][c] = m5[sc.c[c] as usize].m_lo;
                    d.high[0][c] = m5[sc.c[c] as usize].m_hi;
                }
                d.selectors = [BC7ENC_MODE_5_OPTIMAL_INDEX; 16];
                d.low[0][3] = sc.c[3];
                d.high[0][3] = sc.c[3];
            } else {
                d.mode = 6;
                let best_p = if be1 < be0 { 1usize } else { 0 };
                for c in 0..4usize {
                    d.low[0][c] = m6[sc.c[c] as usize][best_p].m_lo;
                    d.high[0][c] = m6[sc.c[c] as usize][best_p].m_hi;
                }
                d.pbits[0] = [best_p as u32, best_p as u32];
                d.selectors = [BC7ENC_MODE_6_OPTIMAL_INDEX; 16];
            }
        }
        9 | 16 => {
            d.mode = 7;
            let desc = ASTC_BC7_COMMON_PARTITIONS2[u.common_pattern as usize];
            d.partition = desc.bc7 as u32;
            let invert = desc.invert;
            for astc_subset in 0..2usize {
                let (mut xl, mut xh) = ([0f32; 4], [0f32; 4]);
                if total_comps == 2 {
                    xl[0] = uq(astc_subset * 4) as f32 / 255.0;
                    xh[0] = uq(1 + astc_subset * 4) as f32 / 255.0;
                    xl[1] = xl[0];
                    xh[1] = xh[0];
                    xl[2] = xl[0];
                    xh[2] = xh[0];
                    xl[3] = uq(2 + astc_subset * 4) as f32 / 255.0;
                    xh[3] = uq(3 + astc_subset * 4) as f32 / 255.0;
                } else {
                    xl[0] = uq(astc_subset * 8) as f32 / 255.0;
                    xl[1] = uq(2 + astc_subset * 8) as f32 / 255.0;
                    xl[2] = uq(4 + astc_subset * 8) as f32 / 255.0;
                    xl[3] = uq(6 + astc_subset * 8) as f32 / 255.0;
                    xh[0] = uq(1 + astc_subset * 8) as f32 / 255.0;
                    xh[1] = uq(3 + astc_subset * 8) as f32 / 255.0;
                    xh[2] = uq(5 + astc_subset * 8) as f32 / 255.0;
                    xh[3] = uq(7 + astc_subset * 8) as f32 / 255.0;
                }
                let (bmin, bmax, pb) = determine_unique_pbits(4, 5, &xl, &xh);
                let bi = if invert { 1 - astc_subset } else { astc_subset };
                d.low[bi] = bmin;
                d.high[bi] = bmax;
                d.pbits[bi] = pb;
            }
            d.selectors[..16].copy_from_slice(&w[..16]);
        }
        _ => return None,
    }
    Some(d)
}

/// Decode a UASTC block and repack it as BC7. The block is unpacked with the
/// endpoint-normalization pass off: the per-mode conversion in
/// `build_bc7_results` and the anchor-based selector flips in `encode_bc7_block`
/// order the endpoints themselves, so normalizing at unpack time is not wanted.
pub fn transcode_uastc_to_bc7(src: &[u8; 16]) -> Option<[u8; 16]> {
    let u = unpack_to_block(src, false, false)?;
    let results = build_bc7_results(&u)?;
    Some(encode_bc7_block(&results))
}
