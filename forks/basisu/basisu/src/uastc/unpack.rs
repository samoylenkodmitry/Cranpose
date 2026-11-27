//! UASTC block unpack: decode a 128-bit UASTC block into structured form
//! (mode, transcoder hints, ASTC-style endpoints and weights), and from there
//! into 16 RGBA pixels.

use super::bise::astc_unquant;
use super::huff_modes::HUFF_MODES;
use super::partitions::{
    ASTC_BC7_COMMON_PARTITIONS2, ASTC_BC7_COMMON_PARTITIONS3, BC7_3_ASTC2_COMMON_PARTITIONS,
};
use super::patterns::{
    ASTC_BC7_PATTERN2_ANCHORS, ASTC_BC7_PATTERN3_ANCHORS, ASTC_BC7_PATTERNS2, ASTC_BC7_PATTERNS3,
    BC7_3_ASTC2_PATTERNS2, BC7_3_ASTC2_PATTERNS2_ANCHORS,
};
use super::tables::{
    ASTC_BISE_RANGE_TABLE, CEM, COMPS, ENDPOINT_RANGES, MODE_HUFF_CODES, PLANES, SUBSETS,
    TOTAL_HINT_BITS, WEIGHT_BITS, WEIGHT_RANGES,
};
use super::{TOTAL_UASTC_MODES, UASTC_MODE_INDEX_SOLID_COLOR};
use crate::bitreader::{read_bit, read_bits1_to_9, read_bits1_to_9_fst, read_bits64};
use crate::color::Color32;

// 0..=64 interpolation ramps, one per weight bit width. The 1..3 bit ramps are
// shared by BC7 and ASTC; the 4 and 5 bit ramps exist only in ASTC.
static BC7_WEIGHTS1: [u32; 2] = [0, 64];
static BC7_WEIGHTS2: [u32; 4] = [0, 21, 43, 64];
static BC7_WEIGHTS3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
static ASTC_WEIGHTS4: [u32; 16] = [0, 4, 8, 12, 17, 21, 25, 29, 35, 39, 43, 47, 52, 56, 60, 64];
static ASTC_WEIGHTS5: [u32; 32] = [
    0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 34, 36, 38, 40, 42, 44, 46, 48, 50,
    52, 54, 56, 58, 60, 62, 64,
];
/// All-zero anchor and partition pattern, used by single-subset modes.
static ZERO_PATTERN: [u8; 16] = [0; 16];

/// The 0..=64 interpolation weight set for a given weight-bit count (1..=5).
/// Empty for any other width, which the callers never request.
fn weight_table(weight_bits: u32) -> &'static [u32] {
    match weight_bits {
        1 => &BC7_WEIGHTS1,
        2 => &BC7_WEIGHTS2,
        3 => &BC7_WEIGHTS3,
        4 => &ASTC_WEIGHTS4,
        5 => &ASTC_WEIGHTS5,
        _ => &[],
    }
}

/// Interpolate two 8-bit endpoints by a 0..=64 weight, expanding each to 16
/// bits first (sRGB expands as `v << 8 | 0x80` per the ASTC specification,
/// linear as `v << 8 | v`) and rounding the lerp back down to 8 bits.
#[inline]
fn astc_interpolate(mut l: u32, mut h: u32, w: u32, srgb: bool) -> u32 {
    if srgb {
        l = (l << 8) | 0x80;
        h = (h << 8) | 0x80;
    } else {
        l = (l << 8) | l;
        h = (h << 8) | h;
    }
    let k = (l * (64 - w) + h * w + 32) >> 6;
    k >> 8
}

/// A decoded UASTC block in ASTC terms: the partition/CEM/plane setup plus the
/// unpacked (still quantized) endpoint and weight values.
#[derive(Clone, Copy)]
pub struct AstcBlockDesc {
    /// BISE range index of the weights.
    pub weight_range: i32,
    /// partition (subset) count, 1..=3.
    pub subsets: i32,
    /// ASTC partition pattern seed for multi-subset blocks.
    pub partition_seed: i32,
    /// color endpoint mode, shared by all subsets.
    pub cem: i32,
    /// dual-plane color component selector: which channel the second plane
    /// carries.
    pub ccs: i32,
    /// set when the mode uses two weight planes, in which case `ccs` names the
    /// channel driven by the second plane.
    pub dual_plane: bool,
    /// quantized endpoint values, subset-major, two per component.
    pub endpoints: [u8; 18],
    /// per-texel weights; two entries per texel when `dual_plane` is set.
    pub weights: [u8; 64],
}

