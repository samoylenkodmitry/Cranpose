//! UASTC HDR 4x4 to BC6H (unsigned) transcode. This is a bounded transcode,
//! not an encoder: endpoints decode from the block's CEM
//! 7/11 payload to qlog12, convert to half floats and then to "blog"
//! (BC6H-log) integers, ASTC weights map to BC6H weights through fixed remap
//! tables, and the delta-endpoint BC6H modes are tried in a fixed order with
//! the first fit winning. The direct-endpoint modes 10 (1 subset) and 9 (2
//! subsets) always succeed as fallbacks, so the only failure paths are the
//! UASTC HDR validity gates themselves.

use super::bc6h_tables::{
    Bc6hBitLayout, BC6H_2SUBSET_PATTERNS, BC6H_BIT_LAYOUTS, BC6H_LAYOUT_OFFSETS, BC6H_MODE_SIG_BITS,
};
use crate::astc::decode::decode_endpoint;
use crate::astc::dequant::dequant_tables;
use crate::astc::half::{is_half_inf_or_nan, qlog16_to_half};
use crate::astc::unpack::LogAstcBlock;
use crate::uastc::partitions::ASTC_BC7_COMMON_PARTITIONS2;

/// Largest qlog12 value that decodes to a finite half (`MAX_QLOG12`); the
/// mode 7/11 decodes reject anything above it.
const MAX_QLOG12: i32 = 3967;

/// First 1-subset BC6H mode index (`BC6H_FIRST_1SUBSET_MODE_INDEX`); modes
/// below it are 2-subset.
const FIRST_1SUBSET_MODE: usize = 10;

/// The mode field written at the start of each block: 2 bits for modes 0-1,
/// 5 bits otherwise.
const MODE_BITS: [u64; 14] = [
    0b00, 0b01, 0b00010, 0b00110, 0b01010, 0b01110, 0b10010, 0b10110, 0b11010, 0b11110, 0b00011,
    0b00111, 0b01011, 0b01111,
];

/// The order the delta-endpoint 2-subset modes are tried in, largest base bits
/// first.
const MODE_ORDER: [usize; 9] = [2, 3, 4, 0, 5, 6, 7, 8, 1];

/// How many of the ASTC/BC7 common 2-subset patterns BC6H can use
/// (`TOTAL_ASTC_BC6H_COMMON_PARTITIONS2`): the first 27 of the table's 30 (the
/// last three need BC7 pattern indices >= 32, past BC6H's 5-bit field).
const TOTAL_COMMON_PARTITIONS: usize = 27;

/// A logical BC6H block ready for packing: endpoints are
/// `[comp][subset * 2 + low/high]`, already blog-quantized and
/// delta-packed for the chosen mode, and weights already have any skipped
/// anchor MSBs cleared.
pub(crate) struct LogicalBlock {
    /// BC6H mode, 0..=13.
    pub(crate) mode: usize,
    /// 2-subset partition pattern index; 0 for the 1-subset modes.
    pub(crate) partition_pattern: usize,
    /// Packed endpoint fields, `[comp][subset * 2 + low/high]`.
    pub(crate) endpoints: [[u32; 4]; 3],
    /// Per-texel weight selectors (4-bit for 1 subset, 3-bit for 2).
    pub(crate) weights: [u8; 16],
}

/// Texel `i` (row-major) of 2-subset pattern `pat`; the low bit is the subset
/// and bit 7 flags an anchor texel.
#[inline]
fn pattern_texel(pat: usize, i: usize) -> u8 {
    BC6H_2SUBSET_PATTERNS[pat * 4 + i / 4][i % 4]
}

