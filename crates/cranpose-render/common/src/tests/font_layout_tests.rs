use ab_glyph::FontRef;

use super::*;

fn test_font() -> FontRef<'static> {
    FontRef::try_from_slice(include_bytes!("../../assets/NotoSansMerged.ttf")).expect("font")
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