// manual impl: [u8; 64] is too large for the derived Default.
impl Default for AstcBlockDesc {
    fn default() -> Self {
        Self {
            weight_range: 0,
            subsets: 0,
            partition_seed: 0,
            cem: 0,
            ccs: 0,
            dual_plane: false,
            endpoints: [0; 18],
            weights: [0; 64],
        }
    }
}

/// A fully parsed UASTC block: the ASTC-shaped payload plus the mode, the
/// solid color for solid-color blocks, and the encoder-written transcoder
/// hints (only meaningful when decoded with `read_hints`).
#[derive(Clone, Copy, Default)]
pub struct UnpackedUastcBlock {
    /// the ASTC-form payload: partition/CEM/plane setup, endpoints, and weights.
    pub astc: AstcBlockDesc,
    /// UASTC mode index; `UASTC_MODE_INDEX_SOLID_COLOR` marks a solid block.
    pub mode: u32,
    /// index into the shared BC7/ASTC partition pattern tables for
    /// multi-subset modes.
    pub common_pattern: u32,
    /// the block's RGBA color when `mode` is the solid-color mode.
    pub solid_color: Color32,
    /// hint flags selecting the fast BC1 transcode paths.
    pub bc1_hint0: bool,
    pub bc1_hint1: bool,
    /// ETC1 transcode hints: precomputed flip/diff bits, intensity tables, a
    /// selector bias, and (for solid blocks) a base color and selector.
    pub etc1_flip: bool,
    pub etc1_diff: bool,
    pub etc1_inten0: u32,
    pub etc1_inten1: u32,
    pub etc1_bias: u32,
    /// ETC2 EAC alpha transcode hints (modes with alpha only).
    pub etc2_hints: u32,
    pub etc1_selector: u32,
    pub etc1_r: u32,
    pub etc1_g: u32,
    pub etc1_b: u32,
}

/// Anchor texel indices and the 16-texel partition pattern for a multi-subset
/// mode. Single-subset modes get all-zero patterns (their only anchor is
/// texel 0 and no partition lookup happens).
fn get_anchor_indices(
    subsets: u32,
    mode: u32,
    common_pattern: u32,
) -> (&'static [u8], &'static [u8]) {
    if subsets >= 2 {
        if subsets == 3 {
            (
                &ASTC_BC7_PATTERN3_ANCHORS[common_pattern as usize],
                &ASTC_BC7_PATTERNS3[common_pattern as usize],
            )
        } else if mode == 7 {
            (
                &BC7_3_ASTC2_PATTERNS2_ANCHORS[common_pattern as usize],
                &BC7_3_ASTC2_PATTERNS2[common_pattern as usize],
            )
        } else {
            (
                &ASTC_BC7_PATTERN2_ANCHORS[common_pattern as usize],
                &ASTC_BC7_PATTERNS2[common_pattern as usize],
            )
        }
    } else {
        (&ZERO_PATTERN, &ZERO_PATTERN)
    }
}