/// Append `num_bits` of `val` to the 128-bit block image (`write_bits`).
fn write_bits(val: u64, num_bits: u32, bit_pos: &mut u32, l: &mut u64, h: &mut u64) {
    debug_assert!(num_bits > 0 && num_bits < 64 && *bit_pos < 128);
    debug_assert!(val < (1u64 << num_bits));
    if *bit_pos < 64 {
        *l |= val << *bit_pos;
        if *bit_pos + num_bits > 64 {
            *h |= val >> (64 - *bit_pos);
        }
    } else {
        *h |= val << (*bit_pos - 64);
    }
    *bit_pos += num_bits;
    debug_assert!(*bit_pos <= 128);
}

/// Append `num_bits` of `val` in reversed bit order (`write_rev_bits`), used
/// for layout runs whose first bit index exceeds their last.
fn write_rev_bits(val: u64, num_bits: u32, bit_pos: &mut u32, l: &mut u64, h: &mut u64) {
    for i in 0..num_bits {
        write_bits((val >> (num_bits - 1 - i)) & 1, 1, bit_pos, l, h);
    }
}

/// Pack a logical BC6H block into its 16-byte physical form: mode bits, then
/// the mode's bit-layout program over the endpoint fields and partition
/// pattern, then the per-texel weights (anchor
/// texels drop their weight MSB, which the encoders have already cleared).
pub(crate) fn pack_block(log: &LogicalBlock) -> [u8; 16] {
    let mode = log.mode;
    let mut l = MODE_BITS[mode];
    let mut h = 0u64;
    let mut bit_pos: u32 = if mode >= 2 { 5 } else { 2 };

    let num_subsets = if mode >= FIRST_1SUBSET_MODE { 1 } else { 2 };
    debug_assert!(if num_subsets == 2 {
        log.partition_pattern < 32
    } else {
        log.partition_pattern == 0
    });
    for c in 0..3 {
        debug_assert!(log.endpoints[c][0] < (1 << BC6H_MODE_SIG_BITS[mode][0]));
        for j in 1..4 {
            debug_assert!(log.endpoints[c][j] < (1 << BC6H_MODE_SIG_BITS[mode][c + 1]));
        }
    }

    let mut li = BC6H_LAYOUT_OFFSETS[mode];
    loop {
        let lay: &Bc6hBitLayout = &BC6H_BIT_LAYOUTS[li];
        if lay.comp == -1 {
            break;
        }
        let mut v = if lay.comp == 3 {
            log.partition_pattern as u32
        } else {
            log.endpoints[lay.comp as usize][lay.index as usize]
        };

        if lay.first_bit == -1 {
            write_bits(
                ((v >> lay.last_bit) & 1) as u64,
                1,
                &mut bit_pos,
                &mut l,
                &mut h,
            );
        } else {
            let total_bits = (lay.last_bit as i32 - lay.first_bit as i32).unsigned_abs() + 1;
            v >>= lay.first_bit.min(lay.last_bit) as u32;
            v &= (1 << total_bits) - 1;
            if lay.first_bit > lay.last_bit {
                write_rev_bits(v as u64, total_bits, &mut bit_pos, &mut l, &mut h);
            } else {
                write_bits(v as u64, total_bits, &mut bit_pos, &mut l, &mut h);
            }
        }
        li += 1;
    }

    let num_mode_sel_bits: u32 = if num_subsets == 1 { 4 } else { 3 };
    for (i, &sel) in log.weights.iter().enumerate() {
        let mut num_bits = num_mode_sel_bits;
        if num_subsets == 2 {
            num_bits -= (pattern_texel(log.partition_pattern, i) >> 7) as u32;
        } else if i == 0 {
            num_bits -= 1;
        }
        write_bits(sel as u64, num_bits, &mut bit_pos, &mut l, &mut h);
    }
    debug_assert_eq!(bit_pos, 128);

    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&l.to_le_bytes());
    out[8..].copy_from_slice(&h.to_le_bytes());
    out
}

/// Quantize a half to an unsigned `num_bits`-bit blog value with a closed-form
/// approximation (not the exact minimum-error quantizer, but very close). The
/// input must be finite and nonnegative (at most 0x7BFF), which the callers
/// guarantee.
#[inline]
pub(crate) fn half_to_blog(h: u16, num_bits: u32) -> u32 {
    debug_assert!(h <= 0x7BFF);
    (h as u32 * 64 + 30) / (31 * (1u32 << (16 - num_bits)))
}

