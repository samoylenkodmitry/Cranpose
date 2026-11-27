//! UASTC to ASTC 4x4 transcode: repack a decoded UASTC block as a physical
//! ASTC block. Includes the BISE (bounded integer sequence encoding) encode
//! path and the LSB-first bit writers it needs; the inverse of the BISE decode
//! in `bise.rs`.

use super::astc_pack_tables::{ASTC_QUINT_ENCODE, ASTC_TRIT_ENCODE};
use super::tables::{ASTC_BISE_RANGE_TABLE, ENDPOINT_RANGES};
use super::unpack::unpack_to_block;
use super::{TOTAL_UASTC_MODES, UASTC_MODE_INDEX_SOLID_COLOR};

/// Width of the leading ASTC block-mode field; the config fields that follow
/// start at this bit offset.
const ASTC_BLOCK_MODE_BITS: i32 = 11;
/// Width of the partition-count field (number of subsets minus one).
const ASTC_PART_BITS: u32 = 2;
/// Width of the color endpoint mode field for the single-subset case.
const ASTC_CEM_BITS: u32 = 4;
/// Width of the partition seed for the multi-subset case.
const ASTC_PARTITION_INDEX_BITS: u32 = 10;
/// Width of the dual-plane color component selector.
const ASTC_CCS_BITS: u32 = 2;

/// Precomputed 11-bit ASTC block-mode field per UASTC mode. Mode 8 (solid
/// color) has none: it packs as a void-extent block instead.
static MODE_ASTC_BLOCK_MODE: [u32; TOTAL_UASTC_MODES] = [
    0x242, 0x42, 0x53, 0x42, 0x42, 0x53, 0x442, 0x42, 0, 0x42, 0x242, 0x442, 0x53, 0x441, 0x42,
    0x242, 0x42, 0x442, 0x253,
];

/// The inclusive `[low, high]` bit range of `bits`, shifted down to bit 0.
#[inline]
fn extract_bits(bits: u32, low: i32, high: i32) -> u32 {
    (bits >> low) & ((1 << (high - low + 1)) - 1)
}

/// Write `total_bits` of `value` at `*bit_pos` (LSB-first), advancing the
/// cursor. ORs into `buf`, so the target bits must still be zero, and the
/// buffer must cover the final bit position.
pub(crate) fn set_bits(buf: &mut [u8], bit_pos: &mut i32, mut value: u32, mut total_bits: u32) {
    while total_bits != 0 {
        let bits_to_write = (total_bits as i32).min(8 - (*bit_pos & 7)) as u32;
        buf[(*bit_pos >> 3) as usize] |= (value << (*bit_pos & 7)) as u8;
        *bit_pos += bits_to_write as i32;
        total_bits -= bits_to_write;
        value >>= bits_to_write;
    }
}

/// Write up to 9 bits of `code` at `*bit_pos`, advancing the cursor. A 9-bit
/// write spans at most two bytes, so this trades the general `set_bits` loop
/// for two fixed byte ORs.
fn set_bits_1_to_9(buf: &mut [u8], bit_pos: &mut i32, code: u32, codesize: u32) {
    debug_assert!(codesize <= 9);
    if codesize != 0 {
        let byte_bit_offset = (*bit_pos & 7) as u32;
        let val = code << byte_bit_offset;
        let index = (*bit_pos >> 3) as usize;
        buf[index] |= val as u8;
        if codesize > (8 - byte_bit_offset) {
            buf[index + 1] |= (val >> 8) as u8;
        }
        *bit_pos += codesize as i32;
    }
}

/// BISE-encode five values from a trit range: each value splits into `n` low
/// bits plus a trit, the five trits pack into one 8-bit code, and that code's
/// bits interleave with the low bits in the trit-block layout the ASTC
/// specification defines.
pub(crate) fn encode_trits(buf: &mut [u8], values: &[u8; 5], bit_pos: &mut i32, n: i32) {
    let mut trits = 0i32;
    let mut bits = [0u32; 5];
    let bit_mask = (1u32 << n) - 1;
    const MULS: [i32; 5] = [1, 3, 9, 27, 81];
    for i in 0..5 {
        let t = (values[i] as i32) >> n;
        trits += t * MULS[i];
        bits[i] = values[i] as u32 & bit_mask;
    }
    let tt = ASTC_TRIT_ENCODE[trits as usize] as u32;
    set_bits(
        buf,
        bit_pos,
        bits[0] | (extract_bits(tt, 0, 1) << n) | (bits[1] << (2 + n)),
        (n * 2 + 2) as u32,
    );
    set_bits(
        buf,
        bit_pos,
        extract_bits(tt, 2, 3)
            | (bits[2] << 2)
            | (extract_bits(tt, 4, 4) << (2 + n))
            | (bits[3] << (3 + n))
            | (extract_bits(tt, 5, 6) << (3 + n * 2))
            | (bits[4] << (5 + n * 2))
            | (extract_bits(tt, 7, 7) << (5 + n * 3)),
        (n * 3 + 6) as u32,
    );
}

