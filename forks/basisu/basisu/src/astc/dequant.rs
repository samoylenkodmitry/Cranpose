//! BISE symbol dequantization for the ASTC HDR decode: endpoint symbols to
//! [0,255] and weight symbols to [0,64], per Table 103 of the ASTC
//! specification, plus the lazily built lookup tables the block decoder
//! indexes. Three directions are provided: symbol-to-value (used by the block
//! decoder) and, for the UASTC HDR 6x6 intermediate codec's requantizers, the
//! value-to-symbol and symbol-rank directions.

use crate::once::OnceBox;
use crate::uastc::tables::ASTC_BISE_RANGE_TABLE;
use alloc::boxed::Box;

/// Replicate `num_src_bits` of `src` across `num_dst_bits`.
fn bit_replication_scale(src: u32, num_src_bits: i32, num_dst_bits: i32) -> u32 {
    debug_assert!(num_src_bits <= num_dst_bits);
    let mut dst = 0;
    let mut shift = num_dst_bits - num_src_bits;
    while shift > -num_src_bits {
        dst |= if shift >= 0 {
            src << shift
        } else {
            src >> -shift
        };
        shift -= num_src_bits;
    }
    dst
}

/// Dequantize an endpoint ISE symbol of `ise_range` (4..=20) to [0,255]
/// (`dequant_bise_endpoint`). Pure-bit ranges bit-replicate; trit/quint ranges
/// follow the Table 103 A/B/C/D construction.
pub fn dequant_bise_endpoint(val: u32, ise_range: u32) -> u32 {
    match ise_range {
        5 => bit_replication_scale(val, 3, 8),
        8 => bit_replication_scale(val, 4, 8),
        11 => bit_replication_scale(val, 5, 8),
        14 => bit_replication_scale(val, 6, 8),
        17 => bit_replication_scale(val, 7, 8),
        20 => val,
        _ => {
            let t = &ASTC_BISE_RANGE_TABLE[ise_range as usize];
            let num_bits = t[0] as u32;
            let num_quints = t[2] as u32;

            // Table 103 row index.
            let range_index = (num_bits * 2 + num_quints) as usize - 2;

            let bits = val & ((1 << num_bits) - 1);
            let tval = val >> num_bits;

            let b = (bits >> 1) & 1;
            let c = (bits >> 2) & 1;
            let d = (bits >> 3) & 1;
            let e = (bits >> 4) & 1;
            let f = (bits >> 5) & 1;

            let a = if bits & 1 != 0 { 511 } else { 0 };
            // The scattered bit-replication pattern B per Table 103 row; the
            // comments give the 9-bit source layout (bit 8 first).
            let bb = match range_index {
                2 => (b << 1) | (b << 2) | (b << 4) | (b << 8), // b000b0bb0
                3 => (b << 2) | (b << 3) | (b << 8),            // b0000bb00
                4 => b | (c << 1) | (b << 2) | (c << 3) | (b << 7) | (c << 8), // cb000cbcb
                5 => c | (b << 1) | (c << 2) | (b << 7) | (c << 8), // cb0000cbc
                6 => b | (c << 1) | (d << 2) | (b << 6) | (c << 7) | (d << 8), // dcb000dcb
                7 => c | (d << 1) | (b << 6) | (c << 7) | (d << 8), // dcb0000dc
                8 => d | (e << 1) | (b << 5) | (c << 6) | (d << 7) | (e << 8), // edcb000ed
                9 => e | (b << 5) | (c << 6) | (d << 7) | (e << 8), // edcb0000e
                10 => f | (b << 4) | (c << 5) | (d << 6) | (e << 7) | (f << 8), // fedcb000f
                _ => 0,
            };

            const C_VALS: [u32; 11] = [204, 113, 93, 54, 44, 26, 22, 13, 11, 6, 5];
            let mut u = tval * C_VALS[range_index] + bb;
            u ^= a;
            (a & 0x80) | (u >> 2)
        }
    }
}

