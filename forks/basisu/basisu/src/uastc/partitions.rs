//! Descriptor tables for the partition patterns UASTC restricts itself to:
//! patterns representable by both an ASTC partition seed and a BC7 partition
//! index, so a multi-subset block transcodes to either format without
//! repartitioning.

/// A two-subset partition pattern representable by both ASTC and BC7.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AstcBc7CommonPartition2 {
    /// BC7 partition index.
    pub bc7: u8,
    /// ASTC partition seed producing the same pattern.
    pub astc: u16,
    /// The two formats label the subsets oppositely for this pattern, so
    /// subset indices must be swapped when mapping endpoints across.
    pub invert: bool,
}

/// A three-subset partition pattern representable by both ASTC and BC7.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AstcBc7CommonPartition3 {
    /// BC7 partition index.
    pub bc7: u8,
    /// ASTC partition seed producing the same pattern.
    pub astc: u16,
    /// Row of `ASTC_TO_BC7_PERM` that relabels this pattern's ASTC subset
    /// indices into BC7 subset indices.
    pub astc_to_bc7_perm: u8,
}

/// A BC7 three-subset pattern whose subsets merge into an ASTC two-subset
/// pattern.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bc73Astc2CommonPartition {
    /// BC7 partition index (three subsets).
    pub bc73: u8,
    /// ASTC partition seed of the merged two-subset pattern.
    pub astc2: u16,
    /// Merge variant: `k >> 1` selects which BC7 subsets collapse onto ASTC
    /// subset 0 and `k & 1` swaps the two resulting subsets.
    pub k: u8,
}

/// Shorthand `AstcBc7CommonPartition2` constructor used by the table literal.
const fn p2(bc7: u8, astc: u16, invert: bool) -> AstcBc7CommonPartition2 {
    AstcBc7CommonPartition2 { bc7, astc, invert }
}
/// Shorthand `AstcBc7CommonPartition3` constructor used by the table literal.
const fn p3(bc7: u8, astc: u16, astc_to_bc7_perm: u8) -> AstcBc7CommonPartition3 {
    AstcBc7CommonPartition3 {
        bc7,
        astc,
        astc_to_bc7_perm,
    }
}
/// Shorthand `Bc73Astc2CommonPartition` constructor used by the table literal.
const fn pb(bc73: u8, astc2: u16, k: u8) -> Bc73Astc2CommonPartition {
    Bc73Astc2CommonPartition { bc73, astc2, k }
}

/// The 30 two-subset patterns common to ASTC and BC7, indexed by a
/// two-subset mode's `common_pattern` field.
pub static ASTC_BC7_COMMON_PARTITIONS2: [AstcBc7CommonPartition2; 30] = [
    p2(0, 28, false),
    p2(1, 20, false),
    p2(2, 16, true),
    p2(3, 29, false),
    p2(4, 91, true),
    p2(5, 9, false),
    p2(6, 107, true),
    p2(7, 72, true),
    p2(8, 149, false),
    p2(9, 204, true),
    p2(10, 50, false),
    p2(11, 114, true),
    p2(12, 496, true),
    p2(13, 17, true),
    p2(14, 78, false),
    p2(15, 39, true),
    p2(17, 252, true),
    p2(18, 828, true),
    p2(19, 43, false),
    p2(20, 156, false),
    p2(21, 116, false),
    p2(22, 210, true),
    p2(23, 476, true),
    p2(24, 273, false),
    p2(25, 684, true),
    p2(26, 359, false),
    p2(29, 246, true),
    p2(32, 195, true),
    p2(33, 694, true),
    p2(52, 524, true),
];

/// The 11 three-subset patterns common to ASTC and BC7, indexed by mode 3's
/// `common_pattern` field.
pub static ASTC_BC7_COMMON_PARTITIONS3: [AstcBc7CommonPartition3; 11] = [
    p3(4, 260, 0),
    p3(8, 74, 5),
    p3(9, 32, 5),
    p3(10, 156, 2),
    p3(11, 183, 2),
    p3(12, 15, 0),
    p3(13, 745, 4),
    p3(20, 0, 1),
    p3(35, 335, 1),
    p3(36, 902, 5),
    p3(57, 254, 0),
];

/// The 19 BC7 three-subset patterns that merge into ASTC two-subset patterns,
/// indexed by mode 7's `common_pattern` field.
pub static BC7_3_ASTC2_COMMON_PARTITIONS: [Bc73Astc2CommonPartition; 19] = [
    pb(10, 36, 4),
    pb(11, 48, 4),
    pb(0, 61, 3),
    pb(2, 137, 4),
    pb(8, 161, 5),
    pb(13, 183, 4),
    pb(1, 226, 2),
    pb(33, 281, 2),
    pb(40, 302, 3),
    pb(20, 307, 4),
    pb(21, 479, 0),
    pb(58, 495, 3),
    pb(3, 593, 0),
    pb(32, 594, 2),
    pb(59, 605, 1),
    pb(34, 799, 3),
    pb(20, 812, 1),
    pb(14, 988, 4),
    pb(31, 993, 3),
];
