//! Physical ASTC block unpack: decode a 16-byte ASTC block into a logical
//! description (grid config, partitions, CEMs, ISE endpoint and weight
//! symbols), the subset needed for UASTC HDR 4x4 sources. Endpoints are
//! BISE-decoded forward from the end of the config bits; weights are
//! BISE-decoded from the bit-reversed block starting at bit zero, because ASTC
//! stores them from the top of the block downward.

use super::tables::{ASTC_QUINT_DECODE, ASTC_TRIT_DECODE};
use crate::uastc::tables::ASTC_BISE_RANGE_TABLE;

/// Maximum weight-grid values in a block (both planes together).
pub const MAX_GRID_WEIGHTS: usize = 64;
/// Maximum ISE endpoint values in a block.
pub const MAX_ENDPOINTS: usize = 18;
/// Smallest legal endpoint BISE range (`FIRST_VALID_ENDPOINT_ISE_RANGE`).
pub const FIRST_VALID_ENDPOINT_ISE_RANGE: u32 = 4;
/// Largest legal endpoint BISE range.
pub const LAST_VALID_ENDPOINT_ISE_RANGE: u32 = 20;
/// Largest legal weight BISE range.
pub const LAST_VALID_WEIGHT_ISE_RANGE: u32 = 11;

/// A logical ASTC block: the decoded configuration plus raw ISE symbols. An
/// error is represented by returning `None` from [`unpack_block`] instead of a
/// flag on this struct.
#[derive(Clone, Copy)]
pub struct LogAstcBlock {
    /// LDR void extent: `solid_color` holds four 16-bit UNORM components.
    pub solid_color_flag_ldr: bool,
    /// HDR void extent: `solid_color` holds four half-float components.
    pub solid_color_flag_hdr: bool,
    /// The void-extent color (meaning depends on the LDR/HDR flag).
    pub solid_color: [u16; 4],
    /// Weight grid width in texels (not the block width).
    pub grid_width: u32,
    /// Weight grid height in texels.
    pub grid_height: u32,
    /// Whether the block carries a second weight plane.
    pub dual_plane: bool,
    /// Weight BISE range, 0..=11.
    pub weight_ise_range: u32,
    /// Endpoint BISE range, 4..=20 (inferred from the leftover bit count).
    pub endpoint_ise_range: u32,
    /// Color component selector, 0..=3; only meaningful in dual-plane mode.
    pub color_component_selector: u32,
    /// Number of partitions (subsets), 1..=4.
    pub num_partitions: u32,
    /// 10-bit partition pattern seed; 0 when there is one partition.
    pub partition_id: u32,
    /// Each partition's color endpoint mode.
    pub color_endpoint_modes: [u8; 4],
    /// ISE weight-grid symbols; dual-plane order is p0,p1 interleaved.
    pub weights: [u8; MAX_GRID_WEIGHTS],
    /// ISE endpoint symbols, subset-major.
    pub endpoints: [u8; MAX_ENDPOINTS],
}

impl Default for LogAstcBlock {
    /// An all-zero logical block. Hand-written because `derive(Default)` does
    /// not cover the 64-element weight array (derived `Default` stops at 32).
    fn default() -> Self {
        Self {
            solid_color_flag_ldr: false,
            solid_color_flag_hdr: false,
            solid_color: [0; 4],
            grid_width: 0,
            grid_height: 0,
            dual_plane: false,
            weight_ise_range: 0,
            endpoint_ise_range: 0,
            color_component_selector: 0,
            num_partitions: 0,
            partition_id: 0,
            color_endpoint_modes: [0; 4],
            weights: [0; MAX_GRID_WEIGHTS],
            endpoints: [0; MAX_ENDPOINTS],
        }
    }
}

/// A 128-bit ASTC block as a bit string, with the random-access bit reads the
/// unpack needs.
#[derive(Clone, Copy)]
pub struct Bits128 {
    lo: u64,
    hi: u64,
}

impl Bits128 {
    /// Build from the block's 16 bytes, little-endian.
    pub fn from_le_bytes(b: &[u8; 16]) -> Self {
        Self {
            lo: u64::from_le_bytes(b[0..8].try_into().unwrap()),
            hi: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        }
    }