/// Dequantize a weight ISE symbol of `ise_range` (0..=11) to [0,64]
/// (`dequant_bise_weight`). Values above the midpoint get the +1 nudge so the
/// range ends exactly at 64.
pub fn dequant_bise_weight(val: u32, ise_range: u32) -> u32 {
    let mut u = match ise_range {
        0 => {
            if val != 0 {
                63
            } else {
                0
            }
        }
        1 => [0u32, 32, 63][val as usize],
        2 => bit_replication_scale(val, 2, 6),
        3 => [0u32, 16, 32, 47, 63][val as usize],
        5 => bit_replication_scale(val, 3, 6),
        8 => bit_replication_scale(val, 4, 6),
        11 => bit_replication_scale(val, 5, 6),
        _ => {
            let t = &ASTC_BISE_RANGE_TABLE[ise_range as usize];
            let num_bits = t[0] as u32;
            let num_quints = t[2] as u32;

            // Table 103 row index (weight table).
            let range_index = (num_bits * 2 + num_quints) as usize;

            let bits = val & ((1 << num_bits) - 1);
            let d = val >> num_bits;

            const C_TABLE: [u32; 5] = [50, 28, 23, 13, 11];

            let b = (bits >> 1) & 1;
            let c = (bits >> 2) & 1;

            let a = if bits & 1 == 0 { 0 } else { 0x7F };
            let bb = match range_index {
                4 => (b << 6) | (b << 2) | b,
                5 => (b << 6) | (b << 1),
                6 => (c << 6) | (b << 5) | (c << 1) | b,
                _ => 0,
            };

            let mut u = d * C_TABLE[range_index - 2] + bb;
            u ^= a;
            (a & 0x20) | (u >> 2)
        }
    };
    if u > 32 {
        u += 1;
    }
    u
}

/// The symbol-to-value dequantization tables the block decoder uses: one row
/// per endpoint range 4..=20 (index `range - 4`) and one per weight range
/// 0..=11, filled for that range's level count and zero beyond it.
pub struct DequantTables {
    /// Endpoint ISE symbol to [0,255], `[range - 4][symbol]`.
    pub endpoints: [[u8; 256]; 17],
    /// Weight ISE symbol to [0,64], `[range][symbol]`.
    pub weights: [[u8; 32]; 12],
}

/// Levels representable by a BISE range (`get_ise_levels`), local copy to keep
/// this module free of the unpack module.
fn levels(range: u32) -> u32 {
    let t = &ASTC_BISE_RANGE_TABLE[range as usize];
    ((1 + 2 * t[1] + 4 * t[2]) << t[0]) as u32
}

/// The dequantization tables, built on first use and cached. Filling every
/// symbol with its direct dequantization is correct because dequantization is
/// injective per range (proven by the unit test below), so each symbol is the
/// unique nearest symbol of its own dequantized value.
pub fn dequant_tables() -> &'static DequantTables {
    static TABLES: OnceBox<DequantTables> = OnceBox::new();
    TABLES.get_or_init(|| {
        let mut t = Box::new(DequantTables {
            endpoints: [[0; 256]; 17],
            weights: [[0; 32]; 12],
        });
        for range in 4..=20u32 {
            for sym in 0..levels(range) {
                t.endpoints[(range - 4) as usize][sym as usize] =
                    dequant_bise_endpoint(sym, range) as u8;
            }
        }
        for range in 0..=11u32 {
            for sym in 0..levels(range) {
                t.weights[range as usize][sym as usize] = dequant_bise_weight(sym, range) as u8;
            }
        }
        t
    })
}

/// The quantization (value-to-symbol) and rank tables the 6x6 intermediate
/// codec's requantizers use, for every endpoint range 4..=20 and weight range
/// 0..=11. `val_to_ise` maps a [0,255] (or [0,64]) value to the nearest ISE
/// symbol (ties resolved to the lowest symbol via a linear scan); the rank
/// tables order a range's symbols by dequantized value.
pub struct QuantTables {
    /// Endpoint value [0,255] to nearest ISE symbol, `[range - 4][value]`.
    pub endpoint_val_to_ise: [[u8; 256]; 17],
    /// Endpoint ISE symbol to its dequantized-value rank, `[range - 4][symbol]`.
    pub endpoint_ise_to_rank: [[u8; 256]; 17],
    /// Endpoint rank to ISE symbol (inverse of the above).
    pub endpoint_rank_to_ise: [[u8; 256]; 17],
    /// Weight value [0,64] to nearest ISE symbol, `[range][value]`.
    pub weight_val_to_ise: [[u8; 65]; 12],
    /// Weight ISE symbol to its dequantized-value rank, `[range][symbol]`.
    pub weight_ise_to_rank: [[u8; 32]; 12],
    /// Weight rank to ISE symbol (inverse of the above).
    pub weight_rank_to_ise: [[u8; 32]; 12],
}

/// Linear scan for the lowest symbol whose dequantization is nearest `v`, with
/// an early-out on an exact match. `weight_flag` selects the weight
/// dequantizer over the endpoint one.
fn find_nearest(v: i32, range: u32, weight_flag: bool) -> u32 {
    let n = levels(range);
    let mut best_e = i32::MAX;
    let mut best_index = 0;
    for i in 0..n {
        let qv = if weight_flag {
            dequant_bise_weight(i, range)
        } else {
            dequant_bise_endpoint(i, range)
        } as i32;
        let e = (v - qv).abs();
        if e < best_e {
            best_e = e;
            best_index = i;
            if best_e == 0 {
                break;
            }
        }
    }
    best_index
}

