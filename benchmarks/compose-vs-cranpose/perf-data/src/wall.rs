//! The still wall: a window of tiles that never change, under a clock line
//! whose text changes every frame, with a cursor outline that rests on
//! another tile every frame. Each frame changes two small things on a screen
//! of thousands of nodes, so a framework that pays for the whole screen each
//! frame shows that cost, and one that pays only for what changed does not.
//!
//! Everything a frame shows follows from its frame index `k`, in integer
//! arithmetic, so every app shows the same digits and the same cursor on
//! the same frame. `../README.md` describes the screen and its tiers.

/// One load tier of the still wall: the size of its tiles, in whole points
/// so that no framework rounds a row of them past the window. The tiles
/// fill the wall's area: `../README.md` lists their counts in the desktop
/// window.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WallTier {
    pub tile_width: f32,
    pub tile_height: f32,
}

/// The tiers, 1 to 20: each holds about 1.2 to 1.4 times the tiles of the one
/// before.
pub const WALL_TIERS: [WallTier; 20] = [
    WallTier {
        tile_width: 64.0,
        tile_height: 40.0,
    },
    WallTier {
        tile_width: 54.0,
        tile_height: 34.0,
    },
    WallTier {
        tile_width: 46.0,
        tile_height: 29.0,
    },
    WallTier {
        tile_width: 40.0,
        tile_height: 25.0,
    },
    WallTier {
        tile_width: 34.0,
        tile_height: 21.0,
    },
    WallTier {
        tile_width: 30.0,
        tile_height: 18.0,
    },
    WallTier {
        tile_width: 26.0,
        tile_height: 16.0,
    },
    WallTier {
        tile_width: 22.0,
        tile_height: 14.0,
    },
    WallTier {
        tile_width: 19.0,
        tile_height: 12.0,
    },
    WallTier {
        tile_width: 16.0,
        tile_height: 10.0,
    },
    WallTier {
        tile_width: 14.0,
        tile_height: 9.0,
    },
    WallTier {
        tile_width: 12.0,
        tile_height: 8.0,
    },
    WallTier {
        tile_width: 10.0,
        tile_height: 7.0,
    },
    WallTier {
        tile_width: 8.0,
        tile_height: 6.0,
    },
    WallTier {
        tile_width: 7.0,
        tile_height: 5.0,
    },
    WallTier {
        tile_width: 6.0,
        tile_height: 4.0,
    },
    WallTier {
        tile_width: 5.0,
        tile_height: 4.0,
    },
    WallTier {
        tile_width: 5.0,
        tile_height: 3.0,
    },
    WallTier {
        tile_width: 4.0,
        tile_height: 3.0,
    },
    WallTier {
        tile_width: 3.0,
        tile_height: 3.0,
    },
];

/// Tier `1..=20`; anything else is clamped into that range.
pub fn wall_tier(tier: usize) -> WallTier {
    WALL_TIERS[tier.clamp(1, WALL_TIERS.len()) - 1]
}

/// The tiles across and down that fill a wall `width` by `height` points
/// with tiles of `tier`, `WALL_GAP` apart and from the wall's edges.
pub fn wall_grid(tier: WallTier, width: f32, height: f32) -> (usize, usize) {
    let fit =
        |space: f32, tile: f32| ((space - WALL_GAP) / (tile + WALL_GAP)).floor().max(1.0) as usize;
    (fit(width, tier.tile_width), fit(height, tier.tile_height))
}

/// The space between two tiles and around the wall, in points.
pub const WALL_GAP: f32 = 1.0;
/// The height of the clock line between the top bar and the wall, in points.
pub const WALL_CLOCK_HEIGHT: f32 = 24.0;
/// The clock line's text size, in points.
pub const WALL_CLOCK_TEXT: f32 = 14.0;
/// A tile's corner radius, in points.
pub const WALL_CORNER: f32 = 2.0;
/// The width of the cursor's outline, in points.
pub const WALL_CURSOR_WIDTH: f32 = 2.0;
/// The wall's background behind the tiles.
pub const WALL_BACKGROUND_RGB: [u8; 3] = [0x0F, 0x12, 0x20];
/// The color of the tiles' text and of the cursor.
pub const WALL_INK_RGB: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// A tile's label: its index in five digits.
pub fn wall_label(index: usize) -> String {
    format!("{index:05}")
}

/// A tile's value, which never changes: three digits that its index mixes.
pub fn wall_value(index: usize) -> String {
    let mixed = (index as u32).wrapping_mul(2_654_435_761) >> 22;
    format!("{:03}", mixed % 1000)
}

/// A tile's fill.
pub fn wall_fill_rgb(index: usize) -> [u8; 3] {
    crate::PALETTE_RGB[index % crate::PALETTE_RGB.len()]
}

/// The color of a tile's dot.
pub fn wall_dot_rgb(index: usize) -> [u8; 3] {
    crate::CHIP_BACKGROUND_RGB
        [(index / crate::PALETTE_RGB.len()) % crate::CHIP_BACKGROUND_RGB.len()]
}

/// The tile the cursor rests on in frame `frame`: it jumps 37 tiles a
/// frame, so in a few frames it has crossed the whole wall.
pub fn wall_cursor(frame: u32, tiles: usize) -> usize {
    (frame as usize).wrapping_mul(37) % tiles.max(1)
}

/// The clock line in frame `frame`.
pub fn wall_clock(frame: u32) -> String {
    format!("frame {frame:06}")
}

/// The size of a tile's label and value text for tiles `height` points
/// tall. Neither wraps: a tile shows each on one line.
pub fn wall_text_size(height: f32) -> (f32, f32) {
    (height * 0.30, height * 0.26)
}

/// A tile's padding for tiles `height` points tall.
pub fn wall_padding(height: f32) -> f32 {
    height * 0.06
}

/// The diameter of a tile's dot for tiles `height` points tall.
pub fn wall_dot_size(height: f32) -> f32 {
    height * 0.22
}