/// BISE-encode three values from a quint range: each value splits into `n`
/// low bits plus a quint, the three quints pack into one 7-bit code, and that
/// code's bits interleave with the low bits in the quint-block layout the ASTC
/// specification defines. The quint code table is caller-supplied because two
/// variants are in use that differ only at index 124 (three max quints), one
/// coding 31 and the other 7. Both decode identically, but byte-exact output
/// requires each pack path to use its own variant.
fn encode_quints_tab(buf: &mut [u8], values: &[u8; 5], bit_pos: &mut i32, n: i32, tab: &[u8; 125]) {
    let mut quints = 0i32;
    let mut bits = [0u32; 3];
    let bit_mask = (1u32 << n) - 1;
    const MULS: [i32; 3] = [1, 5, 25];
    for i in 0..3 {
        let t = (values[i] as i32) >> n;
        quints += t * MULS[i];
        bits[i] = values[i] as u32 & bit_mask;
    }
    let tt = tab[quints as usize] as u32;
    set_bits(
        buf,
        bit_pos,
        bits[0]
            | (extract_bits(tt, 0, 2) << n)
            | (bits[1] << (3 + n))
            | (extract_bits(tt, 3, 4) << (3 + n * 2))
            | (bits[2] << (5 + n * 2))
            | (extract_bits(tt, 5, 6) << (5 + n * 3)),
        (7 + n * 3) as u32,
    );
}

/// BISE-pack `num_vals` values of `src` into `dst` starting at `bit_pos`,
/// using the trit, quint, or plain-bits encoding that `range` selects.
pub(crate) fn pack_bise(dst: &mut [u8; 16], src: &[u8], bit_pos: i32, num_vals: i32, range: usize) {
    pack_bise_tab(dst, src, bit_pos, num_vals, range, &ASTC_QUINT_ENCODE);
}

/// `pack_bise` against a caller-chosen quint code table (see
/// `encode_quints_tab` for why two copies exist).
pub(crate) fn pack_bise_tab(
    dst: &mut [u8; 16],
    src: &[u8],
    mut bit_pos: i32,
    num_vals: i32,
    range: usize,
    quint_tab: &[u8; 125],
) {
    // 16 block bytes plus 4 bytes of slack: the last group's write can spill
    // past byte 15, and only the first 16 bytes are merged into dst below.
    let mut temp = [0u8; 20];
    let num_bits = ASTC_BISE_RANGE_TABLE[range][0];
    let group_size = if ASTC_BISE_RANGE_TABLE[range][1] != 0 {
        5
    } else if ASTC_BISE_RANGE_TABLE[range][2] != 0 {
        3
    } else {
        0
    };

    if group_size != 0 {
        let total_groups = if group_size == 5 {
            (num_vals + 4) / 5
        } else {
            (num_vals + 2) / 3
        };
        for g in 0..total_groups {
            let mut vals = [0u8; 5];
            let limit = group_size.min(num_vals - g * group_size);
            for i in 0..limit {
                vals[i as usize] = src[(g * group_size + i) as usize];
            }
            if group_size == 5 {
                encode_trits(&mut temp, &vals, &mut bit_pos, num_bits);
            } else {
                encode_quints_tab(&mut temp, &vals, &mut bit_pos, num_bits, quint_tab);
            }
        }
    } else {
        for i in 0..num_vals {
            set_bits_1_to_9(
                &mut temp,
                &mut bit_pos,
                src[i as usize] as u32,
                num_bits as u32,
            );
        }
    }

    for (d, t) in dst.iter_mut().zip(temp.iter()) {
        *d |= *t;
    }
}

/// Pack a solid color as an LDR void-extent block: a constant-color block
/// whose extent coordinates are all ones (meaning no extent), with each 8-bit
/// channel replicated into the 16 bits of its void-extent color field.
fn pack_astc_solid_block(color: crate::color::Color32) -> [u8; 16] {
    let (r, g, b, a) = (
        color[0] as u32,
        color[1] as u32,
        color[2] as u32,
        color[3] as u32,
    );
    let mut out = [0u8; 16];
    out[0] = 0xfc;
    out[1] = 0xfd;
    out[2] = 0xff;
    out[3] = 0xff;
    out[4..8].copy_from_slice(&0xffffffffu32.to_le_bytes());
    // bytes 8..16 stay zero; the void-extent color is written into them next.
    let mut bit_pos = 64i32;
    set_bits(&mut out, &mut bit_pos, r | (r << 8), 16);
    set_bits(&mut out, &mut bit_pos, g | (g << 8), 16);
    set_bits(&mut out, &mut bit_pos, b | (b << 8), 16);
    set_bits(&mut out, &mut bit_pos, a | (a << 8), 16);
    out
}

// ASTC stores weights with their bits reversed. These tables map a weight
// value to its bit-reversed form so it can be written directly: REVn reverses
// the low n bits of an index in [0, 2^n).