    /// `len` bits (1..=32) starting at bit `ofs`.
    pub fn get_bits(&self, ofs: u32, len: u32) -> u32 {
        debug_assert!((1..=32).contains(&len) && ofs + len <= 128);
        let v = if ofs >= 64 {
            self.hi >> (ofs - 64)
        } else if ofs == 0 {
            self.lo
        } else {
            (self.lo >> ofs) | (self.hi << (64 - ofs))
        };
        (v & ((1u64 << len) - 1)) as u32
    }

    /// Sequential read: `len` bits at `*ofs`, then advance `*ofs`.
    pub fn next_bits(&self, ofs: &mut u32, len: u32) -> u32 {
        let x = self.get_bits(*ofs, len);
        *ofs += len;
        x
    }

    /// The 128-bit value with all bits reversed (bit 0 <-> bit 127), used to
    /// read the weight data that ASTC stores from the top of the block down.
    pub fn reversed(&self) -> Self {
        Self {
            lo: self.hi.reverse_bits(),
            hi: self.lo.reverse_bits(),
        }
    }
}

/// Levels representable by a BISE range.
pub fn ise_levels(range: u32) -> u32 {
    let t = &ASTC_BISE_RANGE_TABLE[range as usize];
    ((1 + 2 * t[1] + 4 * t[2]) << t[0]) as u32
}

/// Bits consumed by a BISE sequence of `count` values in `range`, per the
/// spec's 18.22 Data Size Determination.
pub fn ise_sequence_bits(count: i32, range: u32) -> i32 {
    let t = &ASTC_BISE_RANGE_TABLE[range as usize];
    let mut total = t[0] * count;
    total += (t[1] * 8 * count + 4) / 5;
    total += (t[2] * 7 * count + 2) / 3;
    total
}

/// Decode one trit block: up to five values, each `bits_per_val` mantissa bits
/// interleaved with the packed T bits, then the trit lookup.
fn decode_trit_block(vals: &mut [u8], bits: &Bits128, bit_ofs: &mut u32, bits_per_val: u32) {
    debug_assert!((1..=5).contains(&vals.len()));
    const T_BITS: [u32; 5] = [2, 2, 1, 2, 1];
    let mut m = [0u32; 5];
    let mut t = 0u32;
    let mut t_ofs = 0;
    for (c, mc) in m.iter_mut().enumerate().take(vals.len()) {
        if bits_per_val != 0 {
            *mc = bits.next_bits(bit_ofs, bits_per_val);
        }
        t |= bits.next_bits(bit_ofs, T_BITS[c]) << t_ofs;
        t_ofs += T_BITS[c];
    }
    let trits = &ASTC_TRIT_DECODE[t as usize];
    for (i, v) in vals.iter_mut().enumerate() {
        *v = (((trits[i] as u32) << bits_per_val) | m[i]) as u8;
    }
}

/// Decode one quint block: up to three values, mantissa bits interleaved with
/// the packed T bits, then the quint lookup.
fn decode_quint_block(vals: &mut [u8], bits: &Bits128, bit_ofs: &mut u32, bits_per_val: u32) {
    debug_assert!((1..=3).contains(&vals.len()));
    const T_BITS: [u32; 3] = [3, 2, 2];
    let mut m = [0u32; 3];
    let mut t = 0u32;
    let mut t_ofs = 0;
    for (c, mc) in m.iter_mut().enumerate().take(vals.len()) {
        if bits_per_val != 0 {
            *mc = bits.next_bits(bit_ofs, bits_per_val);
        }
        t |= bits.next_bits(bit_ofs, T_BITS[c]) << t_ofs;
        t_ofs += T_BITS[c];
    }
    let quints = &ASTC_QUINT_DECODE[t as usize];
    for (i, v) in vals.iter_mut().enumerate() {
        *v = (((quints[i] as u32) << bits_per_val) | m[i]) as u8;
    }
}

