#![cfg(feature = "embedded-default-font")]

use ab_glyph_rasterizer::Rasterizer;
use cranpose_render_common::{
    glyph_mask::{GlyphMask, GlyphRaster, rasterize_glyph},
    text_fonts::default_typeface,
};

struct MaskCase {
    name: &'static str,
    ch: char,
    size: f32,
    raster: GlyphRaster,
    fixture: &'static [u8],
}

// Skia's fake-bold ratio at `size` device pixels per em (1/24 at <= 9 px,
// interpolating linearly to 1/32 at >= 36 px).
fn fake_bold_embolden(size: f32) -> f32 {
    let along = ((size - 9.0) / 27.0).clamp(0.0, 1.0);
    size * (1.0 / 24.0 + along * (1.0 / 32.0 - 1.0 / 24.0))
}

fn decode_fixture(bytes: &[u8]) -> GlyphMask {
    assert_eq!(&bytes[0..4], b"GMSK", "fixture magic");
    let left = i32::from_le_bytes(bytes[4..8].try_into().expect("left bytes"));
    let top = i32::from_le_bytes(bytes[8..12].try_into().expect("top bytes"));
    let width = usize::try_from(u32::from_le_bytes(
        bytes[12..16].try_into().expect("width bytes"),
    ))
    .expect("width fits usize");
    let height = usize::try_from(u32::from_le_bytes(
        bytes[16..20].try_into().expect("height bytes"),
    ))
    .expect("height fits usize");
    let alpha = &bytes[20..];
    assert_eq!(alpha.len(), width * height, "fixture payload size");
    GlyphMask {
        alpha: alpha.into(),
        width,
        height,
        left,
        top,
    }
}

fn alpha_at(mask: &GlyphMask, x: i32, y: i32) -> u8 {
    let Ok(col) = usize::try_from(x - mask.left) else {
        return 0;
    };
    let Ok(row) = usize::try_from(y - mask.top) else {
        return 0;
    };
    if col >= mask.width || row >= mask.height {
        return 0;
    }
    mask.alpha[row * mask.width + col]
}

fn inked_box(mask: &GlyphMask) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    for row in 0..mask.height {
        for col in 0..mask.width {
            if mask.alpha[row * mask.width + col] == 0 {
                continue;
            }
            let x = mask.left + i32::try_from(col).expect("col fits i32");
            let y = mask.top + i32::try_from(row).expect("row fits i32");
            bounds = Some(match bounds {
                Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
                None => (x, y, x, y),
            });
        }
    }
    bounds
}

fn compare_masks(name: &str, expected: &GlyphMask, actual: &GlyphMask, max_channel_diff: u8) {
    let min_x = expected.left.min(actual.left);
    let min_y = expected.top.min(actual.top);
    let max_x = (expected.left + i32::try_from(expected.width).expect("width fits i32"))
        .max(actual.left + i32::try_from(actual.width).expect("width fits i32"));
    let max_y = (expected.top + i32::try_from(expected.height).expect("height fits i32"))
        .max(actual.top + i32::try_from(actual.height).expect("height fits i32"));

    let mut worst_diff = 0u8;
    let mut worst_at = (0, 0, 0u8, 0u8);
    for y in min_y..max_y {
        for x in min_x..max_x {
            let old = alpha_at(expected, x, y);
            let new = alpha_at(actual, x, y);
            let diff = old.abs_diff(new);
            if diff > worst_diff {
                worst_diff = diff;
                worst_at = (x, y, old, new);
            }
        }
    }

    let (el, et, er, eb) = inked_box(expected).expect("expected mask has inked pixels");
    let (al, at, ar, ab) = inked_box(actual).expect("actual mask has inked pixels");
    let overhang = (el - al).max(et - at).max(ar - er).max(ab - eb).max(0);

    assert!(
        overhang <= 1,
        "{name}: inked box grew by {overhang} px (expected {el}..={er}, {et}..={eb}; actual {al}..={ar}, {at}..={ab})"
    );
    assert!(
        worst_diff <= max_channel_diff,
        "{name}: max pixel diff {worst_diff} exceeds bound {max_channel_diff} at ({}, {}): expected {}, got {}",
        worst_at.0,
        worst_at.1,
        worst_at.2,
        worst_at.3
    );
}

#[test]
fn stroked_and_fake_bold_glyph_masks_match_pinned_references() {
    let typeface = default_typeface().expect("embedded default typeface");
    let cases = [
        MaskCase {
            name: "stroke_18",
            ch: 'R',
            size: 18.0,
            raster: GlyphRaster {
                stroke: Some(2.0),
                embolden: 0.0,
                skew: 0.0,
            },
            fixture: include_bytes!("fixtures/glyph_masks/stroke_18.bin"),
        },
        MaskCase {
            name: "stroke_48",
            ch: 'R',
            size: 48.0,
            raster: GlyphRaster {
                stroke: Some(3.8),
                embolden: 0.0,
                skew: 0.0,
            },
            fixture: include_bytes!("fixtures/glyph_masks/stroke_48.bin"),
        },
        MaskCase {
            name: "fake_bold_18",
            ch: 'R',
            size: 18.0,
            raster: GlyphRaster {
                stroke: None,
                embolden: fake_bold_embolden(18.0),
                skew: 0.0,
            },
            fixture: include_bytes!("fixtures/glyph_masks/fake_bold_18.bin"),
        },
        MaskCase {
            name: "fake_bold_48",
            ch: 'R',
            size: 48.0,
            raster: GlyphRaster {
                stroke: None,
                embolden: fake_bold_embolden(48.0),
                skew: 0.0,
            },
            fixture: include_bytes!("fixtures/glyph_masks/fake_bold_48.bin"),
        },
    ];

    let mut rasterizer = Rasterizer::new(0, 0);
    for case in cases {
        let glyph = typeface.glyph(case.ch).expect("glyph present");
        let actual = rasterize_glyph(&typeface, glyph, case.size, case.raster, &mut rasterizer)
            .expect("glyph produces mask");
        let expected = decode_fixture(case.fixture);
        compare_masks(case.name, &expected, &actual, 0);
    }
}