/// Decode a 128-bit UASTC block into structured form. Returns `None` on an
/// invalid mode or out-of-range partition pattern. `read_hints` decodes the
/// transcoder hint fields instead of skipping over them. `blue_contract_check`
/// reorders endpoint pairs (inverting the affected weights to compensate) so
/// that an ASTC decoder will not apply blue contraction, which UASTC never
/// uses; pixel decode does not need it because the swap is lossless.
pub fn unpack_to_block(
    bytes: &[u8; 16],
    blue_contract_check: bool,
    read_hints: bool,
) -> Option<UnpackedUastcBlock> {
    let mut u = UnpackedUastcBlock::default();

    let mode = HUFF_MODES[(bytes[0] & 127) as usize] as u32;
    if mode >= TOTAL_UASTC_MODES as u32 {
        return None;
    }
    u.mode = mode;
    let mut bit_ofs = MODE_HUFF_CODES[mode as usize][1];

    if mode == UASTC_MODE_INDEX_SOLID_COLOR {
        let r = read_bits1_to_9_fst(bytes, &mut bit_ofs, 8);
        let g = read_bits1_to_9_fst(bytes, &mut bit_ofs, 8);
        let b = read_bits1_to_9_fst(bytes, &mut bit_ofs, 8);
        let a = read_bits1_to_9_fst(bytes, &mut bit_ofs, 8);
        u.solid_color = Color32::new(r, g, b, a);
        if read_hints {
            u.etc1_flip = false;
            u.etc1_diff = read_bit(bytes, &mut bit_ofs) != 0;
            u.etc1_inten0 = read_bits1_to_9_fst(bytes, &mut bit_ofs, 3);
            u.etc1_inten1 = 0;
            u.etc1_selector = read_bits1_to_9_fst(bytes, &mut bit_ofs, 2);
            u.etc1_r = read_bits1_to_9_fst(bytes, &mut bit_ofs, 5);
            u.etc1_g = read_bits1_to_9_fst(bytes, &mut bit_ofs, 5);
            u.etc1_b = read_bits1_to_9_fst(bytes, &mut bit_ofs, 5);
            u.etc1_bias = 0;
            u.etc2_hints = 0;
        }
        return Some(u);
    }

    if read_hints {
        // a hint bit is present in the stream only for modes that carry it, so
        // the table check must short-circuit before any bit is consumed.
        u.bc1_hint0 =
            super::tables::HAS_BC1_HINT0[mode as usize] != 0 && read_bit(bytes, &mut bit_ofs) != 0;
        u.bc1_hint1 =
            super::tables::HAS_BC1_HINT1[mode as usize] != 0 && read_bit(bytes, &mut bit_ofs) != 0;
        u.etc1_flip = read_bit(bytes, &mut bit_ofs) != 0;
        u.etc1_diff = read_bit(bytes, &mut bit_ofs) != 0;
        u.etc1_inten0 = read_bits1_to_9_fst(bytes, &mut bit_ofs, 3);
        u.etc1_inten1 = read_bits1_to_9_fst(bytes, &mut bit_ofs, 3);
        u.etc1_bias = if super::tables::HAS_ETC1_BIAS[mode as usize] != 0 {
            read_bits1_to_9_fst(bytes, &mut bit_ofs, 5)
        } else {
            0
        };
        u.etc2_hints = if super::tables::HAS_ALPHA[mode as usize] != 0 {
            read_bits1_to_9_fst(bytes, &mut bit_ofs, 8)
        } else {
            0
        };
    } else {
        bit_ofs += TOTAL_HINT_BITS[mode as usize] as u32;
    }

    let mut subsets = 1u32;
    match mode {
        2 | 4 | 7 | 9 | 16 => {
            u.common_pattern = read_bits1_to_9_fst(bytes, &mut bit_ofs, 5);
            subsets = 2;
        }
        3 => {
            u.common_pattern = read_bits1_to_9_fst(bytes, &mut bit_ofs, 4);
            subsets = 3;
        }
        _ => {}
    }

    let mut part_seed = 0u32;
    match mode {
        2 | 4 | 9 | 16 => {
            if u.common_pattern as usize >= ASTC_BC7_COMMON_PARTITIONS2.len() {
                return None;
            }
            part_seed = ASTC_BC7_COMMON_PARTITIONS2[u.common_pattern as usize].astc as u32;
        }
        3 => {
            if u.common_pattern as usize >= ASTC_BC7_COMMON_PARTITIONS3.len() {
                return None;
            }
            part_seed = ASTC_BC7_COMMON_PARTITIONS3[u.common_pattern as usize].astc as u32;
        }
        7 => {
            if u.common_pattern as usize >= BC7_3_ASTC2_COMMON_PARTITIONS.len() {
                return None;
            }
            part_seed = BC7_3_ASTC2_COMMON_PARTITIONS[u.common_pattern as usize].astc2 as u32;
        }
        _ => {}
    }

    let mut total_planes = 1u32;
    match mode {
        6 | 11 | 13 => {
            u.astc.ccs = read_bits1_to_9_fst(bytes, &mut bit_ofs, 2) as i32;
            total_planes = 2;
        }
        17 => {
            u.astc.ccs = 3;
            total_planes = 2;
        }
        _ => {}
    }
    u.astc.dual_plane = total_planes == 2;
    u.astc.subsets = subsets as i32;
    u.astc.partition_seed = part_seed as i32;

    let total_comps = COMPS[mode as usize] as u32;
    let weight_bits = WEIGHT_BITS[mode as usize] as u32;
    u.astc.weight_range = WEIGHT_RANGES[mode as usize] as i32;
    let total_values = total_comps * 2 * subsets;
    let endpoint_range = ENDPOINT_RANGES[mode as usize] as usize;
    u.astc.cem = CEM[mode as usize] as i32;

    let ep_bits = ASTC_BISE_RANGE_TABLE[endpoint_range][0] as u32;
    let ep_trits = ASTC_BISE_RANGE_TABLE[endpoint_range][1] as u32;
    let ep_quints = ASTC_BISE_RANGE_TABLE[endpoint_range][2] as u32;

    // BISE endpoint decode. The trit (base 3) parts of 5 values pack into one
    // 8-bit bundle, the quint (base 5) parts of 3 values into one 7-bit bundle;
    // each value's plain low bits are read separately below. A final partial
    // bundle is truncated to just the bits its remaining values need.
    let (mut total_tqs, mut bundle_size, mut mul) = (0u32, 0u32, 0u32);
    if ep_trits != 0 {
        total_tqs = total_values.div_ceil(5);
        bundle_size = 5;
        mul = 3;
    } else if ep_quints != 0 {
        total_tqs = total_values.div_ceil(3);
        bundle_size = 3;
        mul = 5;
    }

    let mut tq_values = [0u32; 8];
    for i in 0..total_tqs {
        let mut num_bits = if ep_trits != 0 { 8 } else { 7 };
        if i == total_tqs - 1 {
            let num_remaining = total_values - (total_tqs - 1) * bundle_size;
            if ep_trits != 0 {
                num_bits = match num_remaining {
                    1 => 2,
                    2 => 4,
                    3 => 5,
                    4 => 7,
                    _ => num_bits,
                };
            } else if ep_quints != 0 {
                num_bits = match num_remaining {
                    1 => 3,
                    2 => 5,
                    _ => num_bits,
                };
            }
        }
        tq_values[i as usize] = read_bits1_to_9_fst(bytes, &mut bit_ofs, num_bits);
    }

    let (mut accum, mut accum_remaining, mut next_tq_index) = (0u32, 0u32, 0usize);
    for i in 0..total_values {
        let mut value = read_bits1_to_9_fst(bytes, &mut bit_ofs, ep_bits);
        if total_tqs != 0 {
            if accum_remaining == 0 {
                accum = tq_values[next_tq_index];
                next_tq_index += 1;
                accum_remaining = bundle_size;
            }
            let v = accum % mul;
            accum /= mul;
            accum_remaining -= 1;
            value |= v << ep_bits;
        }
        u.astc.endpoints[i as usize] = value as u8;
    }

    let (anchor_indices, partition_pattern) = get_anchor_indices(subsets, mode, u.common_pattern);

    // Weight decode. Anchor texels store one bit less: their weight's MSB is
    // zero by construction, as in BC7.
    if mode == 18 {
        // mode 18 has 5-bit weights (16 * 5 - 1 = 79 bits), too wide for the
        // single 64-bit read below, so pull them one texel at a time.
        for i in 0..16usize {
            let nb = if i != 0 { weight_bits } else { weight_bits - 1 };
            u.astc.weights[i] = read_bits1_to_9(bytes, &mut bit_ofs, nb) as u8;
        }
    } else {
        // every other mode's weights fit in 64 bits: grab them in one read and
        // peel the fields out of the word locally.
        let n = 64.min(128 - bit_ofs);
        let mut tmp = bit_ofs;
        let bits = read_bits64(bytes, &mut tmp, n);
        let mut wofs = 0u32;
        let mask = (1u32 << weight_bits) - 1;
        let anchor_mask = (1u32 << (weight_bits - 1)) - 1;

        if total_planes == 2 {
            u.astc.weights[0] = ((bits >> wofs) as u32 & anchor_mask) as u8;
            wofs += weight_bits - 1;
            u.astc.weights[1] = ((bits >> wofs) as u32 & anchor_mask) as u8;
            wofs += weight_bits - 1;
            for i in 2..32usize {
                u.astc.weights[i] = ((bits >> wofs) as u32 & mask) as u8;
                wofs += weight_bits;
            }
        } else if subsets == 1 {
            u.astc.weights[0] = ((bits >> wofs) as u32 & anchor_mask) as u8;
            wofs += weight_bits - 1;
            for i in 1..16usize {
                u.astc.weights[i] = ((bits >> wofs) as u32 & mask) as u8;
                wofs += weight_bits;
            }
        } else {
            let (a0, a1, a2) = (anchor_indices[0], anchor_indices[1], anchor_indices[2]);
            for i in 0..16u32 {
                if i == a0 as u32 || i == a1 as u32 || i == a2 as u32 {
                    u.astc.weights[i as usize] = ((bits >> wofs) as u32 & anchor_mask) as u8;
                    wofs += weight_bits - 1;
                } else {
                    u.astc.weights[i as usize] = ((bits >> wofs) as u32 & mask) as u8;
                    wofs += weight_bits;
                }
            }
        }
    }

    if blue_contract_check && total_comps >= 3 {
        // an ASTC decoder blue-contracts a subset whose first endpoint has the
        // larger unquantized RGB sum; swap each component pair to restore
        // s0 <= s1 and invert that subset's weights so decoded colors are
        // unchanged (the weight ramps are symmetric).
        let unquant = astc_unquant();
        let mut invert_subset = [false; 3];
        let mut any_flag = false;
        let tc = total_comps as usize;
        for si in 0..subsets as usize {
            let e = &u.astc.endpoints;
            let s0 = unquant[endpoint_range][e[si * tc * 2] as usize].m_unquant as i32
                + unquant[endpoint_range][e[si * tc * 2 + 2] as usize].m_unquant as i32
                + unquant[endpoint_range][e[si * tc * 2 + 4] as usize].m_unquant as i32;
            let s1 = unquant[endpoint_range][e[si * tc * 2 + 1] as usize].m_unquant as i32
                + unquant[endpoint_range][e[si * tc * 2 + 3] as usize].m_unquant as i32
                + unquant[endpoint_range][e[si * tc * 2 + 5] as usize].m_unquant as i32;
            if s1 < s0 {
                for c in 0..tc {
                    u.astc
                        .endpoints
                        .swap(si * tc * 2 + c * 2, si * tc * 2 + c * 2 + 1);
                }
                invert_subset[si] = true;
                any_flag = true;
            }
        }
        if any_flag {
            let weight_mask = (1u32 << weight_bits) - 1;
            let tp = total_planes as usize;
            for (i, &subset) in partition_pattern.iter().enumerate().take(16) {
                if invert_subset[subset as usize] {
                    u.astc.weights[i * tp] = (weight_mask - u.astc.weights[i * tp] as u32) as u8;
                    if total_planes == 2 {
                        u.astc.weights[i * tp + 1] =
                            (weight_mask - u.astc.weights[i * tp + 1] as u32) as u8;
                    }
                }
            }
        }
    }

    Some(u)
}

