//! Anchor texel indices for the 11 three-subset ASTC/BC7 partition patterns.

/// The three subset anchor texel indices for partition `common_pattern`. An
/// anchor texel stores one fewer weight bit (its high bit is implied), so
/// `unpack` reads the short weight at these positions.
pub static ASTC_BC7_PATTERN3_ANCHORS: [[u8; 3]; 11] = [
    [0, 8, 10],
    [8, 0, 12],
    [4, 0, 12],
    [8, 0, 4],
    [3, 0, 2],
    [0, 1, 3],
    [0, 2, 1],
    [1, 9, 0],
    [1, 2, 0],
    [4, 0, 8],
    [0, 6, 2],
];