/// Encode with mode 10: direct (non-delta) 10-bit endpoints and 4-bit
/// weights. Always succeeds; the terminal fallback of the 1-subset path.
/// `endpoints` is `[comp][low/high]` halves.
fn enc_block_mode10(endpoints: &[[u16; 2]; 3], weights: &[u8; 16]) -> [u8; 16] {
    let mut log = LogicalBlock {
        mode: FIRST_1SUBSET_MODE,
        partition_pattern: 0,
        endpoints: [[0; 4]; 3],
        weights: *weights,
    };
    for (dst, src) in log.endpoints.iter_mut().zip(endpoints) {
        dst[0] = half_to_blog(src[0], 10);
        dst[1] = half_to_blog(src[1], 10);
    }
    // BC6H implicitly anchors texel 0's weight MSB to 0; invert the selectors
    // and swap the endpoints if it is set.
    if log.weights[0] & 8 != 0 {
        for w in log.weights.iter_mut() {
            *w = 15 - *w;
        }
        for ep in log.endpoints.iter_mut() {
            ep.swap(0, 1);
        }
    }
    pack_block(&log)
}

/// Encode a 1-subset block with 4-bit weights: try the delta-endpoint modes
/// 13, 12, 11 in that order and take the first whose per-channel deltas fit,
/// falling back to mode 10.
fn enc_block_1subset_4bit_weights(endpoints: &[[u16; 2]; 3], weights: &[u8; 16]) -> [u8; 16] {
    debug_assert!(weights.iter().all(|&w| w <= 15));
    for mode in (FIRST_1SUBSET_MODE + 1..=13).rev() {
        let num_base_bits = BC6H_MODE_SIG_BITS[mode][0] as u32;
        let num_delta_bits = BC6H_MODE_SIG_BITS[mode][1] as u32;
        let delta_bitmask = (1i32 << num_delta_bits) - 1;

        let mut blog = [[0u32; 2]; 3];
        for (b, src) in blog.iter_mut().zip(endpoints) {
            b[0] = half_to_blog(src[0], num_base_bits);
            b[1] = half_to_blog(src[1], num_base_bits);
        }

        let mut log = LogicalBlock {
            mode,
            partition_pattern: 0,
            endpoints: [[0; 4]; 3],
            weights: *weights,
        };
        if log.weights[0] & 8 != 0 {
            for w in log.weights.iter_mut() {
                *w = 15 - *w;
            }
            for b in blog.iter_mut() {
                b.swap(0, 1);
            }
        }

        let max_delta = (1i32 << (num_delta_bits - 1)) - 1;
        let min_delta = -(max_delta + 1);

        let mut failed = false;
        for (dst, b) in log.endpoints.iter_mut().zip(&blog) {
            dst[0] = b[0];
            let delta = b[1] as i32 - b[0] as i32;
            if delta < min_delta || delta > max_delta {
                failed = true;
                break;
            }
            dst[1] = (delta & delta_bitmask) as u32;
        }
        if !failed {
            return pack_block(&log);
        }
    }
    enc_block_mode10(endpoints, weights)
}

