//! The patchwork: nine patches of the still wall's tiles, three across and
//! three down. Two never change. In the other seven a frame changes a small
//! part and leaves the rest as it was, each patch in a way of its own: one
//! row of its tiles, two items of a row, one row of a list, one word of a
//! paragraph, one tile under others. A framework that keeps what did not
//! change pays for the part that did; one that builds the screen again each
//! frame pays for all of it. Each kind of change costs the two designs
//! differently, so no one design pays little for all nine.
//!
//! Everything a frame shows follows from its frame index `k`, in integer
//! arithmetic, as on the still wall, whose tiers, tiles and clock line the
//! patchwork takes. `../README.md` describes the patches.

use crate::wall::{WALL_GAP, wall_label};

/// The patches across the screen.
pub const PATCH_COLUMNS: usize = 3;
/// The patches down the screen.
pub const PATCH_ROWS: usize = 3;

/// The patches in the order the screen shows them, row by row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Patch {
    /// Tiles that never change.
    Still,
    /// Line charts of many points that never change.
    Charts,
    /// Tiles whose content never changes and whose fill changes every
    /// few frames, one row of the tiles each frame.
    Recolor,
    /// Cells that show another kind of content every few frames, one row of
    /// the cells each frame.
    Churn,
    /// A wrapping row of items 1 to 3 tiles wide, two of which change places
    /// every frame.
    Reorder,
    /// Boxes nested `PATCHWORK_DEPTH` deep around digits that change every
    /// few frames, in one row of the nests each frame.
    Deep,
    /// A list of rows of tiles that scrolls by one row every frame.
    Scroll,
    /// One paragraph of five-digit words, one of which changes every frame.
    Paragraph,
    /// Translucent tiles lying over one another, one of which is opaque,
    /// another one every frame.
    Pile,
}

impl Patch {
    /// Every patch, in the screen's order.
    pub const ALL: [Self; PATCH_COLUMNS * PATCH_ROWS] = [
        Self::Still,
        Self::Charts,
        Self::Recolor,
        Self::Churn,
        Self::Reorder,
        Self::Deep,
        Self::Scroll,
        Self::Paragraph,
        Self::Pile,
    ];
}

/// The step, in tiles or words, of everything that jumps: with the still
/// wall's cursor it crosses a patch in a few frames.
const JUMP: usize = 37;

/// The items of a patch that change in turn: `columns` by `rows` tiles,
/// cells or nests. Each frame changes the items of one row, the next row
/// down the frame after, so a frame changes one row of the patch and the
/// others stay. What stays grows faster with the tier than what changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InTurn {
    pub columns: usize,
    pub rows: usize,
}

impl InTurn {
    /// The changes of item `index` up to frame `frame`: one more every
    /// `rows` frames, in the frame of the item's row.
    pub fn step(self, index: usize, frame: u32) -> usize {
        (index / self.columns.max(1) + frame as usize) / self.rows.max(1)
    }
}

/// The points of one chart of the charts patch. A chart covers 2 by 2 tiles
/// and the gap between them, so its points lie closer than a point apart at
/// the higher tiers: a chart of more samples than it has pixels.
pub const PATCHWORK_CHART_POINTS: usize = 512;
/// The width of a chart's line, in points.
pub const PATCHWORK_CHART_STROKE: f32 = 1.0;

/// The height of chart `index` at `point`, as a fraction of the chart: the
/// gauntlet's sparkline on frame 0, eight points of the chart to one of the
/// sparkline's.
pub fn patchwork_chart_value(index: usize, point: usize) -> f32 {
    let at = point as f32 * 0.125;
    0.5 + 0.38 * (at * 0.32 + index as f32 * 0.77).sin() + 0.08 * (at * 1.7 + index as f32).sin()
}

/// The color of chart `index`'s line.
pub fn patchwork_chart_rgb(index: usize) -> [u8; 3] {
    crate::PALETTE_RGB[index % crate::PALETTE_RGB.len()]
}

/// The fill of tile `index` of the recolor patch in frame `frame`.
pub fn patchwork_recolor_rgb(index: usize, frame: u32, turn: InTurn) -> [u8; 3] {
    crate::PALETTE_RGB[(index + turn.step(index, frame)) % crate::PALETTE_RGB.len()]
}

/// What a cell of the churn patch shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChurnKind {
    /// The wall's tile: a label over a dot and a value.
    Tile,
    /// A fill of the tile's dot color, and nothing on it.
    Blank,
    /// The tile's fill with its label alone.
    Label,
}

/// What cell `index` of the churn patch shows in frame `frame`: each cell
/// goes through the three kinds.
pub fn patchwork_churn(index: usize, frame: u32, turn: InTurn) -> ChurnKind {
    match turn.step(index, frame) % 3 {
        0 => ChurnKind::Tile,
        1 => ChurnKind::Blank,
        _ => ChurnKind::Label,
    }
}

/// The items of the reorder patch for a patch of `tiles` tiles: they fill
/// about the patch, and its edge cuts the rows past it.
pub fn patchwork_reorder_count(tiles: usize) -> usize {
    (tiles * 2 / 5).max(1)
}