/// BISE-decode `vals.len()` symbols of `ise_range` starting at `bit_ofs`.
pub fn decode_bise(ise_range: u32, vals: &mut [u8], bits: &Bits128, mut bit_ofs: u32) {
    let t = &ASTC_BISE_RANGE_TABLE[ise_range as usize];
    let bits_per_val = t[0] as u32;
    let num_vals = vals.len();
    if t[1] != 0 {
        // Trits+bits: five values per block.
        for b in 0..num_vals.div_ceil(5) {
            let n = (num_vals - 5 * b).min(5);
            decode_trit_block(
                &mut vals[5 * b..5 * b + n],
                bits,
                &mut bit_ofs,
                bits_per_val,
            );
        }
    } else if t[2] != 0 {
        // Quints+bits: three values per block.
        for b in 0..num_vals.div_ceil(3) {
            let n = (num_vals - 3 * b).min(3);
            decode_quint_block(
                &mut vals[3 * b..3 * b + n],
                bits,
                &mut bit_ofs,
                bits_per_val,
            );
        }
    } else {
        for v in vals.iter_mut() {
            *v = bits.next_bits(&mut bit_ofs, bits_per_val) as u8;
        }
    }
}

/// One row of the block-mode field decode table: bit offsets and sizes for the
/// dual-plane, precision, grid-size, and p bits. Negative offsets mean the
/// field is absent, zero sizes mean the dimension is fixed.
struct DecRow {
    dp_ofs: i8,
    p_ofs: i8,
    w_ofs: i8,
    w_size: i8,
    h_ofs: i8,
    h_size: i8,
    w_bias: i8,
    h_bias: i8,
    p0_ofs: i8,
    p1_ofs: i8,
    p2_ofs: i8,
}

/// Shorthand `DecRow` constructor used by the table literal.
const fn dr(v: [i8; 11]) -> DecRow {
    DecRow {
        dp_ofs: v[0],
        p_ofs: v[1],
        w_ofs: v[2],
        w_size: v[3],
        h_ofs: v[4],
        h_size: v[5],
        w_bias: v[6],
        h_bias: v[7],
        p0_ofs: v[8],
        p1_ofs: v[9],
        p2_ofs: v[10],
    }
}

/// The ten block-mode layout rows, selected by the low bit patterns of the
/// block. The trailing comment on each row gives the grid bias (W H).
static DEC_ROWS: [DecRow; 10] = [
    dr([10, 9, 7, 2, 5, 2, 4, 2, 4, 0, 1]),  // 4 2
    dr([10, 9, 7, 2, 5, 2, 8, 2, 4, 0, 1]),  // 8 2
    dr([10, 9, 5, 2, 7, 2, 2, 8, 4, 0, 1]),  // 2 8
    dr([10, 9, 5, 2, 7, 1, 2, 6, 4, 0, 1]),  // 2 6
    dr([10, 9, 7, 1, 5, 2, 2, 2, 4, 0, 1]),  // 2 2
    dr([10, 9, 0, 0, 5, 2, 12, 2, 4, 2, 3]), // 12 2
    dr([10, 9, 5, 2, 0, 0, 2, 12, 4, 2, 3]), // 2 12
    dr([10, 9, 0, 0, 0, 0, 6, 10, 4, 2, 3]), // 6 10
    dr([10, 9, 0, 0, 0, 0, 10, 6, 4, 2, 3]), // 10 6
    dr([-1, -1, 5, 2, 9, 2, 6, 6, 4, 2, 3]), // 6 6
];

/// Decode a void-extent block: validates the extent fields, reads the four
/// 16-bit components, and rejects HDR extents with Inf/NaN components.
fn decode_void_extent(bits: &Bits128, log: &mut LogAstcBlock) -> bool {
    if bits.get_bits(10, 2) != 0b11 {
        return false;
    }
    let mut ofs = 12;
    let min_s = bits.next_bits(&mut ofs, 13);
    let max_s = bits.next_bits(&mut ofs, 13);
    let min_t = bits.next_bits(&mut ofs, 13);
    let max_t = bits.next_bits(&mut ofs, 13);
    debug_assert_eq!(ofs, 64);

    let all_ones = min_s == 0x1FFF && max_s == 0x1FFF && min_t == 0x1FFF && max_t == 0x1FFF;
    if !all_ones && (min_s >= max_s || min_t >= max_t) {
        return false;
    }

    let hdr = bits.get_bits(9, 1) != 0;
    if hdr {
        log.solid_color_flag_hdr = true;
    } else {
        log.solid_color_flag_ldr = true;
    }

    for c in 0..4 {
        log.solid_color[c] = bits.get_bits(64 + 16 * c as u32, 16) as u16;
    }

    if log.solid_color_flag_hdr {
        for &c in &log.solid_color {
            if super::half::is_half_inf_or_nan(c) {
                return false;
            }
        }
    }
    true
}

