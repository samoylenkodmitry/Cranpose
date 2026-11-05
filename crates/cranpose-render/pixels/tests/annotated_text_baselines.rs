#[path = "../../common/tests/support/annotated_text.rs"]
mod annotated_fixture;

use cranpose_render_pixels::{Scene, draw_scene};
use cranpose_ui::text::{
    LineHeightAlignment, LineHeightStyle, LineHeightTrim, SpanStyle, TextStyle, TextUnit,
};
use cranpose_ui_graphics::Rect;

#[test]
fn pixels_renderer_aligns_small_and_large_spans_on_each_visible_line() {
    for compact in [false, true] {
        let mut style = TextStyle::from_span_style(SpanStyle {
            font_size: TextUnit::Sp(10.0),
            ..Default::default()
        });
        if compact {
            style.paragraph_style.line_height = TextUnit::Sp(14.0);
            style.paragraph_style.line_height_style = Some(LineHeightStyle {
                alignment: LineHeightAlignment::Center,
                trim: LineHeightTrim::None,
                ..Default::default()
            });
        }
        let mut scene = Scene::new();
        scene.replace_graph(annotated_fixture::text_graph(
            annotated_fixture::mixed_size_pairs(),
            style,
            Rect {
                x: 4.0,
                y: 4.0,
                width: 96.0,
                height: 90.0,
            },
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        ));
        let mut pixels = vec![0; 100 * 100 * 4];
        cranpose_ui::AppContext::new().enter(|| draw_scene(&mut pixels, 100, 100, &scene));
        let bottoms = |channel: usize| {
            let mut bottoms = Vec::new();
            for (y, row) in pixels.as_chunks::<400>().0.iter().enumerate() {
                if row
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[channel] > 64 && p[channel] > p[1 - channel].saturating_mul(2))
                {
                    if bottoms.last().is_some_and(|previous| *previous == y - 1) {
                        bottoms.pop();
                    }
                    bottoms.push(y);
                }
            }
            bottoms
        };
        let small = bottoms(0);
        let large = bottoms(1);
        if compact {
            let small = small.last().expect("small last-line glyph");
            let large = large.last().expect("large last-line glyph");
            assert!(
                small.abs_diff(*large) <= 1,
                "short line boxes may not cut off glyphs: small={small}, large={large}"
            );
        } else {
            assert_eq!(small.len(), 2, "both small glyphs are visible: {small:?}");
            assert_eq!(large.len(), 2, "both large glyphs are visible: {large:?}");
            for (small, large) in small.into_iter().zip(large) {
                assert!(
                    small.abs_diff(large) <= 1,
                    "small baseline={small}, large baseline={large}"
                );
            }
        }
    }
}
