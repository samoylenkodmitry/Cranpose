//! A laid-out text's glyphs for a renderer that draws solid glyphs from an
//! atlas of their masks.

use cranpose_ui::Color;
use cranpose_ui_graphics::whole;

use crate::{
    glyph_mask::{GlyphMaskCache, GlyphMaskKey},
    text_glyphs::{SpanPaints, visit_placed_glyphs},
    text_raster::TextRasterRequest,
};

/// Where a glyph whose mask an atlas holds draws, and in which color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasGlyph {
    /// The key of the glyph's mask.
    pub key: GlyphMaskKey,
    /// Where the mask's left edge sits, in device pixels from the text's
    /// top left corner.
    pub x: i32,
    /// Where the mask's top edge sits.
    pub y: i32,
    /// The mask's width in pixels.
    pub width: usize,
    /// The mask's height in pixels.
    pub height: usize,
    /// The color the glyph draws in.
    pub color: Color,
}

/// Appends to `out` each glyph of `request` that puts pixels down, its mask
/// in `glyph_cache`, placed from the text's top left corner. Returns `None`,
/// appending nothing, for a text that cannot draw from an atlas: one in
/// motion, or with a gradient, a shadow or a stroke.
pub fn collect_solid_text_atlas_run(
    request: TextRasterRequest<'_>,
    glyph_cache: &mut GlyphMaskCache,
    out: &mut Vec<AtlasGlyph>,
) -> Option<()> {
    if request.is_degenerate() {
        return Some(());
    }
    let paints = SpanPaints::of(request.text, request.style, request.fallback_color);
    if !request.static_motion()
        || !paints.all(|paint| {
            paint.solid_color().is_some() && paint.shadow.is_none() && paint.stroke.is_none()
        })
    {
        return None;
    }
    let origin = request.origin;
    visit_placed_glyphs(
        request.layout,
        &paints,
        (origin, request.scale),
        true,
        |glyph| {
            let Some(color) = glyph.paint.solid_color() else {
                return;
            };
            let key = GlyphMaskKey::of(glyph.typeface, glyph.glyph, glyph.size, glyph.raster);
            let Some(mask) = glyph_cache.mask(glyph.typeface, glyph.glyph, glyph.size, glyph.raster)
            else {
                return;
            };
            out.push(AtlasGlyph {
                key,
                x: whole::to_i32(whole::round(glyph.x - origin.x)) + mask.left,
                y: whole::to_i32(whole::round(glyph.y - origin.y)) + mask.top,
                width: mask.width,
                height: mask.height,
                color,
            });
        },
    );
    Some(())
}
