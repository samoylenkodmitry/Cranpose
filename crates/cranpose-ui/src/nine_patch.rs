//! Filling a box with an image that is bigger, or smaller, than the box.
//!
//! Two questions this answers that a plain scale cannot. A tiled fill repeats
//! its source instead of stretching it, which is how a texture, a hatch or a
//! skin's background covers an area of any size without going soft. And a
//! nine-patch fill keeps the corners of a source at their own size while the
//! edges and the middle grow, which is how a button, a panel or a scrollbar
//! trough drawn once at one size is drawn correctly at every other.
//!
//! Both are expressed the same way: a list of source-rectangle to
//! destination-rectangle pairs, each of which is one ordinary image draw. That
//! keeps the answer identical on every backend and, unlike a repeating sampler,
//! cannot bleed a neighbouring sprite in from a shared atlas — the sprite an
//! application tiles is usually a region of an atlas, and a wrapped sampler
//! reads whatever is next to it.

use cranpose_ui_graphics::{Rect, Size};

/// How a region larger than its source is filled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PatchFill {
    /// Scale the source to fill the region.
    #[default]
    Stretch,
    /// Repeat the source at its own size, clipping the last row and column.
    Tile,
}

impl PatchFill {
    /// Whether this fill repeats rather than scales.
    pub fn is_tiled(self) -> bool {
        matches!(self, PatchFill::Tile)
    }
}

/// How far in from each edge of a source image the stretchable middle starts.
///
/// The four corners outside these insets are drawn at their own size at every
/// destination size; everything inside grows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NinePatchInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl NinePatchInsets {
    /// Insets clamped to non-negative values.
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left: sanitize(left),
            top: sanitize(top),
            right: sanitize(right),
            bottom: sanitize(bottom),
        }
    }

    /// The same inset on all four edges.
    pub fn uniform(inset: f32) -> Self {
        Self::new(inset, inset, inset, inset)
    }

    /// Insets scaled by `factor`, for a source measured in pixels drawn into a
    /// destination measured in points.
    pub fn scaled(self, factor: f32) -> Self {
        if !factor.is_finite() || factor <= 0.0 {
            return self;
        }
        Self::new(
            self.left * factor,
            self.top * factor,
            self.right * factor,
            self.bottom * factor,
        )
    }

    /// Whether a source of this size has room for the corners plus a middle.
    ///
    /// Insets that meet or cross leave no stretchable middle, which is not a
    /// nine-patch at all — the caller wanted a plain scale.
    pub fn fit(self, source: Size) -> bool {
        source.width > self.left + self.right && source.height > self.top + self.bottom
    }
}

fn sanitize(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

/// One image draw: the part of the source to read and the part of the
/// destination to fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatchQuad {
    pub source: Rect,
    pub destination: Rect,
}

/// The draws that tile `source` across `destination` at the source's own size.
///
/// The last row and column are clipped, so a destination that is not a whole
/// number of tiles ends mid-tile rather than being scaled to fit.
pub fn tile_quads(source: Rect, destination: Rect) -> Vec<PatchQuad> {
    let mut quads = Vec::new();
    push_tiles(&mut quads, source, destination);
    quads
}

/// How many tiles [`tile_quads`] would produce, without building them.
///
/// A caller filling a large area with a small source can check the cost before
/// paying it: each tile is one draw.
pub fn tile_count(source: Rect, destination: Rect) -> usize {
    if !usable(source) || !usable(destination) {
        return 0;
    }
    let columns = (destination.width / source.width).ceil().max(0.0) as usize;
    let rows = (destination.height / source.height).ceil().max(0.0) as usize;
    columns.saturating_mul(rows)
}

/// The draws that fill `destination` with `source` as a nine-patch.
///
/// The four corners keep their source size. The four edges grow along one axis
/// and keep their size across it. The middle grows along both. `center` and
/// `edges` decide whether the growing parts are stretched or tiled.
///
/// Returns a single stretched quad when the insets leave no middle, or when the
/// destination is too small to hold the corners — the picture stays sensible
/// instead of drawing corners on top of each other.
pub fn nine_patch_quads(
    source: Rect,
    destination: Rect,
    insets: NinePatchInsets,
    center: PatchFill,
    edges: PatchFill,
) -> Vec<PatchQuad> {
    if !usable(source) || !usable(destination) {
        return Vec::new();
    }
    let source_size = Size::new(source.width, source.height);
    let corners_fit = destination.width > insets.left + insets.right
        && destination.height > insets.top + insets.bottom;
    if !insets.fit(source_size) || !corners_fit {
        return vec![PatchQuad {
            source,
            destination,
        }];
    }

    let source_columns = [
        (source.x, insets.left),
        (
            source.x + insets.left,
            source.width - insets.left - insets.right,
        ),
        (source.x + source.width - insets.right, insets.right),
    ];
    let source_rows = [
        (source.y, insets.top),
        (
            source.y + insets.top,
            source.height - insets.top - insets.bottom,
        ),
        (source.y + source.height - insets.bottom, insets.bottom),
    ];
    let destination_columns = [
        (destination.x, insets.left),
        (
            destination.x + insets.left,
            destination.width - insets.left - insets.right,
        ),
        (
            destination.x + destination.width - insets.right,
            insets.right,
        ),
    ];
    let destination_rows = [
        (destination.y, insets.top),
        (
            destination.y + insets.top,
            destination.height - insets.top - insets.bottom,
        ),
        (
            destination.y + destination.height - insets.bottom,
            insets.bottom,
        ),
    ];

    let mut quads = Vec::new();
    for row in 0..3 {
        for column in 0..3 {
            let (source_x, source_width) = source_columns[column];
            let (source_y, source_height) = source_rows[row];
            let (destination_x, destination_width) = destination_columns[column];
            let (destination_y, destination_height) = destination_rows[row];
            if source_width <= 0.0
                || source_height <= 0.0
                || destination_width <= 0.0
                || destination_height <= 0.0
            {
                continue;
            }
            let patch_source = Rect {
                x: source_x,
                y: source_y,
                width: source_width,
                height: source_height,
            };
            let patch_destination = Rect {
                x: destination_x,
                y: destination_y,
                width: destination_width,
                height: destination_height,
            };
            let stretched = column == 1 || row == 1;
            let fill = match (column, row) {
                (1, 1) => center,
                _ if stretched => edges,
                _ => PatchFill::Stretch,
            };
            if fill.is_tiled() {
                push_tiles(&mut quads, patch_source, patch_destination);
            } else {
                quads.push(PatchQuad {
                    source: patch_source,
                    destination: patch_destination,
                });
            }
        }
    }
    quads
}

fn usable(rect: Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
        && rect.width > 0.0
        && rect.height > 0.0
}

fn push_tiles(quads: &mut Vec<PatchQuad>, source: Rect, destination: Rect) {
    if !usable(source) || !usable(destination) {
        return;
    }
    let mut y = destination.y;
    let bottom = destination.y + destination.height;
    while y < bottom {
        let height = source.height.min(bottom - y);
        let mut x = destination.x;
        let right = destination.x + destination.width;
        while x < right {
            let width = source.width.min(right - x);
            quads.push(PatchQuad {
                source: Rect {
                    x: source.x,
                    y: source.y,
                    width,
                    height,
                },
                destination: Rect {
                    x,
                    y,
                    width,
                    height,
                },
            });
            x += source.width;
        }
        y += source.height;
    }
}

#[cfg(test)]
#[path = "tests/nine_patch_tests.rs"]
mod tests;