/// Decode the 11-bit block-mode config: reserved patterns, void extents, and
/// otherwise the grid dimensions, dual-plane flag, and weight BISE range.
fn decode_config(bits: &Bits128, log: &mut LogAstcBlock) -> bool {
    // Reserved patterns.
    if bits.get_bits(0, 4) == 0 {
        return false;
    }
    if bits.get_bits(0, 2) == 0 && bits.get_bits(6, 3) == 0b111 && bits.get_bits(2, 4) != 0b1111 {
        return false;
    }

    // Void extent.
    if bits.get_bits(0, 9) == 0b111111100 {
        return decode_void_extent(bits, log);
    }

    // Select the layout row from the low bit patterns.
    let x0_2 = bits.get_bits(0, 2);
    let x2_2 = bits.get_bits(2, 2);
    let x5_4 = bits.get_bits(5, 4);
    let x8_1 = bits.get_bits(8, 1);
    let x7_2 = bits.get_bits(7, 2);

    let row_index: i32 = if x0_2 == 0 {
        if x7_2 == 0b00 {
            5
        } else if x7_2 == 0b01 {
            6
        } else if x5_4 == 0b1100 {
            7
        } else if x5_4 == 0b1101 {
            8
        } else if x7_2 == 0b10 {
            9
        } else {
            -1
        }
    } else if x2_2 == 0b00 {
        0
    } else if x2_2 == 0b01 {
        1
    } else if x2_2 == 0b10 {
        2
    } else if x8_1 == 0 {
        3
    } else {
        4
    };
    if row_index < 0 {
        return false;
    }
    let r = &DEC_ROWS[row_index as usize];

    let p_flag = r.p_ofs >= 0 && bits.get_bits(r.p_ofs as u32, 1) != 0;
    let dp = r.dp_ofs >= 0 && bits.get_bits(r.dp_ofs as u32, 1) != 0;

    let mut w = r.w_bias as u32;
    let mut h = r.h_bias as u32;
    if r.w_size != 0 {
        w += bits.get_bits(r.w_ofs as u32, r.w_size as u32);
    }
    if r.h_size != 0 {
        h += bits.get_bits(r.h_ofs as u32, r.h_size as u32);
    }

    let p0 = bits.get_bits(r.p0_ofs as u32, 1);
    let p1 = bits.get_bits(r.p1_ofs as u32, 1);
    let p2 = bits.get_bits(r.p2_ofs as u32, 1);
    let p = p0 | (p1 << 1) | (p2 << 2);
    if p < 2 {
        return false;
    }

    log.grid_width = w;
    log.grid_height = h;
    // Ranges 0..=5 use the base precision; the P bit selects the 10-level
    // family six ranges up.
    log.weight_ise_range = (p - 2) + if p_flag { 6 } else { 0 };
    log.dual_plane = dp;
    true
}

