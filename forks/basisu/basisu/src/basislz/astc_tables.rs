//! ETC1S-to-ASTC lookup tables built once at runtime. These are the shared
//! prerequisite for the ETC1S-to-ASTC 4x4 conversion: the ISE unquant table,
//! the selector range index, the best-grayscale-mapping tables (derived from
//! the `G_ETC1_TO_ASTC[_0_255]` solution tables), and the single-color
//! encodings. The selector ranges and mappings double as inputs to the BC1 and
//! BC7 emits.

use crate::once::OnceBox;
use crate::tables::astc::G_ETC1_TO_ASTC;
use crate::tables::astc_0_255::G_ETC1_TO_ASTC_0_255;
use crate::tables::solution::Etc1ToSolution;
use alloc::boxed::Box;

/// The six `{low, high}` selector ranges the ASTC, BC7 and BC1 emits index by.
pub static SELECTOR_RANGES: [[u8; 2]; 6] = [[0, 3], [1, 3], [0, 2], [1, 2], [2, 3], [0, 1]];
/// Number of selector low/high ranges in `SELECTOR_RANGES`.
pub const NUM_SELECTOR_RANGES: usize = 6;
/// Number of 4-entry selector remap candidates in `SELECTOR_MAPPINGS`.
pub const NUM_SELECTOR_MAPPINGS: usize = 10;

/// Each of the ten rows is a candidate
/// reassignment of the four ETC1S selectors to output selector values in
/// linear order. Shared by the ASTC, BC7 and BC1 emits (BC1 composes these
/// with its own selector-order remap).
pub static SELECTOR_MAPPINGS: [[u8; 4]; NUM_SELECTOR_MAPPINGS] = [
    [0, 0, 1, 1],
    [0, 0, 1, 2],
    [0, 0, 1, 3],
    [0, 0, 2, 3],
    [0, 1, 1, 1],
    [0, 1, 2, 2],
    [0, 1, 2, 3],
    [0, 2, 3, 3],
    [1, 2, 2, 2],
    [1, 2, 3, 3],
];

/// ASTC endpoint dequantization for the one-trit,
/// four-bit quant level (3 trit values times 16 bit patterns). `a`/`b`/`c`/`d` are the
/// ASTC spec's bounded-integer-sequence unquantization terms; the final
/// `(a & 0x80) | (unq >> 2)` is the spec's bit replication that spreads the
/// result back across the 8-bit range.
pub fn build_ise_to_unquant() -> [u32; 48] {
    let mut t = [0u32; 48];
    for trit in 0..3u32 {
        for bit in 0..16u32 {
            let a = if bit & 1 != 0 { 511 } else { 0 };
            let b = (bit >> 1) | ((bit >> 1) << 6);
            let c = 22u32;
            let d = trit;
            let mut unq = d * c + b;
            unq ^= a;
            unq = (a & 0x80) | (unq >> 2);
            t[(bit | (trit << 4)) as usize] = unq;
        }
    }
    t
}

/// The `[4][4]` selector range index.
pub fn build_selector_range_index() -> [[u32; 4]; 4] {
    let mut t = [[0u32; 4]; 4];
    for (i, r) in SELECTOR_RANGES.iter().enumerate() {
        t[r[0] as usize][r[1] as usize] = i as u32;
    }
    t
}

/// The `[32][8][6]` best-grayscale-mapping: per (base, inten, range),
/// the mapping with lowest error in the given solution table.
fn build_best_grayscale_mapping(
    table: &[Etc1ToSolution],
) -> Box<[[[u8; NUM_SELECTOR_RANGES]; 8]; 32]> {
    let mut out = Box::new([[[0u8; NUM_SELECTOR_RANGES]; 8]; 32]);
    for base in 0..32usize {
        for inten in 0..8usize {
            for range in 0..NUM_SELECTOR_RANGES {
                let base_idx = (inten * 32 + base) * (NUM_SELECTOR_RANGES * NUM_SELECTOR_MAPPINGS)
                    + range * NUM_SELECTOR_MAPPINGS;
                let mut best_mapping = 0u8;
                let mut best_err = u32::MAX;
                for m in 0..NUM_SELECTOR_MAPPINGS {
                    let err = table[base_idx + m].m_err as u32;
                    if err < best_err {
                        best_err = err;
                        best_mapping = m as u8;
                    }
                }
                out[base][inten][range] = best_mapping;
            }
        }
    }
    out
}

