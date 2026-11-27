/// Anchor texel index per subset for the 19 partitions expressible both as a
/// three-subset BC7 pattern and a two-subset ASTC pattern (UASTC mode 7).
/// Indexed by the common pattern; the anchor is the texel in each subset whose
/// weight is stored with one fewer bit (its top bit is an implied zero). Rows
/// are 3 wide to share the layout of the 3-subset anchor tables, but only the
/// first two entries are meaningful here; the third is 0.
pub static BC7_3_ASTC2_PATTERNS2_ANCHORS: [[u8; 3]; 19] = [
    [0, 4, 0],
    [0, 2, 0],
    [2, 0, 0],
    [0, 7, 0],
    [8, 0, 0],
    [0, 1, 0],
    [0, 3, 0],
    [0, 1, 0],
    [2, 0, 0],
    [0, 1, 0],
    [0, 8, 0],
    [2, 0, 0],
    [0, 1, 0],
    [0, 7, 0],
    [12, 0, 0],
    [2, 0, 0],
    [9, 0, 0],
    [0, 2, 0],
    [4, 0, 0],
];