/// Encode a 1-subset block as two equal subsets with mode 9's direct 6-bit
/// endpoints and 3-bit weights. Always succeeds; the terminal fallback of the
/// 3-bit-weight path.
fn enc_block_1subset_mode9_3bit_weights(endpoints: &[[u16; 2]; 3], weights: &[u8; 16]) -> [u8; 16] {
    let mut log = LogicalBlock {
        mode: 9,
        partition_pattern: 0,
        endpoints: [[0; 4]; 3],
        weights: *weights,
    };
    for (dst, src) in log.endpoints.iter_mut().zip(endpoints) {
        dst[0] = half_to_blog(src[0], 6);
        dst[2] = dst[0];
        dst[1] = half_to_blog(src[1], 6);
        dst[3] = dst[1];
    }

    // Pattern 0's anchors are texels 0 (subset 0) and 15 (subset 1); clear
    // each anchor's weight MSB by swapping that subset's endpoints and
    // inverting its texels' selectors.
    if log.weights[0] & 4 != 0 {
        for ep in log.endpoints.iter_mut() {
            ep.swap(0, 1);
        }
        for i in 0..16 {
            if pattern_texel(0, i) & 0x7F == 0 {
                log.weights[i] = 7 - log.weights[i];
            }
        }
    }
    if log.weights[15] & 4 != 0 {
        for ep in log.endpoints.iter_mut() {
            ep.swap(2, 3);
        }
        for i in 0..16 {
            if pattern_texel(0, i) & 0x7F == 1 {
                log.weights[i] = 7 - log.weights[i];
            }
        }
    }
    pack_block(&log)
}

/// Encode a 1-subset block with 3-bit weights as two equal subsets: try the
/// delta-endpoint 2-subset modes in `MODE_ORDER` with pattern 0 and duplicated
/// endpoints, falling back to direct mode 9.
fn enc_block_1subset_3bit_weights(endpoints: &[[u16; 2]; 3], weights: &[u8; 16]) -> [u8; 16] {
    debug_assert!(weights.iter().all(|&w| w <= 7));
    for &mode in MODE_ORDER.iter() {
        let num_base_bits = BC6H_MODE_SIG_BITS[mode][0] as u32;

        let mut blog = [[0u32; 4]; 3];
        for (b, src) in blog.iter_mut().zip(endpoints) {
            b[0] = half_to_blog(src[0], num_base_bits);
            b[2] = b[0];
            b[1] = half_to_blog(src[1], num_base_bits);
            b[3] = b[1];
        }

        let mut log = LogicalBlock {
            mode,
            partition_pattern: 0,
            endpoints: [[0; 4]; 3],
            weights: *weights,
        };
        if log.weights[0] & 4 != 0 {
            for b in blog.iter_mut() {
                b.swap(0, 1);
            }
            for i in 0..16 {
                if pattern_texel(0, i) & 0x7F == 0 {
                    log.weights[i] = 7 - log.weights[i];
                }
            }
        }
        if log.weights[15] & 4 != 0 {
            for b in blog.iter_mut() {
                b.swap(2, 3);
            }
            for i in 0..16 {
                if pattern_texel(0, i) & 0x7F == 1 {
                    log.weights[i] = 7 - log.weights[i];
                }
            }
        }

        if delta_pack_endpoints(mode, &blog, &mut log.endpoints) {
            return pack_block(&log);
        }
    }
    enc_block_1subset_mode9_3bit_weights(endpoints, weights)
}

/// Delta-encode four blog endpoint fields against subset 0's low endpoint
/// with mode `mode`'s per-channel delta widths. Returns false when any delta
/// overflows its signed field, which sends the encoders to the next mode.
fn delta_pack_endpoints(mode: usize, blog: &[[u32; 4]; 3], out: &mut [[u32; 4]; 3]) -> bool {
    for c in 0..3 {
        let num_delta_bits = BC6H_MODE_SIG_BITS[mode][c + 1] as u32;
        let delta_bitmask = (1i32 << num_delta_bits) - 1;
        let max_delta = (1i32 << (num_delta_bits - 1)) - 1;
        let min_delta = -(max_delta + 1);

        out[c][0] = blog[c][0];
        let base = blog[c][0] as i32;
        let d0 = blog[c][1] as i32 - base;
        let d1 = blog[c][2] as i32 - base;
        let d2 = blog[c][3] as i32 - base;
        if d0 < min_delta
            || d0 > max_delta
            || d1 < min_delta
            || d1 > max_delta
            || d2 < min_delta
            || d2 > max_delta
        {
            return false;
        }
        out[c][1] = (d0 & delta_bitmask) as u32;
        out[c][2] = (d1 & delta_bitmask) as u32;
        out[c][3] = (d2 & delta_bitmask) as u32;
    }
    true
}

