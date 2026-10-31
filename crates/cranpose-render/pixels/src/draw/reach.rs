//! Where the items of a raster scene reach: the pixels each can draw in a
//! window, and whether one paints the whole window over.

use cranpose_ui_graphics::{Rect, whole};

use super::{
    ClipBounds, PlacedShape, RenderItem, ShapeCoverage, clip_rect_to_window, image_rect,
    paints_opaque, place_shape, placed_shadow, snap_delta_for_anchor, text_raster_rect,
};
use crate::{
    scene::{RasterScene, TextDraw},
    scene_item::ItemRef,
    style::point_in_resolved_rounded_rect,
};

impl RasterScene {
    /// The pixels `item` can reach before its clip, for the items drawn into
    /// a frame directly; text reaches what its rasterized image covers.
    pub(crate) fn item_reach(&self, item: RenderItem) -> Option<Rect> {
        match item {
            RenderItem::Shape(index) => {
                let placed = place_shape(&self.shapes[index])?;
                Some(placed.coverage.reach(placed.rect))
            }
            RenderItem::Image(index) => Some(image_rect(&self.images[index])),
            RenderItem::Shadow(index) => {
                let draw = &self.shadows[index];
                let snap = draw
                    .snap_anchor
                    .map(snap_delta_for_anchor)
                    .unwrap_or_default();
                Some(draw.shadow.bounds.translate(snap.x, snap.y))
            }
            RenderItem::Text(_) => None,
        }
    }

    /// Whether `item` paints every pixel of `window` with opaque bytes: an
    /// opaque solid fill whose clipped bounds hold the window and, when
    /// rounded, whose shape holds the window's corner pixels, and so every
    /// pixel between them, the shape being convex.
    pub(super) fn covers_window(&self, item: RenderItem, window: ClipBounds) -> bool {
        let RenderItem::Shape(index) = item else {
            return false;
        };
        let draw = &self.shapes[index];
        if !paints_opaque(&draw.brush, draw.blend_mode) {
            return false;
        }
        let Some(PlacedShape {
            coverage: ShapeCoverage::Fill { rect, radii },
            ..
        }) = place_shape(draw)
        else {
            return false;
        };
        if clip_rect_to_window(rect, draw.clip, window) != Some(window) {
            return false;
        }
        radii.is_none_or(|radii| {
            let (left, right) = (window.min_x as f32 + 0.5, window.max_x as f32 - 0.5);
            let (top, bottom) = (window.min_y as f32 + 0.5, window.max_y as f32 - 0.5);
            // Between the corners on either axis, every pixel the rect covers
            // lies inside it: no corner needs a test.
            let between_columns = left >= rect.x + radii.top_left.max(radii.bottom_left)
                && right <= rect.x + rect.width - radii.top_right.max(radii.bottom_right);
            let between_rows = top >= rect.y + radii.top_left.max(radii.top_right)
                && bottom <= rect.y + rect.height - radii.bottom_left.max(radii.bottom_right);
            between_columns
                || between_rows
                || [(left, top), (right, top), (left, bottom), (right, bottom)]
                    .into_iter()
                    .all(|(x, y)| point_in_resolved_rounded_rect(x, y, rect, &radii))
        })
    }

    /// Puts into `reach` where each item of the order can draw in `window`,
    /// in order: what a host that draws a picture in strips would otherwise
    /// work out again for every strip.
    pub(crate) fn reach_into(&self, window: ClipBounds, reach: &mut Vec<Option<ClipBounds>>) {
        reach.clear();
        reach.extend(
            self.order
                .iter()
                .map(|&item| self.item_bounds(item, window)),
        );
    }

    pub(crate) fn item_bounds(&self, item: RenderItem, window: ClipBounds) -> Option<ClipBounds> {
        self.item(item).bounds(window)
    }
}

impl ItemRef<'_> {
    // The pixels of `window` the item can reach.
    pub(crate) fn bounds(self, window: ClipBounds) -> Option<ClipBounds> {
        match self {
            ItemRef::Shape(draw) => {
                let placed = place_shape(draw)?;
                clip_rect_to_window(placed.coverage.reach(placed.rect), draw.clip, window)
            }
            ItemRef::Image(draw) => {
                if draw.alpha <= 0.0 {
                    return None;
                }
                clip_rect_to_window(image_rect(draw), draw.clip, window)
            }
            ItemRef::Shadow(draw) => {
                let (shadow, clip) = placed_shadow(draw);
                clip_rect_to_window(shadow.bounds, clip, window)
            }
            ItemRef::Text(draw) => text_bounds(draw, window),
        }
    }
}

/// The pixels of `window` that the rasterized `draw` can reach.
pub(super) fn text_bounds(draw: &TextDraw, window: ClipBounds) -> Option<ClipBounds> {
    let lines = draw.glyph_reach(text_raster_rect(draw)?);
    let reach = Rect {
        width: whole::ceil(lines.width).max(1.0),
        height: whole::ceil(lines.height).max(1.0),
        ..lines
    };
    clip_rect_to_window(reach, draw.clip, window)
}
