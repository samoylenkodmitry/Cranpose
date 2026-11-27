//! Generic logical-to-physical ASTC block packing (the block packer plus the
//! void-extent packers), used by the UASTC HDR 6x6 intermediate codec to emit
//! standard ASTC 6x6 blocks. The header/config encoding follows Tables 81/82
//! of the ASTC specification; endpoints pack forward after the header, weights
//! pack backward from bit 127 (bit-reversed).

use crate::astc::unpack::{
    ise_sequence_bits, LogAstcBlock, FIRST_VALID_ENDPOINT_ISE_RANGE, LAST_VALID_ENDPOINT_ISE_RANGE,
    LAST_VALID_WEIGHT_ISE_RANGE, MAX_ENDPOINTS, MAX_GRID_WEIGHTS,
};
use crate::uastc::astc_pack::{pack_bise_tab, set_bits};

/// The quint-encode table for the ASTC block packer. It differs from the UASTC
/// transcoder's copy (`uastc::astc_pack_tables::ASTC_QUINT_ENCODE`) only at
/// index 124 (three max quints), which this copy codes as 7 where the UASTC
/// copy uses 31. Both decode identically, but byte-exact packing here requires
/// this variant.
fn helpers_quint_encode() -> [u8; 125] {
    let mut t = crate::uastc::astc_pack_tables::ASTC_QUINT_ENCODE;
    t[124] = 7;
    t
}

/// Most partitions (subsets) an ASTC block may carry.
const MAX_PARTITIONS: u32 = 4;
/// Number of distinct 10-bit partition pattern seeds.
const NUM_PARTITION_PATTERNS: u32 = 1024;

/// Whether `value` is non-negative and fits in `num_bits` bits, used to test
/// each biased grid dimension against its config-row field width.
#[inline]
fn is_packable(value: i32, num_bits: u32) -> bool {
    value >= 0 && value < (1 << num_bits)
}

/// Encode the 11-bit block header (weight grid dims, weight range, dual plane)
/// per Tables 81/82, trying each layout row in turn. `None` for grid dims no
/// row can express.
fn get_config_bits(log: &LogAstcBlock) -> Option<u32> {
    let w = log.grid_width as i32;
    let h = log.grid_height as i32;

    let p_hi = u32::from(log.weight_ise_range >= 6); // high precision
    let dp_p = ((log.dual_plane as u32) << 1) | p_hi;

    // Compute p from the weight range, then rearrange its bits to p0 p2 p1.
    let p = 2 + log.weight_ise_range - if p_hi != 0 { 6 } else { 0 };
    let p = (p >> 1) + ((p & 1) << 2);

    // W+4 H+2
    if is_packable(w - 4, 2) && is_packable(h - 2, 2) {
        return Some(
            (dp_p << 9)
                | (((w - 4) as u32) << 7)
                | (((h - 2) as u32) << 5)
                | ((p & 4) << 2)
                | (p & 3),
        );
    }
    // W+8 H+2
    if is_packable(w - 8, 2) && is_packable(h - 2, 2) {
        return Some(
            (dp_p << 9)
                | (((w - 8) as u32) << 7)
                | (((h - 2) as u32) << 5)
                | ((p & 4) << 2)
                | 4
                | (p & 3),
        );
    }
    // W+2 H+8
    if is_packable(w - 2, 2) && is_packable(h - 8, 2) {
        return Some(
            (dp_p << 9)
                | (((h - 8) as u32) << 7)
                | (((w - 2) as u32) << 5)
                | ((p & 4) << 2)
                | 8
                | (p & 3),
        );
    }
    // W+2 H+6
    if is_packable(w - 2, 2) && is_packable(h - 6, 1) {
        return Some(
            (dp_p << 9)
                | (((h - 6) as u32) << 7)
                | (((w - 2) as u32) << 5)
                | ((p & 4) << 2)
                | 12
                | (p & 3),
        );
    }
    // W+2 H+2
    if is_packable(w - 2, 1) && is_packable(h - 2, 2) {
        return Some(
            (dp_p << 9)
                | ((w as u32) << 7)
                | (((h - 2) as u32) << 5)
                | ((p & 4) << 2)
                | 12
                | (p & 3),
        );
    }
    // 12 H+2
    if w == 12 && is_packable(h - 2, 2) {
        return Some((dp_p << 9) | (((h - 2) as u32) << 5) | (p << 2));
    }
    // W+2 12
    if h == 12 && is_packable(w - 2, 2) {
        return Some((dp_p << 9) | (1 << 7) | (((w - 2) as u32) << 5) | (p << 2));
    }
    // 6 10
    if w == 6 && h == 10 {
        return Some((dp_p << 9) | (3 << 7) | (p << 2));
    }
    // 10 6
    if w == 10 && h == 6 {
        return Some((dp_p << 9) | (0b1101 << 5) | (p << 2));
    }
    // W+6 H+6 (no dual plane or high precision)
    if dp_p == 0 && is_packable(w - 6, 2) && is_packable(h - 6, 2) {
        return Some((((h - 6) as u32) << 9) | 256 | (((w - 6) as u32) << 5) | (p << 2));
    }
    None
}

