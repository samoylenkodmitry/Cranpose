use perf_data::{WALL_GAP, WallTier, wall_grid};

pub(crate) const HEADER: f32 = 56.0;
pub(crate) const BAR: [u8; 3] = [0x1e, 0x2a, 0x4a];

#[derive(Clone, Copy)]
pub(crate) struct Grid {
    pub tier: WallTier,
    columns: usize,
    pub tiles: usize,
}

impl Grid {
    pub fn new(tier: WallTier, width: f32, height: f32) -> Self {
        let (columns, rows) = wall_grid(tier, width, height);
        Self {
            tier,
            columns,
            tiles: columns * rows,
        }
    }

    pub fn origin(self, index: usize) -> (f32, f32) {
        (
            WALL_GAP + (index % self.columns) as f32 * (self.tier.tile_width + WALL_GAP),
            WALL_GAP + (index / self.columns) as f32 * (self.tier.tile_height + WALL_GAP),
        )
    }
}
