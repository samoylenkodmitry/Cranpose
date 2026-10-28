use ab_glyph::{Font as _, FontRef, PxScale, point};
use cranpose_text::{GlyphOutline, OutlineTransform};

use crate::fonts::{DEVANAGARI_VF, ROBOTO, bytes, typeface};

// A whole pixel count or position as a float.
fn float(value: i32) -> f32 {
    f32::from(i16::try_from(value).expect("a glyph box is small"))
}

fn index(x: u32, y: u32, width: u32) -> usize {
    usize::try_from(y * width + x).expect("a glyph box is small")
}

#[test]
fn outlines_draw_the_coverage_ab_glyph_draws() {
    for (path, text) in [(ROBOTO, "agQ@"), (DEVANAGARI_VF, "कष")] {
        let face = typeface(path);
        let font_bytes = bytes(path);
        let font = FontRef::try_from_slice(&font_bytes).expect("ab_glyph reads the font");
        let units = font.units_per_em().expect("the font has an em");
        let mut rasterizer = ab_glyph_rasterizer::Rasterizer::new(0, 0);
        for ch in text.chars() {
            for (size, offset) in [(12.5, 0.0), (31.0, 0.25)] {
                let glyph = face.glyph(ch).expect("the font maps the character");
                let outline = GlyphOutline::of(
                    &face,
                    glyph,
                    OutlineTransform {
                        size,
                        x_offset: offset,
                        y_offset: offset,
                        skew: 0.0,
                    },
                )
                .expect("the glyph has an outline");
                let scale = PxScale::from(size * font.height_unscaled() / units);
                let expected_glyph = font
                    .glyph_id(ch)
                    .with_scale_and_position(scale, point(offset, offset));
                let expected = font
                    .outline_glyph(expected_glyph)
                    .expect("ab_glyph outlines it");
                let bounds = expected.px_bounds();
                let width = outline.width();
                assert_eq!(
                    (
                        float(outline.left()),
                        float(outline.top()),
                        float(i32::try_from(width).expect("small")),
                        float(i32::try_from(outline.height()).expect("small")),
                    ),
                    (bounds.min.x, bounds.min.y, bounds.width(), bounds.height()),
                    "{ch} at {size}"
                );
                let cells = index(0, outline.height(), width);
                let (mut ours, mut theirs) = (vec![0.0; cells], vec![0.0; cells]);
                outline.draw(&mut rasterizer, |x, y, value| {
                    ours[index(x, y, width)] = value;
                });
                expected.draw(|x, y, value| theirs[index(x, y, width)] = value);
                let worst = ours
                    .iter()
                    .zip(&theirs)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max);
                assert!(worst < 1e-4, "{ch} at {size}: {worst}");
            }
        }
    }
}

#[test]
fn a_space_has_no_outline() {
    let face = typeface(ROBOTO);
    let space = face.glyph(' ').expect("the font maps a space");

    assert!(
        GlyphOutline::of(
            &face,
            space,
            OutlineTransform {
                size: 16.0,
                ..OutlineTransform::default()
            }
        )
        .is_none()
    );
}
