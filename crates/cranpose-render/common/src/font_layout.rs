use ab_glyph::{Font, Glyph, OutlinedGlyph, Point, PxScale, ScaleFont, point};

/// Vertical metrics of a font at one pixel size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontVerticalMetrics {
    /// Distance from the baseline to the top of the line in pixels.
    pub ascent: f32,
    /// Distance from the baseline to the bottom of the line in pixels, negative below the baseline.
    pub descent: f32,
    /// Ascent minus descent, rounded up to a whole pixel.
    pub natural_line_height: f32,
    /// `hhea.lineGap` at this size. Read only by a style that asks for
    /// Android's `includeFontPadding`; every other line box ignores it.
    pub line_gap: f32,
}

/// Integer pixel box that a rasterized glyph covers, relative to the glyph origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphPixelBounds {
    /// Left edge of the box in pixels.
    pub min_x: i32,
    /// Top edge of the box in pixels.
    pub min_y: i32,
    /// Right edge of the box in pixels, exclusive.
    pub max_x: i32,
    /// Bottom edge of the box in pixels, exclusive.
    pub max_y: i32,
}

impl GlyphPixelBounds {
    /// Box width in pixels, zero when the box is empty.
    pub fn width(self) -> usize {
        self.max_x.saturating_sub(self.min_x) as usize
    }

    /// Box height in pixels, zero when the box is empty.
    pub fn height(self) -> usize {
        self.max_y.saturating_sub(self.min_y) as usize
    }
}

/// Returns the vertical metrics of `font` at `font_size` pixels.
pub fn vertical_metrics(font: &impl Font, font_size: f32) -> FontVerticalMetrics {
    let scaled_font = font.as_scaled(PxScale::from(font_size));
    let ascent = scaled_font.ascent();
    let descent = scaled_font.descent();
    FontVerticalMetrics {
        ascent,
        descent,
        natural_line_height: (ascent - descent).ceil(),
        line_gap: scaled_font.line_gap(),
    }
}

/// Lays out `text` as one line of glyphs, applying kerning. `font_size` is in pixels and
/// `origin` is the position of the first glyph on the baseline.
pub fn layout_line_glyphs(
    font: &impl Font,
    text: &str,
    font_size: f32,
    origin: Point,
) -> Vec<Glyph> {
    let scale = PxScale::from(font_size);
    let scaled_font = font.as_scaled(scale);
    let mut caret_x = origin.x;
    let mut previous = None;
    let mut glyphs = Vec::with_capacity(text.chars().count());

    for ch in text.chars() {
        let glyph_id = scaled_font.glyph_id(ch);
        if let Some(previous_id) = previous {
            caret_x += scaled_font.kern(previous_id, glyph_id);
        }
        glyphs.push(glyph_id.with_scale_and_position(scale, point(caret_x, origin.y)));
        caret_x += scaled_font.h_advance(glyph_id);
        previous = Some(glyph_id);
    }

    glyphs
}

/// Returns the advance width of `text` as one line at `font_size` pixels, including kerning.
pub fn line_advance_width(font: &impl Font, text: &str, font_size: f32) -> f32 {
    // The scale reads the face's ascender and descender: once a line, not
    // once a glyph.
    let h_scale = font.as_scaled(PxScale::from(font_size)).h_scale_factor();
    let mut width = 0.0;
    let mut previous = None;

    for ch in text.chars() {
        let glyph_id = font.glyph_id(ch);
        if let Some(previous_id) = previous {
            width += font.kern_unscaled(previous_id, glyph_id) * h_scale;
        }
        width += font.h_advance_unscaled(glyph_id) * h_scale;
        previous = Some(glyph_id);
    }

    width.max(0.0)
}

/// Rounds the glyph position to whole pixels when `static_text_motion` is true. Otherwise the
/// glyph is returned unchanged.
pub fn align_glyph_to_pixel_grid(mut glyph: Glyph, static_text_motion: bool) -> Glyph {
    if static_text_motion {
        glyph.position.x = cranpose_ui_graphics::whole::round(glyph.position.x);
        glyph.position.y = cranpose_ui_graphics::whole::round(glyph.position.y);
    }
    glyph
}

/// Returns the pixel box that `glyph` covers once outlined, or `None` when it has no outline
/// (for example a space).
pub fn glyph_pixel_bounds(font: &impl Font, glyph: &Glyph) -> Option<GlyphPixelBounds> {
    let outlined = font.outline_glyph(glyph.clone())?;
    Some(pixel_bounds_from_outlined(&outlined))
}

pub(crate) fn pixel_bounds_from_outlined(outlined: &OutlinedGlyph) -> GlyphPixelBounds {
    let bounds = outlined.px_bounds();
    GlyphPixelBounds {
        min_x: bounds.min.x as i32,
        min_y: bounds.min.y as i32,
        max_x: bounds.max.x as i32,
        max_y: bounds.max.y as i32,
    }
}

#[cfg(test)]
#[path = "tests/font_layout_tests.rs"]
mod tests;