/// Encode a 2-subset block with mode 9's direct 6-bit endpoints. Always
/// succeeds; the terminal fallback of the 2-subset path. `endpoints` is
/// `[subset][comp][low/high]` halves.
fn enc_block_2subset_mode9_3bit_weights(
    common_part_index: usize,
    endpoints: &[[[u16; 2]; 3]; 2],
    weights: &[u8; 16],
) -> [u8; 16] {
    let mut log = LogicalBlock {
        mode: 9,
        partition_pattern: 0,
        endpoints: [[0; 4]; 3],
        weights: *weights,
    };
    for (s, sub) in endpoints.iter().enumerate() {
        for (c, comp) in sub.iter().enumerate() {
            log.endpoints[c][s * 2] = half_to_blog(comp[0], 6);
            log.endpoints[c][s * 2 + 1] = half_to_blog(comp[1], 6);
        }
    }

    let common = &ASTC_BC7_COMMON_PARTITIONS2[common_part_index];
    // ASTC and BC7 label the subsets oppositely for inverted patterns; swap
    // the subsets' endpoints to relabel.
    if common.invert {
        for ep in log.endpoints.iter_mut() {
            ep.swap(0, 2);
            ep.swap(1, 3);
        }
    }
    let pat = common.bc7 as usize;

    let mut swap_flags = [false; 2];
    for i in 0..16 {
        let p = pattern_texel(pat, i);
        if p & 0x80 != 0 && log.weights[i] & 4 != 0 {
            swap_flags[(p & 1) as usize] = true;
        }
    }
    for (s, &flag) in swap_flags.iter().enumerate() {
        if flag {
            for ep in log.endpoints.iter_mut() {
                ep.swap(s * 2, s * 2 + 1);
            }
            for i in 0..16 {
                if (pattern_texel(pat, i) & 0x7F) as usize == s {
                    log.weights[i] = 7 - log.weights[i];
                }
            }
        }
    }

    log.partition_pattern = pat;
    pack_block(&log)
}

/// Encode a 2-subset block with 3-bit weights: try the delta-endpoint 2-subset
/// modes in `MODE_ORDER`, falling back to direct mode 9.
fn enc_block_2subset_3bit_weights(
    common_part_index: usize,
    endpoints: &[[[u16; 2]; 3]; 2],
    weights: &[u8; 16],
) -> [u8; 16] {
    debug_assert!(weights.iter().all(|&w| w <= 7));
    for &mode in MODE_ORDER.iter() {
        let num_base_bits = BC6H_MODE_SIG_BITS[mode][0] as u32;

        let mut blog = [[0u32; 4]; 3];
        for (s, sub) in endpoints.iter().enumerate() {
            for (c, comp) in sub.iter().enumerate() {
                blog[c][s * 2] = half_to_blog(comp[0], num_base_bits);
                blog[c][s * 2 + 1] = half_to_blog(comp[1], num_base_bits);
            }
        }

        let mut log = LogicalBlock {
            mode,
            partition_pattern: 0,
            endpoints: [[0; 4]; 3],
            weights: *weights,
        };

        let common = &ASTC_BC7_COMMON_PARTITIONS2[common_part_index];
        if common.invert {
            for b in blog.iter_mut() {
                b.swap(0, 2);
                b.swap(1, 3);
            }
        }
        let pat = common.bc7 as usize;

        let mut swap_flags = [false; 2];
        for i in 0..16 {
            let p = pattern_texel(pat, i);
            if p & 0x80 != 0 && log.weights[i] & 4 != 0 {
                swap_flags[(p & 1) as usize] = true;
            }
        }
        for (s, &flag) in swap_flags.iter().enumerate() {
            if flag {
                for b in blog.iter_mut() {
                    b.swap(s * 2, s * 2 + 1);
                }
                for i in 0..16 {
                    if (pattern_texel(pat, i) & 0x7F) as usize == s {
                        log.weights[i] = 7 - log.weights[i];
                    }
                }
            }
        }

        if delta_pack_endpoints(mode, &blog, &mut log.endpoints) {
            log.partition_pattern = pat;
            return pack_block(&log);
        }
    }
    enc_block_2subset_mode9_3bit_weights(common_part_index, endpoints, weights)
}

