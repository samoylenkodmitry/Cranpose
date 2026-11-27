//! UASTC per-mode property tables, one entry per mode, indexed 0..=18. Each
//! table answers one question about a mode: its
//! weight bits and BISE ranges, subset and plane counts, component count, the
//! ETC1/BC1 hint flags, alpha layout, color endpoint mode, and the Huffman code
//! used to read the mode index from the block.

use super::TOTAL_UASTC_MODES;

/// Weight bits per texel.
pub static WEIGHT_BITS: [u8; TOTAL_UASTC_MODES] =
    [4, 2, 3, 2, 2, 3, 2, 2, 0, 2, 4, 2, 3, 1, 2, 4, 2, 2, 5];

/// BISE range index of the weight values.
pub static WEIGHT_RANGES: [u8; TOTAL_UASTC_MODES] =
    [8, 2, 5, 2, 2, 5, 2, 2, 0, 2, 8, 2, 5, 0, 2, 8, 2, 2, 11];

/// BISE range index of the endpoint values.
pub static ENDPOINT_RANGES: [u8; TOTAL_UASTC_MODES] = [
    19, 20, 8, 7, 12, 20, 18, 12, 0, 8, 13, 13, 19, 20, 20, 20, 20, 20, 11,
];

/// Number of ASTC partitions/subsets.
pub static SUBSETS: [u8; TOTAL_UASTC_MODES] =
    [1, 1, 2, 3, 2, 1, 1, 2, 0, 2, 1, 1, 1, 1, 1, 1, 2, 1, 1];

/// Plane count: 2 for the dual-plane modes, otherwise 1.
pub static PLANES: [u8; TOTAL_UASTC_MODES] =
    [1, 1, 1, 1, 1, 1, 2, 1, 0, 1, 1, 2, 1, 2, 1, 1, 1, 2, 1];

/// Endpoint component count: 2 = luminance-alpha, 3 = RGB, 4 = RGBA.
pub static COMPS: [u8; TOTAL_UASTC_MODES] =
    [3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 2, 2, 2, 3];

/// Whether the mode's hint bits include a 5-bit ETC1 bias.
pub static HAS_ETC1_BIAS: [u8; TOTAL_UASTC_MODES] =
    [1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1];

/// Whether the mode's hint bits include BC1 hint 0.
pub static HAS_BC1_HINT0: [u8; TOTAL_UASTC_MODES] =
    [1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];

/// Whether the mode's hint bits include BC1 hint 1.
pub static HAS_BC1_HINT1: [u8; TOTAL_UASTC_MODES] =
    [1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1];

/// Whether the mode encodes alpha.
pub static HAS_ALPHA: [u8; TOTAL_UASTC_MODES] =
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0];

/// Whether the mode is luminance-alpha.
pub static IS_LA: [u8; TOTAL_UASTC_MODES] =
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0];

/// ASTC color endpoint mode per UASTC mode: 8 = RGB direct, 12 = RGBA direct,
/// 4 = LA direct. Unused (0) for the solid-color mode, which packs as a
/// void-extent block.
pub static CEM: [u8; TOTAL_UASTC_MODES] = [
    8, 8, 8, 8, 8, 8, 8, 8, 0, 12, 12, 12, 12, 12, 12, 4, 4, 4, 8,
];

/// Total hint bits per mode; used to skip the hint region when not reading
/// hints.
pub static TOTAL_HINT_BITS: [u8; TOTAL_UASTC_MODES] = [
    15, 15, 15, 15, 15, 15, 15, 15, 0, 23, 17, 17, 17, 23, 23, 23, 23, 23, 15,
];

/// Mode Huffman codes as `{code, size}` pairs, used to decode the mode index
/// from the start of a UASTC block. `TOTAL_UASTC_MODES + 1` entries: index 19
/// is reserved, and a block decoding to it is rejected as invalid.
pub static MODE_HUFF_CODES: [[u32; 2]; TOTAL_UASTC_MODES + 1] = [
    [0x1, 4],
    [0x35, 6],
    [0x1D, 5],
    [0x3, 5],
    [0x13, 5],
    [0xB, 5],
    [0x1B, 5],
    [0x7, 5],
    [0x17, 5],
    [0xF, 5],
    [0x2, 3],
    [0x0, 2],
    [0x6, 3],
    [0x1F, 5],
    [0xD, 5],
    [0x5, 7],
    [0x15, 6],
    [0x25, 6],
    [0x9, 4],
    [0x45, 7],
];

/// Subset-index permutations from ASTC order to BC7 order for the
/// three-subset common partitions; the row is selected by a partition
/// descriptor's `astc_to_bc7_perm` field.
pub static ASTC_TO_BC7_PERM: [[u8; 3]; 6] = [
    [0, 1, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
    [0, 2, 1],
    [1, 0, 2],
];

/// Row-by-row inverses of `ASTC_TO_BC7_PERM`, mapping BC7 subset order back
/// to ASTC order.
pub static BC7_TO_ASTC_PERM: [[u8; 3]; 6] = [
    [0, 1, 2],
    [2, 0, 1],
    [1, 2, 0],
    [2, 1, 0],
    [0, 2, 1],
    [1, 0, 2],
];

/// ASTC BISE range table: one `{bits, trits, quints}` row per range index
/// 0..=20, giving how values of that range are encoded.
pub static ASTC_BISE_RANGE_TABLE: [[i32; 3]; 21] = [
    [1, 0, 0],
    [0, 1, 0],
    [2, 0, 0],
    [0, 0, 1],
    [1, 1, 0],
    [3, 0, 0],
    [1, 0, 1],
    [2, 1, 0],
    [4, 0, 0],
    [2, 0, 1],
    [3, 1, 0],
    [5, 0, 0],
    [3, 0, 1],
    [4, 1, 0],
    [6, 0, 0],
    [4, 0, 1],
    [5, 1, 0],
    [7, 0, 0],
    [5, 0, 1],
    [6, 1, 0],
    [8, 0, 0],
];
