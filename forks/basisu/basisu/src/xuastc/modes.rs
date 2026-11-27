//! XUASTC trial-mode tables: the per-block-size ASTC configuration lists
//! decoded from the packed config table, their grouping by (cem, subsets,
//! ccs, grid-size, grid-aniso), and the per-block-size unique
//! partition-pattern seed lookups. All are built once on first use, and both
//! decode loops index them by the coded `tm_index`.

use super::tables::{ASTC_CFG_TABLE, TOTAL_UNIQUE_PATTERNS, UNIQUE_INDEX_TO_PART_SEED};
use crate::once::OnceBox;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// The 14 ASTC block sizes.
pub const ASTC_BLOCK_SIZES: [(u32, u32); 14] = [
    (4, 4),
    (5, 4),
    (5, 5),
    (6, 5),
    (6, 6),
    (8, 5),
    (8, 6),
    (10, 5),
    (10, 6),
    (8, 8),
    (10, 8),
    (10, 10),
    (12, 10),
    (12, 12),
];

/// Index of the `(w, h)` block dimensions within `ASTC_BLOCK_SIZES`, or
/// `None` when that pairing is not one of the 14 ASTC block sizes.
pub fn block_size_index(w: u32, h: u32) -> Option<usize> {
    ASTC_BLOCK_SIZES.iter().position(|&d| d == (w, h))
}

/// One candidate ASTC configuration for a block: its color-endpoint mode,
/// subset count, optional dual-plane channel, weight grid, and the two ISE
/// range selectors.
#[derive(Clone, Copy)]
pub struct TrialMode {
    pub cem: u32,
    pub num_parts: u32,
    /// Dual-plane channel selector, -1 for single-plane.
    pub ccs_index: i32,
    /// Weight ISE range (0-based, valid ASTC weight range).
    pub weight_ise_range: u32,
    /// Endpoint ISE range (4-based, valid ASTC endpoint range).
    pub endpoint_ise_range: u32,
    pub grid_width: u32,
    pub grid_height: u32,
}

/// The trial modes available for one ASTC block size, together with an index
/// that groups them by coding parameters for fast candidate lookup.
pub struct BlockSizeModes {
    pub modes: Vec<TrialMode>,
    /// Trial-mode indices bucketed by
    /// `[cem][subsets-1][ccs+1][grid_size][grid_aniso]`, flattened through
    /// `group_index`.
    groups: Vec<Vec<u32>>,
}

// Extents of the five grouping dimensions, in the order `group_index` packs
// them. Their product is the number of `groups` buckets.
const OTM_NUM_CEMS: usize = 14;
const OTM_NUM_SUBSETS: usize = 3;
const OTM_NUM_CCS: usize = 5;
const OTM_NUM_GRID_SIZES: usize = 2;
const OTM_NUM_GRID_ANISOS: usize = 3;

/// Flatten a `(cem, subsets, ccs, grid_size, aniso)` coordinate into a single
/// index over the `groups` buckets, row-major in that dimension order.
#[inline]
fn group_index(cem: usize, subsets: usize, ccs: usize, grid_size: usize, aniso: usize) -> usize {
    (((cem * OTM_NUM_SUBSETS + subsets) * OTM_NUM_CCS + ccs) * OTM_NUM_GRID_SIZES + grid_size)
        * OTM_NUM_GRID_ANISOS
        + aniso
}

/// Classify the weight grid's aspect against the block: 0 when the grid and
/// block share the same aspect ratio, 1 when the grid is relatively denser in
/// X, 2 when it is relatively denser in Y. Comparing `gw*bh` against `gh*bw`
/// tests `gw/bw` versus `gh/bh` without dividing.
#[inline]
pub fn calc_grid_aniso_val(gw: u32, gh: u32, bw: u32, bh: u32) -> usize {
    let lhs = gw * bh;
    let rhs = gh * bw;
    if lhs == rhs {
        0
    } else if lhs >= rhs {
        1
    } else {
        2
    }
}

