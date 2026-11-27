//! ASTC BISE (Bounded Integer Sequence Encoding) endpoint unquantization:
//! per-range level counts and validity, single-endpoint unquantization, and
//! the lazily built `[21][256]` unquantization table.

use super::tables::ASTC_BISE_RANGE_TABLE;
use crate::once::OnceBox;
use alloc::boxed::Box;

/// One BISE range's unquantization parameters: the 9-char B bit-source string
/// and the multiplier C, from the ASTC spec's endpoint unquantization tables.
struct UnquantParams {
    /// The B bit-source characters. Ranges with no B bits store all `'0'` so
    /// the `!= '0'` test in [`unquant_astc_endpoint`] short-circuits (a zero
    /// byte would underflow the `c - 'a'` shift).
    pb: [u8; 9],
    /// The C multiplier applied to the trit or quint value during
    /// unquantization. Zero marks a range that carries no trit/quint component.
    c: u32,
}

/// Build one `PARAMS` row from its bit-source string and multiplier.
const fn up(pb: &[u8; 9], c: u32) -> UnquantParams {
    UnquantParams { pb: *pb, c }
}

/// Per-range B/C unquantization parameters, indexed by BISE range. The trailing
/// comments give the range index and, for trit/quint ranges, the level range.
static PARAMS: [UnquantParams; 21] = [
    up(b"000000000", 0),   // 0
    up(b"000000000", 0),   // 1
    up(b"000000000", 0),   // 2
    up(b"000000000", 0),   // 3
    up(b"000000000", 204), // 4   0-5
    up(b"000000000", 0),   // 5
    up(b"000000000", 113), // 6   0-9
    up(b"b000b0bb0", 93),  // 7   0-11
    up(b"000000000", 0),   // 8
    up(b"b0000bb00", 54),  // 9   0-19
    up(b"cb000cbcb", 44),  // 10  0-23
    up(b"000000000", 0),   // 11
    up(b"cb0000cbc", 26),  // 12  0-39
    up(b"dcb000dcb", 22),  // 13  0-47
    up(b"000000000", 0),   // 14
    up(b"dcb0000dc", 13),  // 15  0-79
    up(b"edcb000ed", 11),  // 16  0-95
    up(b"000000000", 0),   // 17
    up(b"edcb0000e", 6),   // 18  0-159
    up(b"fedcb000f", 5),   // 19  0-191
    up(b"000000000", 0),   // 20
];

/// Number of representable levels for a BISE range.
pub fn astc_get_levels(range: u32) -> u32 {
    let t = &ASTC_BISE_RANGE_TABLE[range as usize];
    ((1 + 2 * t[1] + 4 * t[2]) << t[0]) as u32
}

/// Whether `range` is usable for endpoints: pure-bit ranges always are, while
/// trit/quint ranges need a nonzero C multiplier.
pub fn astc_is_valid_endpoint_range(range: u32) -> bool {
    let t = &ASTC_BISE_RANGE_TABLE[range as usize];
    if t[1] == 0 && t[2] == 0 {
        return true;
    }
    PARAMS[range as usize].c != 0
}