/// Encode a solid-color block: all-zero weights and equal endpoints per
/// channel, through the mode 13/12/11 delta path (mode 13 always fits a zero
/// delta). `None` when any component has the
/// half sign bit set; BC6H unsigned cannot represent negative colors. Alpha
/// is ignored (BC6H is RGB).
fn enc_block_solid_color(color: &[u16; 4]) -> Option<[u8; 16]> {
    if (color[0] | color[1] | color[2]) & 0x8000 != 0 {
        return None;
    }
    let endpoints = [
        [color[0], color[0]],
        [color[1], color[1]],
        [color[2], color[2]],
    ];
    Some(enc_block_1subset_4bit_weights(&endpoints, &[0u8; 16]))
}

/// Decode one CEM 7 or CEM 11 endpoint payload to qlog12 endpoints:
/// dequantize the ISE symbols to [0,255] unless the range is already 20 (256
/// levels), run the
/// shared CEM bit-plumbing, and reject any component above `MAX_QLOG12`
/// (which would decode to Inf/NaN). Returns `[low/high][comp]`.
fn decode_cem_to_qlog12(cem: u32, syms: &[u8], ise_range: u32) -> Option<[[i32; 3]; 2]> {
    let n = if cem == 7 { 4 } else { 6 };
    let mut vals = [0u8; 6];
    if ise_range == 20 {
        vals[..n].copy_from_slice(&syms[..n]);
    } else {
        let tab = &dequant_tables().endpoints[(ise_range - 4) as usize];
        for (v, &s) in vals[..n].iter_mut().zip(syms) {
            *v = tab[s as usize];
        }
    }

    let ep = decode_endpoint(cem, &vals);
    let mut e = [[0i32; 3]; 2];
    for c in 0..3 {
        e[0][c] = ep[c][0];
        e[1][c] = ep[c][1];
        if e[0][c] > MAX_QLOG12 || e[1][c] > MAX_QLOG12 {
            return None;
        }
    }
    Some(e)
}

/// Shift a qlog12 value up to qlog16 and convert it to a half.
#[inline]
fn qlog12_to_half(q: i32) -> u16 {
    qlog16_to_half(q << 4)
}

/// Convert decoded qlog12 endpoints to `[comp][low/high]` halves, rejecting
/// any Inf/NaN result (the per-endpoint sanity check both transcode paths
/// perform after conversion to half).
fn qlog12_endpoints_to_half(e: &[[i32; 3]; 2]) -> Option<[[u16; 2]; 3]> {
    let mut h_e = [[0u16; 2]; 3];
    for c in 0..3 {
        for lh in 0..2 {
            let h = qlog12_to_half(e[lh][c]);
            if is_half_inf_or_nan(h) {
                return None;
            }
            h_e[c][lh] = h;
        }
    }
    Some(h_e)
}