impl BlockSizeModes {
    /// The trial-mode indices grouped under one `(cem, subset, ccs,
    /// grid_size, grid_aniso)` coordinate.
    pub fn tm_candidates(
        &self,
        cem_index: u32,
        subset_index: u32,
        ccs_index: u32,
        grid_size: u32,
        grid_aniso: u32,
    ) -> &[u32] {
        &self.groups[group_index(
            cem_index as usize,
            subset_index as usize,
            ccs_index as usize,
            grid_size as usize,
            grid_aniso as usize,
        )]
    }
}

/// Maps a packed unique-CEM index to the ASTC color-endpoint mode number it
/// stands for.
const UNIQUE_LDR_INDEX_TO_CEM: [u32; 6] = [0, 4, 6, 8, 10, 12];

/// Build the trial-mode list and grouping index for one ASTC block size by
/// unpacking every entry of the shared packed config table that fits the
/// block.
fn build(bw: u32, bh: u32) -> BlockSizeModes {
    let mut modes = Vec::with_capacity(3072);
    let mut groups = alloc::vec![Vec::new(); OTM_NUM_CEMS * OTM_NUM_SUBSETS * OTM_NUM_CCS * OTM_NUM_GRID_SIZES
            * OTM_NUM_GRID_ANISOS];

    for &packed in ASTC_CFG_TABLE.iter() {
        let mut p = packed;
        let mut take = |bits: u32| {
            let v = p & ((1 << bits) - 1);
            p >>= bits;
            v
        };
        let endpoint_ise_range = take(5);
        let weight_ise_range = take(4);
        let ccs_index = take(3);
        let num_subsets = take(2);
        let unique_cem_index = take(3);
        let grid_wh = take(7);

        let grid_width = grid_wh / 11 + 2;
        // Configs are sorted by grid width; nothing after the first
        // too-wide one fits.
        if grid_width > bw {
            break;
        }
        let grid_height = grid_wh % 11 + 2;
        if grid_height > bh {
            continue;
        }

        let tm = TrialMode {
            cem: UNIQUE_LDR_INDEX_TO_CEM[unique_cem_index as usize],
            num_parts: num_subsets + 1,
            ccs_index: ccs_index as i32 - 1,
            weight_ise_range,
            endpoint_ise_range: endpoint_ise_range + 4,
            grid_width,
            grid_height,
        };

        let tm_index = modes.len() as u32;
        // Bucket this mode by its coding parameters so a lookup can fetch
        // every candidate sharing a coordinate at once.
        // grid_size flags a near-full-resolution grid: within one texel of the
        // block dimensions on both axes.
        let grid_size = usize::from(tm.grid_width >= bw - 1 && tm.grid_height >= bh - 1);
        let aniso = calc_grid_aniso_val(tm.grid_width, tm.grid_height, bw, bh);
        groups[group_index(
            tm.cem as usize,
            (tm.num_parts - 1) as usize,
            (tm.ccs_index + 1) as usize,
            grid_size,
            aniso,
        )]
        .push(tm_index);

        modes.push(tm);
    }

    BlockSizeModes { modes, groups }
}

/// The per-block-size tables, built once on first use and cached for the
/// process lifetime. Indexed by the block size's position in
/// `ASTC_BLOCK_SIZES`.
pub fn block_size_modes(block_size_index: usize) -> &'static BlockSizeModes {
    static TABLES: OnceBox<[BlockSizeModes; 14]> = OnceBox::new();
    &TABLES.get_or_init(|| {
        Box::new(core::array::from_fn(|i| {
            let (bw, bh) = ASTC_BLOCK_SIZES[i];
            build(bw, bh)
        }))
    })[block_size_index]
}

/// Number of unique partition patterns for a block size and subset count
/// (`num_parts` is 2 or 3).
#[inline]
pub fn total_unique_patterns(block_size_index: usize, num_parts: u32) -> u32 {
    TOTAL_UNIQUE_PATTERNS[block_size_index][(num_parts - 2) as usize] as u32
}

/// The ASTC partition seed for a given unique-pattern index, block size, and
/// subset count (`num_parts` is 2 or 3).
#[inline]
pub fn unique_pat_index_to_part_seed(
    block_size_index: usize,
    num_parts: u32,
    unique_pat_index: u32,
) -> u16 {
    UNIQUE_INDEX_TO_PART_SEED[(num_parts - 2) as usize][block_size_index][unique_pat_index as usize]
}
