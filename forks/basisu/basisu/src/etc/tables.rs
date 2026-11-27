//! ETC1 and ETC2-EAC constant tables shared by the UASTC-to-ETC and
//! ETC1S-to-ETC transcode paths.

/// ETC1 intensity modifiers indexed `[table][selector]`, added to the subblock
/// base color to form its four candidate colors.
pub static INTEN_TABLES: [[i32; 4]; 8] = [
    [-8, -2, 2, 8],
    [-17, -5, 5, 17],
    [-29, -9, 9, 29],
    [-42, -13, 13, 42],
    [-60, -18, 18, 60],
    [-80, -24, 24, 80],
    [-106, -33, 33, 106],
    [-183, -47, 47, 183],
];

/// EAC deltas indexed `[table][selector]`; a decoder scales the delta by the
/// block multiplier and adds the base value.
pub static EAC_MODIFIER_TABLE: [[i8; 8]; 16] = [
    [-3, -6, -9, -15, 2, 5, 8, 14],
    [-3, -7, -10, -13, 2, 6, 9, 12],
    [-2, -5, -8, -13, 1, 4, 7, 12],
    [-2, -4, -6, -13, 1, 3, 5, 12],
    [-3, -6, -8, -12, 2, 5, 7, 11],
    [-3, -7, -9, -11, 2, 6, 8, 10],
    [-4, -7, -8, -11, 3, 6, 7, 10],
    [-3, -5, -8, -11, 2, 4, 7, 10],
    [-2, -6, -8, -10, 1, 5, 7, 9],
    [-2, -5, -8, -10, 1, 4, 7, 9],
    [-2, -4, -8, -10, 1, 3, 7, 9],
    [-2, -5, -7, -10, 1, 4, 6, 9],
    [-3, -4, -7, -10, 2, 3, 6, 9],
    [-1, -2, -3, -10, 0, 1, 2, 9],
    [-4, -6, -8, -9, 3, 5, 7, 8],
    [-3, -5, -7, -9, 2, 4, 6, 8],
];

/// ETC1 texel positions indexed `[flip][subblock][pixel]`, each entry an
/// `[x, y]` coordinate within the 4x4 block.
pub static ETC1_PIXEL_COORDS: [[[[u8; 2]; 8]; 2]; 2] = [
    [
        [
            [0, 0],
            [0, 1],
            [0, 2],
            [0, 3],
            [1, 0],
            [1, 1],
            [1, 2],
            [1, 3],
        ],
        [
            [2, 0],
            [2, 1],
            [2, 2],
            [2, 3],
            [3, 0],
            [3, 1],
            [3, 2],
            [3, 3],
        ],
    ],
    [
        [
            [0, 0],
            [1, 0],
            [2, 0],
            [3, 0],
            [0, 1],
            [1, 1],
            [2, 1],
            [3, 1],
        ],
        [
            [0, 2],
            [1, 2],
            [2, 2],
            [3, 2],
            [0, 3],
            [1, 3],
            [2, 3],
            [3, 3],
        ],
    ],
];

/// Ready-made ETC1 selector payloads (block bytes 4..8, MSB plane then LSB
/// plane) with all 16 pixels set to the same selector; row i is linear
/// selector i re-expressed in the ETC1 encoding.
pub static ETC1_SOLID_SELECTORS: [[u8; 4]; 4] = [
    [255, 255, 255, 255],
    [255, 255, 0, 0],
    [0, 0, 0, 0],
    [0, 0, 255, 255],
];

/// The packed 3-bit selector stream of an EAC alpha block with every pixel set
/// to selector 4 (binary 100 repeated 16 times).
pub static ETC2_EAC_A8_SEL4: [u8; 6] = [0x92, 0x49, 0x24, 0x92, 0x49, 0x24];

/// Maps a linear selector rank (lowest to highest modifier) to the ETC1
/// selector encoding.
pub static SELECTOR_INDEX_TO_ETC1: [u8; 4] = [3, 2, 0, 1];

/// 5-bit to 8-bit channel expansion.
pub static ETC_5_TO_8: [u8; 32] = [
    0, 8, 16, 24, 33, 41, 49, 57, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 198, 206, 214, 222, 231, 239, 247, 255,
];

/// Lowest per-channel delta a differential-mode ETC1 block can encode (3-bit
/// two's complement).
pub const ETC1_COLOR_DELTA_MIN: i32 = -4;
/// Highest per-channel delta a differential-mode ETC1 block can encode.
pub const ETC1_COLOR_DELTA_MAX: i32 = 3;
