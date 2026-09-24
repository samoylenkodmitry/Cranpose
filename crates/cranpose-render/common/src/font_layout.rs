use ab_glyph::{Font, Glyph, OutlinedGlyph, Point, PxScale, ScaleFont, point};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontVerticalMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub natural_line_height: f32,
    /// `hhea.lineGap` at this size. Read only by a style that asks for
    /// Android's `includeFontPadding`; every other line box ignores it.
    pub line_gap: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphPixelBounds {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl GlyphPixelBounds {
    pub fn width(self) -> usize {
        self.max_x.saturating_sub(self.min_x) as usize
    }

    pub fn height(self) -> usize {
        self.max_y.saturating_sub(self.min_y) as usize
    }
}

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

pub fn line_advance_width(font: &impl Font, text: &str, font_size: f32) -> f32 {
    let scale = PxScale::from(font_size);
    let scaled_font = font.as_scaled(scale);
    let mut width = 0.0;
    let mut previous = None;

    for ch in text.chars() {
        let glyph_id = scaled_font.glyph_id(ch);
        if let Some(previous_id) = previous {
            width += scaled_font.kern(previous_id, glyph_id);
        }
        width += scaled_font.h_advance(glyph_id);
        previous = Some(glyph_id);
    }

    width.max(0.0)
}

pub fn align_glyph_to_pixel_grid(mut glyph: Glyph, static_text_motion: bool) -> Glyph {
    if static_text_motion {
        glyph.position.x = glyph.position.x.round();
        glyph.position.y = glyph.position.y.round();
    }
    glyph
}

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