/// Best-mapping table for the `[0, 47]` ASTC value range, from `G_ETC1_TO_ASTC`.
pub fn build_best_grayscale_mapping_47() -> Box<[[[u8; NUM_SELECTOR_RANGES]; 8]; 32]> {
    build_best_grayscale_mapping(&G_ETC1_TO_ASTC)
}
/// Best-mapping table for the full `[0, 255]` range, from `G_ETC1_TO_ASTC_0_255`.
pub fn build_best_grayscale_mapping_0_255() -> Box<[[[u8; NUM_SELECTOR_RANGES]; 8]; 32]> {
    build_best_grayscale_mapping(&G_ETC1_TO_ASTC_0_255)
}

/// For each of the 256 target grayscale values, the `{lo, hi}` ISE endpoint
/// pair (2 bytes per entry) whose interpolated decode lands closest to it.
pub fn build_single_color_encoding_1(ise: &[u32; 48]) -> [[u8; 2]; 256] {
    let mut t = [[0u8; 2]; 256];
    for (i, e) in t.iter_mut().enumerate() {
        let mut lowest_e = i32::MAX;
        for (lo, &lo_u) in ise.iter().enumerate() {
            let lo_v = lo_u as i32;
            for (hi, &hi_u) in ise.iter().enumerate() {
                let hi_v = hi_u as i32;
                // Decode the pair the way ASTC does: widen each endpoint to 16
                // bits (v | v<<8), blend with the fixed single-weight value
                // 21/64, round (+32), then take the high byte.
                let l = lo_v | (lo_v << 8);
                let h = hi_v | (hi_v << 8);
                let v = ((l * (64 - 21) + h * 21 + 32) / 64) >> 8;
                let err = (v - i as i32).abs();
                if err < lowest_e {
                    e[0] = lo as u8; // m_lo
                    e[1] = hi as u8; // m_hi
                    lowest_e = err;
                }
            }
        }
    }
    t
}

/// Cached selector range index.
pub fn selector_range_index() -> &'static [[u32; 4]; 4] {
    static T: OnceBox<[[u32; 4]; 4]> = OnceBox::new();
    T.get_or_init(|| Box::new(build_selector_range_index()))
}

/// Cached best-grayscale-mapping for the full `[0, 255]` range.
pub fn best_grayscale_mapping_0_255() -> &'static [[[u8; NUM_SELECTOR_RANGES]; 8]; 32] {
    static T: OnceBox<[[[u8; NUM_SELECTOR_RANGES]; 8]; 32]> = OnceBox::new();
    T.get_or_init(build_best_grayscale_mapping_0_255)
}

/// Cached best-grayscale-mapping (the 0..=47 variant).
pub fn best_grayscale_mapping_47() -> &'static [[[u8; NUM_SELECTOR_RANGES]; 8]; 32] {
    static T: OnceBox<[[[u8; NUM_SELECTOR_RANGES]; 8]; 32]> = OnceBox::new();
    T.get_or_init(build_best_grayscale_mapping_47)
}

/// Cached ISE-to-unquant table, `[48]`.
pub fn ise_to_unquant() -> &'static [u32; 48] {
    static T: OnceBox<[u32; 48]> = OnceBox::new();
    T.get_or_init(|| Box::new(build_ise_to_unquant()))
}

/// Cached single-color encoding 0, `[256]`.
pub fn single_color_encoding_0() -> &'static [u8; 256] {
    static T: OnceBox<[u8; 256]> = OnceBox::new();
    T.get_or_init(|| Box::new(build_single_color_encoding_0(ise_to_unquant())))
}

/// Cached single-color encoding 1, `[256]` (`[lo, hi]` per entry).
pub fn single_color_encoding_1() -> &'static [[u8; 2]; 256] {
    static T: OnceBox<[[u8; 2]; 256]> = OnceBox::new();
    T.get_or_init(|| Box::new(build_single_color_encoding_1(ise_to_unquant())))
}

/// For each target grayscale value, the
/// single ISE endpoint index whose unquantized value lands closest to it.
pub fn build_single_color_encoding_0(ise: &[u32; 48]) -> [u8; 256] {
    let mut t = [0u8; 256];
    for (i, e) in t.iter_mut().enumerate() {
        let mut lowest_e = i32::MAX;
        for (lo, &lo_u) in ise.iter().enumerate() {
            let err = (lo_u as i32 - i as i32).abs();
            if err < lowest_e {
                *e = lo as u8;
                lowest_e = err;
            }
        }
    }
    t
}