/// Pack an LDR void extent: all-ones extent coordinates, 16-bit UNORM
/// components.
pub fn pack_void_extent_ldr(r: u16, g: u16, b: u16, a: u16) -> [u8; 16] {
    let mut d = [0xFFu8; 16];
    d[0] = 0b1111_1100;
    d[1] = 0b1111_1101;
    d[8..10].copy_from_slice(&r.to_le_bytes());
    d[10..12].copy_from_slice(&g.to_le_bytes());
    d[12..14].copy_from_slice(&b.to_le_bytes());
    d[14..16].copy_from_slice(&a.to_le_bytes());
    d
}

/// Pack an HDR void extent: all-ones extent coordinates, half-float components.
pub fn pack_void_extent_hdr(r: u16, g: u16, b: u16, a: u16) -> [u8; 16] {
    let mut d = [0xFFu8; 16];
    d[0] = 0b1111_1100;
    d[8..10].copy_from_slice(&r.to_le_bytes());
    d[10..12].copy_from_slice(&g.to_le_bytes());
    d[12..14].copy_from_slice(&b.to_le_bytes());
    d[14..16].copy_from_slice(&a.to_le_bytes());
    d
}

/// Pack a logical block into the 16-byte physical block. `None` for anything
/// the format cannot express (illegal encodings, an endpoint ISE range that
/// does not exactly fill the leftover bits, grid dims with no config row).
/// Solid-color flags take the void-extent paths.
pub fn pack_astc_block(log: &LogAstcBlock) -> Option<[u8; 16]> {
    if log.solid_color_flag_ldr {
        return Some(pack_void_extent_ldr(
            log.solid_color[0],
            log.solid_color[1],
            log.solid_color[2],
            log.solid_color[3],
        ));
    }
    if log.solid_color_flag_hdr {
        return Some(pack_void_extent_hdr(
            log.solid_color[0],
            log.solid_color[1],
            log.solid_color[2],
            log.solid_color[3],
        ));
    }

    if log.num_partitions < 1 || log.num_partitions > MAX_PARTITIONS {
        return None;
    }
    if log.weight_ise_range > LAST_VALID_WEIGHT_ISE_RANGE {
        return None;
    }
    // See 23.24 Illegal Encodings, [0,5] is the minimum ISE encoding for
    // endpoints.
    if log.endpoint_ise_range < FIRST_VALID_ENDPOINT_ISE_RANGE
        || log.endpoint_ise_range > LAST_VALID_ENDPOINT_ISE_RANGE
    {
        return None;
    }
    if log.color_component_selector > 3 {
        return None;
    }

    let config_bits = get_config_bits(log)?;

    let mut phys = [0u8; 16];
    let mut bit_pos: i32 = 0;
    set_bits(&mut phys, &mut bit_pos, config_bits, 11);

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

    let mut total_extra_bits: u32 = 0;

    set_bits(&mut phys, &mut bit_pos, log.num_partitions - 1, 2);

    if log.num_partitions > 1 {
        if log.partition_id >= NUM_PARTITION_PATTERNS {
            return None;
        }
        set_bits(&mut phys, &mut bit_pos, log.partition_id, 10);

        let mut highest_cem = 0u32;
        let mut lowest_cem = u32::MAX;
        for j in 0..log.num_partitions as usize {
            highest_cem = highest_cem.max(log.color_endpoint_modes[j] as u32);
            lowest_cem = lowest_cem.min(log.color_endpoint_modes[j] as u32);
        }
        if highest_cem > 15 {
            return None;
        }
        // The multi-partition CEM encoding spans at most two adjacent classes
        // (cem >> 2), so reject a wider spread.
        if (highest_cem >> 2) > 1 + (lowest_cem >> 2) {
            return None;
        }

        // See tables 79/80.
        let mut encoded_cem = (log.color_endpoint_modes[0] as u32) << 2;
        if lowest_cem != highest_cem {
            encoded_cem = 3.min(1 + (lowest_cem >> 2));
            // See the tables at 23.11 Color Endpoint Mode.
            for j in 0..log.num_partitions as usize {
                let m = (log.color_endpoint_modes[j] as u32) & 3;
                let c =
                    ((log.color_endpoint_modes[j] as i32) >> 2) - ((encoded_cem & 3) as i32 - 1);
                if (c & 1) != c {
                    return None;
                }
                encoded_cem |=
                    ((c as u32) << (2 + j)) | (m << (2 + log.num_partitions as usize + 2 * j));
            }

            total_extra_bits = 3 * log.num_partitions - 4;

            if total_weight_bits as u32 + total_extra_bits > 128 {
                return None;
            }
            let mut cem_bit_pos = (128 - total_weight_bits as u32 - total_extra_bits) as i32;
            set_bits(
                &mut phys,
                &mut cem_bit_pos,
                encoded_cem >> 6,
                total_extra_bits,
            );
        }

        set_bits(&mut phys, &mut bit_pos, encoded_cem & 0x3F, 6);
    } else {
        if log.partition_id != 0 {
            return None;
        }
        if log.color_endpoint_modes[0] > 15 {
            return None;
        }
        set_bits(
            &mut phys,
            &mut bit_pos,
            log.color_endpoint_modes[0] as u32,
            4,
        );
    }

    if log.dual_plane {
        if log.num_partitions > 3 {
            return None;
        }
        total_extra_bits += 2;
        let mut ccs_bit_pos = 128 - total_weight_bits - total_extra_bits as i32;
        if ccs_bit_pos < 0 {
            return None;
        }
        set_bits(&mut phys, &mut ccs_bit_pos, log.color_component_selector, 2);
    }

    let total_config_bits = bit_pos as u32 + total_extra_bits;
    let num_remaining_bits = 128 - total_config_bits as i32 - total_weight_bits;
    if num_remaining_bits < 0 {
        return None;
    }

    let mut total_cem_vals = 0u32;
    for j in 0..log.num_partitions as usize {
        total_cem_vals += 2 + 2 * ((log.color_endpoint_modes[j] as u32) >> 2);
    }
    if total_cem_vals as usize > MAX_ENDPOINTS {
        return None;
    }

    let mut endpoint_ise_range: i32 = -1;
    for k in (1..=20i32).rev() {
        let bits = ise_sequence_bits(total_cem_vals as i32, k as u32);
        if bits <= num_remaining_bits {
            endpoint_ise_range = k;
            break;
        }
    }
    // See 23.24 Illegal Encodings, [0,5] is the minimum ISE encoding for
    // endpoints.
    if endpoint_ise_range < FIRST_VALID_ENDPOINT_ISE_RANGE as i32 {
        return None;
    }
    // The caller must have used exactly the range the layout implies.
    if log.endpoint_ise_range as i32 != endpoint_ise_range {
        return None;
    }

    // Pack endpoints forward.
    pack_bise_tab(
        &mut phys,
        &log.endpoints,
        bit_pos,
        total_cem_vals as i32,
        endpoint_ise_range as usize,
        &helpers_quint_encode(),
    );

    // Pack weights backward: BISE into a zeroed buffer from bit 0, then the
    // whole 128 bits reversed and OR'd in (output byte i gets the bit-reversed
    // input byte 15 - i).
    let mut weight_data = [0u8; 16];
    pack_bise_tab(
        &mut weight_data,
        &log.weights,
        0,
        total_grid_weights as i32,
        log.weight_ise_range as usize,
        &helpers_quint_encode(),
    );
    for i in 0..16 {
        phys[i] |= weight_data[15 - i].reverse_bits();
    }

    Some(phys)
}