/// Unquantize a BISE-coded endpoint from its split (bits, trit, quint) parts
/// to an 8-bit value.
pub fn unquant_astc_endpoint(
    packed_bits: u32,
    packed_trits: u32,
    packed_quints: u32,
    range: u32,
) -> u32 {
    let t = &ASTC_BISE_RANGE_TABLE[range as usize];
    let bits = t[0] as u32;
    let trits = t[1] as u32;
    let quints = t[2] as u32;

    let mut val = 0u32;
    if trits == 0 && quints == 0 {
        // Pure bits: replicate the field across 8 bits.
        let mut bits_left = 8i32;
        while bits_left > 0 {
            let mut v = packed_bits;
            let n = bits_left.min(bits as i32);
            if n < bits as i32 {
                v >>= bits as i32 - n;
            }
            val |= v << (bits_left - n);
            bits_left -= n;
        }
    } else {
        // Trit/quint path per the ASTC spec: A replicates bit 0 of the packed
        // bits, D is the trit or quint value, and B gathers scattered copies
        // of the remaining bits as directed by the range's bit-source string
        // (each letter names the source bit: 'a' = bit 0, 'b' = bit 1, ...).
        let a = if packed_bits & 1 != 0 { 511 } else { 0 };
        let c = PARAMS[range as usize].c;
        let d = if trits != 0 {
            packed_trits
        } else {
            packed_quints
        };

        let mut b = 0u32;
        for i in 0..9 {
            b <<= 1;
            let ch = PARAMS[range as usize].pb[i];
            if ch != b'0' {
                let shift = (ch - b'a') as u32;
                b |= (packed_bits >> shift) & 1;
            }
        }

        // D*C + B, xor with the replicated low bit, then fold the 9-bit value
        // down to 8 bits keeping A's top bit.
        val = d.wrapping_mul(c).wrapping_add(b);
        val ^= a;
        val = (a & 0x80) | (val >> 2);
    }
    val
}

/// Unquantize a packed endpoint value, splitting it into bits + trit/quint
/// parts as the range dictates.
pub fn unquant_astc_endpoint_val(packed_val: u32, range: u32) -> u32 {
    let t = &ASTC_BISE_RANGE_TABLE[range as usize];
    let bits = t[0] as u32;
    let trits = t[1];
    let quints = t[2];
    if trits == 0 && quints == 0 {
        unquant_astc_endpoint(packed_val, 0, 0, range)
    } else if trits != 0 {
        unquant_astc_endpoint(packed_val & ((1 << bits) - 1), packed_val >> bits, 0, range)
    } else {
        unquant_astc_endpoint(packed_val & ((1 << bits) - 1), 0, packed_val >> bits, range)
    }
}

/// The endpoint unquantization table, built on first use and cached.
pub fn astc_unquant() -> &'static [[AstcQuantBin; 256]; TOTAL_ASTC_RANGES] {
    static TABLE: OnceBox<[[AstcQuantBin; 256]; TOTAL_ASTC_RANGES]> = OnceBox::new();
    TABLE.get_or_init(build_astc_unquant)
}

/// One unquantization table entry. 2 bytes, no padding, so the table has a
/// stable byte layout.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AstcQuantBin {
    /// The unquantized 8-bit endpoint value.
    pub m_unquant: u8,
    /// Rank of this encoded index once the range's levels are sorted by
    /// unquantized value.
    pub m_index: u8,
}

/// Total BISE ranges covered by the unquantization tables.
pub const TOTAL_ASTC_RANGES: usize = 21;

/// Build the `[21][256]` unquantization table: for each valid range,
/// unquantize each level, sort by `(unquant << 8) | index`, then invert into
/// `[encoded_index] -> {unquant, sorted_rank}`. Invalid ranges and unused
/// indices stay zero.
pub fn build_astc_unquant() -> Box<[[AstcQuantBin; 256]; TOTAL_ASTC_RANGES]> {
    let mut table = Box::new([[AstcQuantBin::default(); 256]; TOTAL_ASTC_RANGES]);
    for range in 0..TOTAL_ASTC_RANGES as u32 {
        if !astc_is_valid_endpoint_range(range) {
            continue;
        }
        let levels = astc_get_levels(range) as usize;
        let mut vals = [0u32; 256];
        for (i, v) in vals.iter_mut().enumerate().take(levels) {
            *v = (unquant_astc_endpoint_val(i as u32, range) << 8) | i as u32;
        }
        vals[0..levels].sort_unstable();
        for (i, &packed) in vals.iter().enumerate().take(levels) {
            let order = (packed & 0xFF) as usize;
            let unq = (packed >> 8) as u8;
            table[range as usize][order].m_unquant = unq;
            table[range as usize][order].m_index = i as u8;
        }
    }
    table
}
