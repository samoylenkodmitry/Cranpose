//! Anchor texel indices for the 30 two-subset ASTC/BC7 partition patterns.

/// Anchor texel of each subset for partition `common_pattern`. An anchor texel
/// stores one fewer weight bit (its high bit is implied), so the weight decode
/// in `unpack_to_block` reads the short form at these positions. Two slots
/// hold the two subset anchors; the third stays 0 so every anchor table shares
/// the three-wide row shape.
pub static ASTC_BC7_PATTERN2_ANCHORS: [[u8; 3]; 30] = [
    [0, 2, 0],
    [0, 3, 0],
    [1, 0, 0],
    [0, 3, 0],
    [7, 0, 0],
    [0, 2, 0],
    [3, 0, 0],
    [7, 0, 0],
    [0, 11, 0],
    [2, 0, 0],
    [0, 7, 0],
    [11, 0, 0],
    [3, 0, 0],
    [8, 0, 0],
    [0, 4, 0],
    [12, 0, 0],
    [1, 0, 0],
    [8, 0, 0],
    [0, 1, 0],
    [0, 2, 0],
    [0, 4, 0],
    [8, 0, 0],
    [1, 0, 0],
    [0, 2, 0],
    [4, 0, 0],
    [0, 1, 0],
    [4, 0, 0],
    [1, 0, 0],
    [4, 0, 0],
    [1, 0, 0],
];
