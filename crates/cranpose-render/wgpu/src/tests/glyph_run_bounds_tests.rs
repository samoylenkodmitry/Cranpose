use cranpose_ui_graphics::Color;

use super::*;

fn placement(x: i32, y: i32, width: usize, height: usize) -> SoftwareGlyphAtlasPlacement {
    SoftwareGlyphAtlasPlacement {
        key: SoftwareGlyphAtlasKey {
            font_hash: 0,
            glyph_id: 0,
            scale_x_bits: 0,
            scale_y_bits: 0,
            embolden_px_bits: 0,
            slant_bits: 0,
        },
        x,
        y,
        width,
        height,
        color: Color(1.0, 1.0, 1.0, 1.0),
    }
}

fn viewport(width: u32, height: u32, transform: SegmentTransform) -> ViewportUniformParams {
    ViewportUniformParams {
        width,
        height,
        offset: [0.0; 2],
        transform,
        origin: [0.0; 2],
        depth_base: 0.0,
    }
}

fn at(x: f32, y: f32) -> Rect {
    Rect {
        x,
        y,
        width: 40.0,
        height: 20.0,
    }
}

/// A line of glyphs, one of them rising above the others and one a space.
fn line() -> Vec<SoftwareGlyphAtlasPlacement> {
    vec![
        placement(0, 4, 6, 8),
        placement(7, -2, 5, 14),
        placement(13, 4, 0, 0),
        placement(14, 5, 6, 7),
    ]
}

#[test]
fn a_runs_bounds_are_the_box_its_glyphs_cover() {
    assert_eq!(
        GlyphRunBounds::of(line()),
        GlyphRunBounds {
            min: [0, -2],
            max: [20, 12],
        }
    );
}

#[test]
fn a_run_lies_within_a_viewport_only_while_every_glyph_does() {
    let bounds = GlyphRunBounds::of(line());
    let view = viewport(100, 50, SegmentTransform::IDENTITY);
    assert!(bounds.within_viewport(at(10.0, 10.0), None, view, 1.0));
    // The tall glyph reaches above the viewport's top.
    assert!(!bounds.within_viewport(at(10.0, 1.0), None, view, 1.0));
    // The last glyph reaches past its right edge.
    assert!(!bounds.within_viewport(at(81.0, 10.0), None, view, 1.0));
    // A clip cuts the run.
    let clip = Rect {
        x: 0.0,
        y: 0.0,
        width: 25.0,
        height: 50.0,
    };
    assert!(!bounds.within_viewport(at(10.0, 10.0), Some(clip), view, 1.0));
    // At twice the scale the viewport shows half as much of the scene, and
    // a raster rect sits at half its logical place.
    assert!(bounds.within_viewport(at(40.0, 20.0), None, view, 2.0));
    assert!(!bounds.within_viewport(at(81.0, 20.0), None, view, 2.0));
    assert!(!bounds.within_viewport(at(10.0, 10.0), None, view, 0.0));
}

#[test]
fn an_empty_run_never_skips_the_per_glyph_test() {
    let view = viewport(100, 50, SegmentTransform::IDENTITY);
    assert!(!GlyphRunBounds::of(std::iter::empty()).within_viewport(
        at(10.0, 10.0),
        None,
        view,
        1.0
    ));
}

#[test]
fn a_run_draws_the_same_glyphs_whether_or_not_its_bounds_answer_for_them() {
    let glyphs = line();
    let entries: Vec<GlyphAtlasEntry> = glyphs
        .iter()
        .map(|_| GlyphAtlasEntry {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        })
        .collect();
    let run = RunGlyphs::of(glyphs.iter().copied(), &mut RunGlyphScratch::default())
        .expect("the line has a compact form");
    let quads = |bounds| GlyphRunQuads {
        glyphs: &run,
        entries: &entries,
        texel: 1.0 / 64.0,
        bounds,
    };
    let unknown = GlyphRunBounds::of(std::iter::empty());
    let clips = [
        None,
        Some(Rect {
            x: 5.0,
            y: 3.0,
            width: 70.0,
            height: 30.0,
        }),
    ];
    let turned = SegmentTransform::affine([0.0, -1.0, 1.0, 0.0], [100.0, 0.0])
        .expect("a turn is invertible");
    for transform in [SegmentTransform::IDENTITY, turned] {
        let view = viewport(100, 50, transform);
        for clip in clips {
            for step in -30..60 {
                let raster = at(step as f32 * 2.5, step as f32 * 0.75 - 5.0);
                let sink = |bounds| {
                    let mut drawn = Vec::new();
                    for_each_visible_glyph(
                        raster,
                        quads(bounds),
                        (clip, None),
                        view,
                        1.0,
                        |glyph| {
                            drawn.push(glyph.rect);
                        },
                    );
                    drawn
                };
                assert_eq!(
                    sink(GlyphRunBounds::of(glyphs.iter().copied())),
                    sink(unknown),
                    "raster {raster:?}, clip {clip:?}"
                );
            }
        }
    }
}