/// The first of the two neighbouring places of the reorder patch whose items
/// change places in frame `frame`. Every other item lies at the place of its
/// own number, and so do these two again in the next frame.
pub fn patchwork_reorder_swap(frame: u32, count: usize) -> usize {
    (frame as usize).wrapping_mul(JUMP) % count.saturating_sub(1).max(1)
}

/// The item at place `position` of the reorder patch in frame `frame`.
pub fn patchwork_reorder_item(position: usize, frame: u32, count: usize) -> usize {
    let swap = patchwork_reorder_swap(frame, count);
    if count < 2 {
        position
    } else if position == swap {
        position + 1
    } else if position == swap + 1 {
        position - 1
    } else {
        position
    }
}

/// The width of item `item` of the reorder patch, in tiles: 1 to 3. An item
/// `units` tiles wide covers them and the gaps between them, so every item
/// starts on the tiles' grid.
pub fn patchwork_reorder_units(item: usize) -> usize {
    ((item as u32).wrapping_mul(2_654_435_761) >> 22) as usize % 3 + 1
}

/// The width in points of an item `units` tiles wide.
pub fn patchwork_reorder_width(units: usize, tile_width: f32) -> f32 {
    units as f32 * tile_width + (units - 1) as f32 * WALL_GAP
}

/// The boxes around the digits of one nest of the deep patch. A nest covers
/// 2 by 2 tiles and the gap between them; each box lies the tile's padding
/// inside the one around it.
pub const PATCHWORK_DEPTH: usize = 12;

/// The nests across and down the deep patch, and the charts across and down
/// the charts patch, for a patch of `columns` by `rows` tiles: both hold
/// items 2 by 2 tiles large.
pub fn patchwork_pairs(columns: usize, rows: usize) -> (usize, usize) {
    ((columns / 2).max(1), (rows / 2).max(1))
}

/// The width or height of an item that covers 2 tiles of `tile` points and
/// the gap between them.
pub fn patchwork_double(tile: f32) -> f32 {
    2.0 * tile + WALL_GAP
}

/// The fill of box `level` of nest `index`, the outermost box being level 0.
pub fn patchwork_deep_rgb(index: usize, level: usize) -> [u8; 3] {
    crate::PALETTE_RGB[(index + level) % crate::PALETTE_RGB.len()]
}

/// The number inside nest `index` in frame `frame`, under 1000.
pub fn patchwork_deep_number(index: usize, frame: u32, turn: InTurn) -> usize {
    (index * 7 + turn.step(index, frame)) % 1000
}

/// The three digits inside nest `index` in frame `frame`.
pub fn patchwork_deep_value(index: usize, frame: u32, turn: InTurn) -> String {
    format!("{:03}", patchwork_deep_number(index, frame, turn))
}

/// The rows of the scroll patch's list for rows of `columns` tiles: every
/// tile of the list has a label of its own in five digits.
pub fn patchwork_scroll_rows(columns: usize) -> usize {
    100_000 / columns.max(1)
}

/// The first row the scroll patch shows in frame `frame`, of a list of
/// `total` rows of which `visible` show at once: one row further each frame,
/// and from the list's end back to its start.
pub fn patchwork_scroll_first(frame: u32, total: usize, visible: usize) -> usize {
    frame as usize % (total.saturating_sub(visible) + 1)
}

/// The words of the paragraph patch for a patch of `tiles` tiles.
pub fn patchwork_paragraph_words(tiles: usize) -> usize {
    tiles * 3
}

/// The paragraph in frame `frame`: `words` words a space apart, each its
/// index in five digits, but for one that shows the frame instead, another
/// one every frame. Digits are as wide as one another, so no line of the
/// paragraph breaks at another word for it.
pub fn patchwork_paragraph(frame: u32, words: usize) -> String {
    let changed = (frame as usize).wrapping_mul(JUMP) % words.max(1);
    let mut text = String::with_capacity(words * 6);
    for word in 0..words {
        if word > 0 {
            text.push(' ');
        }
        text.push_str(&wall_label(if word == changed {
            frame as usize % 100_000
        } else {
            word
        }));
    }
    text
}

/// The tiles across and down the pile patch for a patch of `columns` by
/// `rows` tiles: one starts at every tile's place but the last column's and
/// the last row's, and covers 2 by 2 tiles, so four tiles lie over most
/// points.
pub fn patchwork_pile_grid(columns: usize, rows: usize) -> (usize, usize) {
    (
        columns.saturating_sub(1).max(1),
        rows.saturating_sub(1).max(1),
    )
}

/// The opacity of the pile patch's tiles' fills, of 255.
pub const PATCHWORK_PILE_ALPHA: u8 = 0x99;

/// The tile of the pile patch whose fill is opaque in frame `frame`, with
/// `PATCHWORK_PILE_OPAQUE_RGB` for its color.
pub fn patchwork_pile_opaque(frame: u32, tiles: usize) -> usize {
    (frame as usize).wrapping_mul(JUMP) % tiles.max(1)
}

/// The fill of the pile patch's opaque tile.
pub const PATCHWORK_PILE_OPAQUE_RGB: [u8; 3] = [0x00, 0x00, 0x00];
