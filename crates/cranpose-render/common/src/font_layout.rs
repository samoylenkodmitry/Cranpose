use ab_glyph::{point, Font, Glyph, OutlinedGlyph, Point, PxScale, ScaleFont};

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
mod tests {
    use ab_glyph::FontRef;

    use super::*;

    fn test_font() -> FontRef<'static> {
        FontRef::try_from_slice(include_bytes!("../assets/NotoSansMerged.ttf")).expect("font")
    }

    #[test]
    fn vertical_metrics_return_positive_line_height() {
        let metrics = vertical_metrics(&test_font(), 24.0);
        assert!(metrics.ascent > 0.0);
        assert!(metrics.descent < 0.0);
        assert!(metrics.natural_line_height >= (metrics.ascent - metrics.descent));
    }

    #[test]
    fn layout_line_glyphs_starts_at_origin() {
        let font = test_font();
        let origin = point(5.0, 13.0);
        let glyphs = layout_line_glyphs(&font, "AV", 18.0, origin);
        assert_eq!(glyphs.len(), 2);
        assert_eq!(glyphs[0].position, origin);
        assert!(glyphs[1].position.x > glyphs[0].position.x);
    }

    #[test]
    fn align_glyph_to_pixel_grid_only_changes_static_positions() {
        let font = test_font();
        let glyph = layout_line_glyphs(&font, "A", 17.0, point(0.0, 13.37))
            .into_iter()
            .next()
            .expect("glyph");
        let snapped = align_glyph_to_pixel_grid(glyph.clone(), true);
        let unchanged = align_glyph_to_pixel_grid(glyph, false);

        assert_eq!(snapped.position.x, snapped.position.x.round());
        assert_eq!(snapped.position.y, snapped.position.y.round());
        assert!((unchanged.position.y - 13.37).abs() < 1e-3);
    }

    #[test]
    fn line_advance_width_uses_font_advances() {
        let font = test_font();
        let width = line_advance_width(&font, "Counter App", 18.0);
        assert!(width > 0.0);
    }

    #[test]
    fn glyph_pixel_bounds_reports_positive_size() {
        let font = test_font();
        let glyph = layout_line_glyphs(&font, "A", 24.0, point(0.0, 20.0))
            .into_iter()
            .next()
            .expect("glyph");
        let bounds = glyph_pixel_bounds(&font, &glyph).expect("bounds");

        assert!(bounds.width() > 0);
        assert!(bounds.height() > 0);
    }
}
