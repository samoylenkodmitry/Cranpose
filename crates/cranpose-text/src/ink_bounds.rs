use std::sync::OnceLock;

use cranpose_ui_graphics::Rect;
use read_fonts::types::GlyphId as FontGlyphId;

use crate::text_layout::TextLayout;

/// Where the glyphs of a [`TextLayout`] put ink, as their outlines give it,
/// in pixels from the layout's top left corner: before a renderer strokes,
/// emboldens or slants them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkBounds {
    /// The box around the ink of every glyph.
    pub rect: Rect,
    /// The farthest the ink of a glyph reaches above the glyph's baseline.
    pub rise: f32,
    /// The farthest the ink of a glyph reaches below the glyph's baseline.
    pub drop: f32,
}

impl InkBounds {
    fn union(self, other: Self) -> Self {
        Self {
            rect: self.rect.union(other.rect),
            rise: self.rise.max(other.rise),
            drop: self.drop.max(other.drop),
        }
    }
}

// The ink of a layout's body, found on first use: a renderer asks for it,
// and a text laid out each frame pays for it only where it draws. A copy of
// a body starts without it, as a body is copied to place its lines anew,
// and bodies compare equal whatever they found.
#[derive(Debug, Default)]
pub(crate) struct InkCache(OnceLock<Option<InkBounds>>);

impl Clone for InkCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for InkCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl InkCache {
    // The ink of `layout`, whose body holds this cache.
    pub(crate) fn of(&self, layout: &TextLayout) -> Option<InkBounds> {
        *self.0.get_or_init(|| find(layout))
    }
}

// The ink of every glyph of `layout`; `None` when no glyph has any.
fn find(layout: &TextLayout) -> Option<InkBounds> {
    let mut ink: Option<InkBounds> = None;
    for line in 0..layout.line_count() {
        for run in layout.glyph_runs(line) {
            let metrics = run.typeface.font().glyph_metrics();
            let scale = run.size / f32::from(run.typeface.units_per_em());
            for glyph in run.positions() {
                let Some(extents) = metrics.extents(FontGlyphId::new(glyph.id.0)) else {
                    continue;
                };
                if extents.width <= 0.0 || extents.height <= 0.0 {
                    continue;
                }
                let rise = extents.y_bearing * scale;
                let glyph_ink = InkBounds {
                    rect: Rect {
                        x: glyph.x + extents.x_bearing * scale,
                        y: glyph.y - rise,
                        width: extents.width * scale,
                        height: extents.height * scale,
                    },
                    rise,
                    drop: (extents.height - extents.y_bearing) * scale,
                };
                ink = Some(ink.map_or(glyph_ink, |ink| ink.union(glyph_ink)));
            }
        }
    }
    ink
}