/// Transcode a 1-partition block's weights and encode. Weight ISE range 5
/// (8 levels) matches BC6H 3-bit weights exactly and goes
/// through the two-equal-subsets 3-bit path; every other range remaps to
/// 4-bit BC6H weights (the tables are indexed by raw ISE symbol, whose order
/// interleaves trit/quint groups, hence the non-monotonic entries).
fn transcode_1subset(h_e: &[[u16; 2]; 3], log: &LogAstcBlock) -> [u8; 16] {
    debug_assert!((1..=8).contains(&log.weight_ise_range));
    let mut w = [0u8; 16];
    if log.weight_ise_range == 5 {
        w.copy_from_slice(&log.weights[..16]);
        return enc_block_1subset_3bit_weights(h_e, &w);
    }

    static REMAP1: [u8; 3] = [0, 8, 15];
    static REMAP2: [u8; 4] = [0, 5, 10, 15];
    static REMAP3: [u8; 5] = [0, 4, 7, 11, 15];
    static REMAP4: [u8; 6] = [0, 15, 3, 12, 6, 9];
    static REMAP6: [u8; 10] = [0, 15, 2, 13, 3, 12, 5, 10, 6, 9];
    static REMAP7: [u8; 12] = [0, 15, 4, 11, 1, 14, 5, 10, 2, 13, 6, 9];
    let remap: &[u8] = match log.weight_ise_range {
        1 => &REMAP1,
        2 => &REMAP2,
        3 => &REMAP3,
        4 => &REMAP4,
        6 => &REMAP6,
        7 => &REMAP7,
        // Range 8 is 16 levels of pure bits: already 4-bit BC6H weights.
        _ => &[],
    };
    for (dst, &sym) in w.iter_mut().zip(&log.weights[..16]) {
        *dst = if remap.is_empty() {
            sym
        } else {
            remap[sym as usize]
        };
    }
    enc_block_1subset_4bit_weights(h_e, &w)
}

/// Transcode a 2-partition block: enforce the UASTC HDR CEM/range pairs,
/// decode both subsets' endpoints, remap the
/// weights to 3-bit, and encode with the common partition pattern.
fn transcode_2subsets(common_part_index: usize, log: &LogAstcBlock) -> Option<[u8; 16]> {
    let cem = log.color_endpoint_modes[0] as u32;
    // Both CEMs must be equal in 2-subset UASTC HDR.
    if log.color_endpoint_modes[1] as u32 != cem || (cem != 7 && cem != 11) {
        return None;
    }
    let pair = (log.weight_ise_range, log.endpoint_ise_range);
    let legal = if cem == 7 {
        matches!(pair, (1, 20) | (2, 20) | (3, 19) | (4, 17) | (5, 15))
    } else {
        matches!(pair, (1, 14) | (2, 12))
    };
    if !legal {
        return None;
    }

    let vals_per_subset = if cem == 7 { 4 } else { 6 };
    let mut endpoints = [[[0u16; 2]; 3]; 2];
    for (s, ep) in endpoints.iter_mut().enumerate() {
        let e = decode_cem_to_qlog12(
            cem,
            &log.endpoints[s * vals_per_subset..],
            log.endpoint_ise_range,
        )?;
        *ep = qlog12_endpoints_to_half(&e)?;
    }

    static REMAP1: [u8; 3] = [0, 4, 7];
    static REMAP2: [u8; 4] = [0, 2, 5, 7];
    static REMAP3: [u8; 5] = [0, 2, 4, 5, 7];
    static REMAP4: [u8; 6] = [0, 7, 1, 6, 3, 4];
    let remap: &[u8] = match log.weight_ise_range {
        1 => &REMAP1,
        2 => &REMAP2,
        3 => &REMAP3,
        4 => &REMAP4,
        // Range 5 is 8 levels of pure bits: already 3-bit BC6H weights.
        _ => &[],
    };
    let mut w = [0u8; 16];
    for (dst, &sym) in w.iter_mut().zip(&log.weights[..16]) {
        *dst = if remap.is_empty() {
            sym
        } else {
            remap[sym as usize]
        };
    }

    Some(enc_block_2subset_3bit_weights(
        common_part_index,
        &endpoints,
        &w,
    ))
}

/// The common-pattern index for an ASTC 2-subset partition seed, or `None`
/// when the seed is not one of the 27 BC6H-usable common patterns. The 27
/// usable patterns have distinct ASTC seeds (asserted below), so a first-match
/// scan resolves each seed unambiguously.
fn common_partition_index(partition_id: u32) -> Option<usize> {
    ASTC_BC7_COMMON_PARTITIONS2[..TOTAL_COMMON_PARTITIONS]
        .iter()
        .position(|p| p.astc as u32 == partition_id)
}