/// 2-bit reversal table.
pub(crate) const REV2: [u8; 4] = [0, 2, 1, 3];
/// 3-bit reversal table.
const REV3: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
/// 4-bit reversal table.
const REV4: [u8; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
/// 5-bit reversal table.
const REV5: [u8; 32] = [
    0, 16, 8, 24, 4, 20, 12, 28, 2, 18, 10, 26, 6, 22, 14, 30, 1, 17, 9, 25, 5, 21, 13, 29, 3, 19,
    11, 27, 7, 23, 15, 31,
];

/// Pack a decoded ASTC block description into the 16-byte physical block.
fn pack_astc_block(block: &super::unpack::AstcBlockDesc, uastc_mode: u32) -> [u8; 16] {
    let mut out = [0u8; 16];
    let total_weights = if block.dual_plane { 32 } else { 16 };

    let mode = MODE_ASTC_BLOCK_MODE[uastc_mode as usize];
    out[0] = mode as u8;
    out[1] = (mode >> 8) as u8;

    let mut bit_pos = ASTC_BLOCK_MODE_BITS;
    let bits_per_weight = ASTC_BISE_RANGE_TABLE[block.weight_range as usize][0];

    set_bits_1_to_9(
        &mut out,
        &mut bit_pos,
        (block.subsets - 1) as u32,
        ASTC_PART_BITS,
    );

    if block.subsets == 1 {
        set_bits_1_to_9(&mut out, &mut bit_pos, block.cem as u32, ASTC_CEM_BITS);
    } else {
        set_bits(
            &mut out,
            &mut bit_pos,
            block.partition_seed as u32,
            ASTC_PARTITION_INDEX_BITS,
        );
        // low 2 bits zero: every subset shares the single CEM in the top 4 bits
        set_bits_1_to_9(
            &mut out,
            &mut bit_pos,
            ((block.cem << 2) & 63) as u32,
            ASTC_CEM_BITS + 2,
        );
    }

    if block.dual_plane {
        // the CCS field sits directly below the weight data, which fills the
        // block downward from bit 127
        let total_weight_bits = total_weights * bits_per_weight;
        let mut ccs_bit_pos = 128 - total_weight_bits - ASTC_CCS_BITS as i32;
        set_bits_1_to_9(&mut out, &mut ccs_bit_pos, block.ccs as u32, ASTC_CCS_BITS);
    }

    // the CEM's high bits give its class; a subset has class + 1 endpoint
    // pairs, two BISE values each
    let num_cem_pairs = (1 + (block.cem >> 2)) * block.subsets;
    pack_bise(
        &mut out,
        &block.endpoints,
        bit_pos,
        num_cem_pairs * 2,
        ENDPOINT_RANGES[uastc_mode as usize] as usize,
    );

    // Weight bits, written in reverse bit order from the top of the block.
    match bits_per_weight {
        1 => {
            for i in 0..total_weights {
                let ofs = (128 - 1 - i) as u32;
                out[(ofs >> 3) as usize] |= block.weights[i as usize] << (ofs & 7);
            }
        }
        2 => {
            for i in 0..total_weights {
                let ofs = (128 - 2 - i * 2) as u32;
                out[(ofs >> 3) as usize] |= REV2[block.weights[i as usize] as usize] << (ofs & 7);
            }
        }
        3 => {
            for i in 0..total_weights {
                let ofs = (128 - 3 - i * 3) as u32;
                let rev = (REV3[block.weights[i as usize] as usize] as u32) << (ofs & 7);
                let mut idx = (ofs >> 3) as usize;
                out[idx] |= (rev & 0xFF) as u8;
                idx += 1;
                if idx < 16 {
                    out[idx] |= (rev >> 8) as u8;
                }
            }
        }
        4 => {
            for i in 0..total_weights {
                let ofs = (128 - 4 - i * 4) as u32;
                out[(ofs >> 3) as usize] |= REV4[block.weights[i as usize] as usize] << (ofs & 7);
            }
        }
        5 => {
            for i in 0..total_weights {
                let ofs = (128 - 5 - i * 5) as u32;
                let rev = (REV5[block.weights[i as usize] as usize] as u32) << (ofs & 7);
                let mut idx = (ofs >> 3) as usize;
                out[idx] |= (rev & 0xFF) as u8;
                idx += 1;
                if idx < 16 {
                    out[idx] |= (rev >> 8) as u8;
                }
            }
        }
        _ => {}
    }

    out
}

/// Decode a 16-byte UASTC block and repack it as a 16-byte ASTC 4x4 block.
/// `None` on an invalid source block. The unpack is run with the
/// blue-contraction check on (the `true` argument): subsets whose endpoints
/// would trigger ASTC blue contraction get their endpoints swapped and weights
/// inverted, so the repacked block decodes without it. ETC1 hints are not read
/// (the `false` argument) since they are not needed to repack ASTC.
pub fn transcode_uastc_to_astc(src: &[u8; 16]) -> Option<[u8; 16]> {
    let u = unpack_to_block(src, true, false)?;
    if u.mode == UASTC_MODE_INDEX_SOLID_COLOR {
        Some(pack_astc_solid_block(u.solid_color))
    } else {
        Some(pack_astc_block(&u.astc, u.mode))
    }
}