/// Decode a structured block into its 16 RGBA pixels: unquantize the
/// endpoints, precompute each subset's interpolated color ramp, then look each
/// texel up by its partition subset and weight (per plane when dual-plane).
pub fn unpack_pixels(
    mode: u32,
    common_pattern: u32,
    solid_color: Color32,
    astc: &AstcBlockDesc,
    srgb: bool,
) -> [Color32; 16] {
    if mode == UASTC_MODE_INDEX_SOLID_COLOR {
        return [solid_color; 16];
    }

    let total_subsets = SUBSETS[mode as usize] as usize;
    let total_comps = (COMPS[mode as usize] as u32).min(4) as usize;
    let endpoint_range = ENDPOINT_RANGES[mode as usize] as usize;
    let total_planes = PLANES[mode as usize] as u32;
    let weight_bits = WEIGHT_BITS[mode as usize] as u32;
    let weight_levels = 1usize << weight_bits;
    let unquant = astc_unquant();

    let mut endpoints = [[Color32::default(); 2]; 3];
    for (si, ep) in endpoints.iter_mut().enumerate().take(total_subsets) {
        let base = si * total_comps * 2;
        if total_comps == 2 {
            let ll = unquant[endpoint_range][astc.endpoints[base] as usize].m_unquant as u32;
            let lh = unquant[endpoint_range][astc.endpoints[base + 1] as usize].m_unquant as u32;
            let al = unquant[endpoint_range][astc.endpoints[base + 2] as usize].m_unquant as u32;
            let ah = unquant[endpoint_range][astc.endpoints[base + 3] as usize].m_unquant as u32;
            ep[0].set(ll, ll, ll, al);
            ep[1].set(lh, lh, lh, ah);
        } else {
            for ci in 0..total_comps {
                ep[0][ci] =
                    unquant[endpoint_range][astc.endpoints[base + ci * 2] as usize].m_unquant;
                ep[1][ci] =
                    unquant[endpoint_range][astc.endpoints[base + ci * 2 + 1] as usize].m_unquant;
            }
            ep[0].c[total_comps..4].fill(255);
            ep[1].c[total_comps..4].fill(255);
        }
    }

    let pweights = weight_table(weight_bits);
    let mut block_colors = [[Color32::default(); 32]; 3];
    for si in 0..total_subsets {
        for l in 0..weight_levels {
            if total_comps == 2 {
                let lc = astc_interpolate(
                    endpoints[si][0][0] as u32,
                    endpoints[si][1][0] as u32,
                    pweights[l],
                    srgb,
                );
                let ac = astc_interpolate(
                    endpoints[si][0][3] as u32,
                    endpoints[si][1][3] as u32,
                    pweights[l],
                    srgb,
                );
                block_colors[si][l].set(lc, lc, lc, ac);
            } else {
                for ci in 0..total_comps {
                    block_colors[si][l][ci] = astc_interpolate(
                        endpoints[si][0][ci] as u32,
                        endpoints[si][1][ci] as u32,
                        pweights[l],
                        srgb,
                    ) as u8;
                }
                block_colors[si][l].c[total_comps..4].fill(255);
            }
        }
    }

    let partition_pattern: &[u8] = if total_subsets >= 2 {
        if total_subsets == 3 {
            &ASTC_BC7_PATTERNS3[common_pattern as usize]
        } else if mode == 7 {
            &BC7_3_ASTC2_PATTERNS2[common_pattern as usize]
        } else {
            &ASTC_BC7_PATTERNS2[common_pattern as usize]
        }
    } else {
        &ZERO_PATTERN
    };

    let mut pixels = [Color32::default(); 16];
    if total_planes == 1 {
        if total_subsets == 1 {
            for (i, px) in pixels.iter_mut().enumerate() {
                *px = block_colors[0][astc.weights[i] as usize];
            }
        } else {
            for (i, px) in pixels.iter_mut().enumerate() {
                *px = block_colors[partition_pattern[i] as usize][astc.weights[i] as usize];
            }
        }
    } else {
        for (i, px) in pixels.iter_mut().enumerate() {
            let wi0 = astc.weights[i * 2] as usize;
            let wi1 = astc.weights[i * 2 + 1] as usize;
            for comp in 0..4 {
                px[comp] = if comp as i32 == astc.ccs {
                    block_colors[0][wi1][comp]
                } else {
                    block_colors[0][wi0][comp]
                };
            }
        }
    }
    pixels
}

/// Decode a 128-bit UASTC block straight to 16 RGBA pixels. Hints are skipped
/// and no blue-contract reordering is applied; neither affects pixel output.
pub fn unpack_uastc(bytes: &[u8; 16], srgb: bool) -> Option<[Color32; 16]> {
    let u = unpack_to_block(bytes, false, false)?;
    Some(unpack_pixels(
        u.mode,
        u.common_pattern,
        u.solid_color,
        &u.astc,
        srgb,
    ))
}