/// Transcode one unpacked UASTC HDR block to a 16-byte BC6H (unsigned) block.
/// `None` on any condition outside the UASTC HDR envelope that aborts the
/// slice: LDR void extents, negative solid colors, non-4x4 grids, dual-plane
/// blocks, CEM/ISE-range combinations outside the envelope, endpoints that
/// decode past `MAX_QLOG12` or to Inf/NaN halves, 2-partition seeds without a
/// common BC7 pattern, and 3-4 partition blocks.
pub fn transcode_block_bc6h(log: &LogAstcBlock) -> Option<[u8; 16]> {
    if log.solid_color_flag_ldr {
        return None;
    }
    if log.solid_color_flag_hdr {
        return enc_block_solid_color(&log.solid_color);
    }
    if log.grid_width != 4 || log.grid_height != 4 || log.dual_plane {
        return None;
    }

    match log.num_partitions {
        1 => {
            if !(1..=8).contains(&log.weight_ise_range) {
                return None;
            }
            let cem = log.color_endpoint_modes[0] as u32;
            match cem {
                7 => {
                    if log.endpoint_ise_range != 20 {
                        return None;
                    }
                }
                11 => {
                    // Weight ranges up to 7 leave room for full 8-bit
                    // endpoints; range 8 (16-level weights) forces the
                    // 192-level endpoint range.
                    let required = if log.weight_ise_range <= 7 { 20 } else { 19 };
                    if log.endpoint_ise_range != required {
                        return None;
                    }
                }
                _ => return None,
            }
            let e = decode_cem_to_qlog12(cem, &log.endpoints, log.endpoint_ise_range)?;
            let h_e = qlog12_endpoints_to_half(&e)?;
            Some(transcode_1subset(&h_e, log))
        }
        2 => {
            let common_index = common_partition_index(log.partition_id)?;
            transcode_2subsets(common_index, log)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A first-match linear search over the 27 usable common patterns resolves
    // each ASTC seed unambiguously only if those seeds are distinct; this
    // guards that invariant.
    #[test]
    fn common_partition_seeds_are_distinct() {
        let seeds = &ASTC_BC7_COMMON_PARTITIONS2[..TOTAL_COMMON_PARTITIONS];
        for (i, a) in seeds.iter().enumerate() {
            for b in &seeds[i + 1..] {
                assert_ne!(a.astc, b.astc);
            }
        }
        // The BC6H pattern field is 5 bits; all 27 usable entries fit.
        for p in &ASTC_BC7_COMMON_PARTITIONS2[..TOTAL_COMMON_PARTITIONS] {
            assert!(p.bc7 < 32);
        }
    }

    // Every mode's layout program must consume exactly 128 bits together with
    // its mode field and weights (the pack_block debug assertion enforces the
    // count; this drives all 14 programs).
    #[test]
    fn pack_block_fills_128_bits_for_every_mode() {
        for (mode, &want) in MODE_BITS.iter().enumerate() {
            let log = LogicalBlock {
                mode,
                partition_pattern: 0,
                endpoints: [[0; 4]; 3],
                weights: [0; 16],
            };
            let out = pack_block(&log);
            // The mode field must survive in the low bits.
            let mask = if mode >= 2 { 0x1F } else { 0x03 };
            assert_eq!(out[0] & mask, want as u8);
        }
    }

    // half_to_blog must stay within each mode's base-bit budget for the
    // largest legal half input.
    #[test]
    fn half_to_blog_fits_base_bits() {
        for (mode, sig) in BC6H_MODE_SIG_BITS.iter().enumerate() {
            let bits = sig[0] as u32;
            assert!(half_to_blog(0x7BFF, bits) < (1 << bits), "mode {mode}");
        }
    }
}