/// The quantization tables, built on first use and cached. The rank tables are
/// identity for pure-bit ranges, and otherwise the symbols sorted by
/// `(dequantized_value << 16) | symbol` (unique keys, so the sort order is
/// total).
pub fn quant_tables() -> &'static QuantTables {
    static TABLES: OnceBox<QuantTables> = OnceBox::new();
    TABLES.get_or_init(|| {
        let mut t = Box::new(QuantTables {
            endpoint_val_to_ise: [[0; 256]; 17],
            endpoint_ise_to_rank: [[0; 256]; 17],
            endpoint_rank_to_ise: [[0; 256]; 17],
            weight_val_to_ise: [[0; 65]; 12],
            weight_ise_to_rank: [[0; 32]; 12],
            weight_rank_to_ise: [[0; 32]; 12],
        });
        for range in 4..=20u32 {
            let r = (range - 4) as usize;
            for v in 0..256 {
                t.endpoint_val_to_ise[r][v as usize] = find_nearest(v, range, false) as u8;
            }
            let n = levels(range);
            let tab = &ASTC_BISE_RANGE_TABLE[range as usize];
            if tab[1] == 0 && tab[2] == 0 {
                for i in 0..n {
                    t.endpoint_ise_to_rank[r][i as usize] = i as u8;
                    t.endpoint_rank_to_ise[r][i as usize] = i as u8;
                }
            } else {
                let mut vals: alloc::vec::Vec<u32> = (0..n)
                    .map(|i| (dequant_bise_endpoint(i, range) << 16) | i)
                    .collect();
                vals.sort_unstable();
                for (rank, key) in vals.iter().enumerate() {
                    let ise = (key & 0xFF) as usize;
                    t.endpoint_ise_to_rank[r][ise] = rank as u8;
                    t.endpoint_rank_to_ise[r][rank] = ise as u8;
                }
            }
        }
        for range in 0..=11u32 {
            for v in 0..=64 {
                t.weight_val_to_ise[range as usize][v as usize] =
                    find_nearest(v, range, true) as u8;
            }
            let n = levels(range);
            let tab = &ASTC_BISE_RANGE_TABLE[range as usize];
            if tab[1] == 0 && tab[2] == 0 {
                for i in 0..n {
                    t.weight_ise_to_rank[range as usize][i as usize] = i as u8;
                    t.weight_rank_to_ise[range as usize][i as usize] = i as u8;
                }
            } else {
                let mut vals: alloc::vec::Vec<u32> = (0..n)
                    .map(|i| (dequant_bise_weight(i, range) << 16) | i)
                    .collect();
                vals.sort_unstable();
                for (rank, key) in vals.iter().enumerate() {
                    let ise = (key & 0xFF) as usize;
                    t.weight_ise_to_rank[range as usize][ise] = rank as u8;
                    t.weight_rank_to_ise[range as usize][rank] = ise as u8;
                }
            }
        }
        t
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Injectivity underpins the direct-fill table construction above: if two
    // symbols shared a dequantized value, a nearest-symbol fill would leave one
    // entry unwritten (zero) where the direct fill writes the value.
    #[test]
    fn endpoint_dequant_is_injective_per_range() {
        for range in 4..=20u32 {
            let n = levels(range);
            let mut seen = [false; 256];
            for sym in 0..n {
                let v = dequant_bise_endpoint(sym, range) as usize;
                assert!(!seen[v], "range {range} symbol {sym} duplicates value {v}");
                seen[v] = true;
            }
        }
    }

    #[test]
    fn weight_dequant_is_injective_per_range() {
        for range in 0..=11u32 {
            let n = levels(range);
            let mut seen = [false; 65];
            for sym in 0..n {
                let v = dequant_bise_weight(sym, range) as usize;
                assert!(v <= 64);
                assert!(!seen[v], "range {range} symbol {sym} duplicates value {v}");
                seen[v] = true;
            }
        }
    }

    // The UASTC LDR endpoint unquantizer implements the same Table 103 math;
    // cross-check the two implementations over every endpoint range and level.
    #[test]
    fn endpoint_dequant_matches_uastc_bise() {
        for range in 4..=20u32 {
            if !crate::uastc::bise::astc_is_valid_endpoint_range(range) {
                continue;
            }
            for sym in 0..levels(range) {
                assert_eq!(
                    dequant_bise_endpoint(sym, range),
                    crate::uastc::bise::unquant_astc_endpoint_val(sym, range),
                    "range {range} symbol {sym}"
                );
            }
        }
    }
}