/// Unpack a physical 16-byte ASTC block for a `blk_width` x `blk_height`
/// footprint into its logical description. `None` signals an error (reserved
/// encoding, illegal config, or bad void extent); a per-block failure aborts
/// the whole slice upstream.
pub fn unpack_block(block: &[u8; 16], blk_width: u32, blk_height: u32) -> Option<LogAstcBlock> {
    let bits = Bits128::from_le_bytes(block);
    let rev_bits = bits.reversed();

    let mut log = LogAstcBlock::default();
    if !decode_config(&bits, &mut log) {
        return None;
    }

    if log.solid_color_flag_hdr || log.solid_color_flag_ldr {
        return Some(log);
    }

    // The weight grid cannot exceed the block footprint.
    if log.grid_width > blk_width || log.grid_height > blk_height {
        return None;
    }

    let total_grid_weights =
        (if log.dual_plane { 2 } else { 1 }) * log.grid_width * log.grid_height;
    let total_weight_bits = ise_sequence_bits(total_grid_weights as i32, log.weight_ise_range);

    // 18.24 Illegal Encodings.
    if total_grid_weights == 0
        || total_grid_weights as usize > MAX_GRID_WEIGHTS
        || !(24..=96).contains(&total_weight_bits)
    {
        return None;
    }
    let total_weight_bits = total_weight_bits as u32;
    let end_of_weight_bit_ofs = 128 - total_weight_bits;

    let mut total_extra_bits = 0u32;

    log.num_partitions = bits.get_bits(11, 2) + 1;
    if log.num_partitions == 1 {
        log.color_endpoint_modes[0] = bits.get_bits(13, 4) as u8;
    } else {
        if log.dual_plane && log.num_partitions == 4 {
            return None;
        }
        log.partition_id = bits.get_bits(13, 10);

        let mut cem_bits = bits.get_bits(23, 6);
        if cem_bits & 3 == 0 {
            // All CEMs the same.
            for i in 0..log.num_partitions as usize {
                log.color_endpoint_modes[i] = (cem_bits >> 2) as u8;
            }
        } else {
            // CEMs differ, within up to two adjacent classes. The per-subset
            // class and mode bits spill into extra bits just below the weight
            // data.
            let first_cem_index = ((cem_bits & 3) - 1) * 4;
            total_extra_bits = 3 * log.num_partitions - 4;
            if total_weight_bits + total_extra_bits > 128 {
                return None;
            }
            let mut cem_bit_pos = end_of_weight_bit_ofs - total_extra_bits;

            let mut c = [0u32; 4];
            let mut m = [0u32; 4];
            cem_bits >>= 2;
            for ci in c.iter_mut().take(log.num_partitions as usize) {
                *ci = cem_bits & 1;
                cem_bits >>= 1;
            }
            match log.num_partitions {
                2 => {
                    m[0] = cem_bits & 3;
                    m[1] = bits.next_bits(&mut cem_bit_pos, 2);
                }
                3 => {
                    m[0] = (cem_bits & 1) | (bits.next_bits(&mut cem_bit_pos, 1) << 1);
                    m[1] = bits.next_bits(&mut cem_bit_pos, 2);
                    m[2] = bits.next_bits(&mut cem_bit_pos, 2);
                }
                _ => {
                    for mi in m.iter_mut() {
                        *mi = bits.next_bits(&mut cem_bit_pos, 2);
                    }
                }
            }
            debug_assert_eq!(cem_bit_pos, end_of_weight_bit_ofs);

            for i in 0..log.num_partitions as usize {
                log.color_endpoint_modes[i] = (first_cem_index + c[i] * 4 + m[i]) as u8;
            }
        }
    }

    if log.dual_plane {
        // The two CCS bits sit beneath any extra CEM bits.
        total_extra_bits += 2;
        if total_extra_bits > end_of_weight_bit_ofs {
            return None;
        }
        let ccs_bit_pos = end_of_weight_bit_ofs - total_extra_bits;
        log.color_component_selector = bits.get_bits(ccs_bit_pos, 2);
    }

    let config_bit_pos = 11 + 2 + if log.num_partitions == 1 { 4 } else { 10 + 6 };
    let total_config_bits = config_bit_pos + total_extra_bits;

    let num_remaining_bits = 128 - total_config_bits as i32 - total_weight_bits as i32;
    if num_remaining_bits < 0 {
        return None;
    }

    let mut total_cem_vals = 0u32;
    for i in 0..log.num_partitions as usize {
        total_cem_vals += 2 + 2 * (log.color_endpoint_modes[i] as u32 >> 2);
    }
    if total_cem_vals as usize > MAX_ENDPOINTS {
        return None;
    }

    // Infer the endpoint ISE range: the largest range whose sequence fits the
    // remaining bits.
    let mut endpoint_ise_range: i32 = -1;
    for k in (1..=20u32).rev() {
        if ise_sequence_bits(total_cem_vals as i32, k) <= num_remaining_bits {
            endpoint_ise_range = k as i32;
            break;
        }
    }
    if endpoint_ise_range < FIRST_VALID_ENDPOINT_ISE_RANGE as i32 {
        return None;
    }
    log.endpoint_ise_range = endpoint_ise_range as u32;

    // Endpoints decode forward from the end of the config bits; weights decode
    // from the reversed block starting at bit zero.
    decode_bise(
        log.endpoint_ise_range,
        &mut log.endpoints[..total_cem_vals as usize],
        &bits,
        config_bit_pos,
    );
    decode_bise(
        log.weight_ise_range,
        &mut log.weights[..total_grid_weights as usize],
        &rev_bits,
        0,
    );

    Some(log)
}
