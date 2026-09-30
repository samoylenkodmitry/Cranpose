#[path = "../../common/tests/support/annotated_text.rs"]
mod annotated_fixture;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_ui::{
    Box, BoxSpec, Color, Modifier, Text, TextStyle,
    text::{
        AnnotatedString, LineHeightAlignment, LineHeightStyle, LineHeightTrim, RangeStyle,
        SpanStyle, TextMotion, TextUnit,
    },
};
use cranpose_ui_graphics::Rect;

use crate::support;

#[test]
fn translating_cached_gradient_text_matches_a_fresh_render_at_its_new_position() {
    for annotated in [false, true] {
        let graph = |x| {
            let mut text = AnnotatedString::from("HH");
            if annotated {
                text.span_styles.push(RangeStyle {
                    range: 1..2,
                    item: SpanStyle {
                        font_size: TextUnit::Sp(30.0),
                        ..Default::default()
                    },
                });
            }
            let style = TextStyle::from_span_style(SpanStyle {
                font_size: TextUnit::Sp(20.0),
                brush: Some(cranpose_ui_graphics::Brush::linear_gradient(
                    (0..17)
                        .map(|index| {
                            let t = index as f32 / 16.0;
                            Color(0.2 + 0.5 * t, 0.3 + 0.3 * t, 0.4 + 0.1 * t, 1.0)
                        })
                        .collect(),
                )),
                ..Default::default()
            });
            annotated_fixture::text_graph(
                text,
                style,
                Rect {
                    x,
                    y: x,
                    width: 64.0,
                    height: 60.0,
                },
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 80.0,
                    height: 80.0,
                },
            )
        };
        let warm = {
            let mut renderer = match support::headless_renderer() {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("headless WGPU unavailable: {error}");
                    return;
                }
            };
            support::capture_graph(&mut renderer, graph(4.0), 80, 80);
            support::capture_graph(&mut renderer, graph(5.0), 80, 80)
        };
        let mut fresh = support::headless_renderer().expect("headless renderer remains available");
        let cold = support::capture_graph(&mut fresh, graph(5.0), 80, 80);
        assert!(
            cold.pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel != &cold.pixels[..4]),
            "the reference frame contains visible gradient glyphs"
        );
        assert_eq!(
            warm.pixels, cold.pixels,
            "translated gradient text must not depend on image cache history (annotated={annotated})"
        );
    }
}

#[test]
fn clipping_variable_height_lines_preserves_the_visible_pixels() {
    assert_clipped_paragraph_pixels(17, true);
}

#[test]
fn clipping_shadowed_text_effects_preserves_the_visible_pixels() {
    assert_clipped_paragraph_pixels(2, false);
}

fn assert_clipped_paragraph_pixels(color_stops: usize, shadow_on_span: bool) {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!("headless WGPU unavailable: {error}");
            return;
        }
    };
    let mut text = AnnotatedString::from("H\nH\nH\nH\nH\nH\nH\nH\nH\nH");
    text.span_styles.push(RangeStyle {
        range: 0..1,
        item: SpanStyle {
            font_size: TextUnit::Sp(40.0),
            ..Default::default()
        },
    });
    for height_and_trim in [
        None,
        Some((14.0, LineHeightTrim::None)),
        Some((32.0, LineHeightTrim::Both)),
    ] {
        for (varying, shadowed_stroke) in [(false, false), (true, false), (false, true)] {
            let mut style = TextStyle::from_span_style(SpanStyle {
                font_size: TextUnit::Sp(10.0),
                brush: Some(cranpose_ui_graphics::Brush::linear_gradient(
                    (0..color_stops)
                        .map(|index| {
                            let t = index as f32 / (color_stops - 1) as f32;
                            if varying {
                                Color(1.0 - t, 0.0, t, 1.0)
                            } else {
                                Color::WHITE
                            }
                        })
                        .collect(),
                )),
                ..Default::default()
            });
            if shadowed_stroke {
                style.span_style.draw_style =
                    Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 2.0 });
                style.span_style.shadow = (!shadow_on_span).then_some(cranpose_ui::text::Shadow {
                    color: Color::WHITE,
                    offset: cranpose_ui_graphics::Point::new(-3.25, 5.75),
                    blur_radius: 4.0,
                });
                style.span_style.font_weight = Some(cranpose_ui::text::FontWeight::BLACK);
                style.span_style.font_style = Some(cranpose_ui::text::FontStyle::Italic);
            }
            if let Some((height, trim)) = height_and_trim {
                style.paragraph_style.line_height = TextUnit::Sp(height);
                style.paragraph_style.line_height_style = Some(LineHeightStyle {
                    alignment: LineHeightAlignment::Center,
                    trim,
                    ..Default::default()
                });
            }
            let graph = |y, height| {
                let mut styled_text = text.clone();
                if shadowed_stroke && shadow_on_span {
                    styled_text.span_styles.push(RangeStyle {
                        range: 0..styled_text.text.len(),
                        item: SpanStyle {
                            shadow: Some(cranpose_ui::text::Shadow {
                                color: Color::WHITE,
                                offset: cranpose_ui_graphics::Point::new(-3.25, 5.75),
                                blur_radius: 4.0,
                            }),
                            ..Default::default()
                        },
                    });
                }
                annotated_fixture::text_graph(
                    styled_text,
                    style.clone(),
                    Rect {
                        x: 4.0,
                        y,
                        width: 64.0,
                        height: 360.0,
                    },
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 80.0,
                        height,
                    },
                )
            };
            let whole = support::capture_graph(&mut renderer, graph(0.0, 400.0), 80, 400);
            drop(renderer);
            renderer = support::headless_renderer().expect("headless renderer remains available");
            for offset in [61.0, 82.0] {
                let clipped = support::capture_graph(&mut renderer, graph(-offset, 30.0), 80, 30);
                let reference = support::region_pixels(
                    &whole,
                    Rect {
                        x: 0.0,
                        y: offset,
                        width: 80.0,
                        height: 30.0,
                    },
                );
                assert!(
                    reference.as_chunks::<4>().0.iter().any(|p| p[0] > 128),
                    "the clipped region contains visible glyphs"
                );
                if !varying {
                    assert_eq!(
                        clipped.pixels, reference,
                        "clipping preserves exact glyph coverage: {height_and_trim:?}, shadowed_stroke={shadowed_stroke}, offset={offset}"
                    );
                } else {
                    for (actual, expected) in clipped
                        .pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(reference.as_chunks::<4>().0.iter())
                    {
                        assert!(
                            actual
                                .iter()
                                .zip(expected)
                                .all(|(a, b)| a.abs_diff(*b) <= 3),
                            "clipping changed ink beyond gradient dither tolerance: {height_and_trim:?}, offset={offset}, {actual:?} vs {expected:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn static_and_animated_rich_text_share_visible_baselines_with_leading() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(error) => {
            eprintln!("headless WGPU unavailable: {error}");
            return;
        }
    };
    let text = annotated_fixture::mixed_size_pairs();
    let mut shell = AppShell::new(
        renderer,
        location_key(file!(), line!(), column!()),
        move || {
            let text = text.clone();
            Box(
                Modifier::empty()
                    .size_points(160.0, 160.0)
                    .background(Color::BLACK),
                BoxSpec::default(),
                move || {
                    for (index, motion) in [TextMotion::Static, TextMotion::Animated]
                        .into_iter()
                        .enumerate()
                    {
                        for (line_height, y) in [(40.0, 0.0), (14.0, 100.0)] {
                            let mut style = TextStyle::from_span_style(SpanStyle {
                                font_size: TextUnit::Sp(10.0),
                                ..Default::default()
                            });
                            style.paragraph_style.line_height = TextUnit::Sp(line_height);
                            style.paragraph_style.line_height_style = Some(LineHeightStyle {
                                alignment: LineHeightAlignment::Center,
                                trim: LineHeightTrim::None,
                                ..Default::default()
                            });
                            style.paragraph_style.text_motion = Some(motion);
                            Text(
                                text.clone(),
                                Modifier::empty().absolute_offset(index as f32 * 80.0, y),
                                style,
                            );
                        }
                    }
                },
            );
        },
    );
    shell.set_viewport(160.0, 160.0);
    shell.set_buffer_size(160, 160);
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(160, 160)
        .expect("captured rich text");
    for x in [0, 80] {
        for (top, bottom) in [(0, 40), (40, 80), (114, 140)] {
            let ink_bottom = |channel: usize| {
                (top..bottom)
                    .rev()
                    .find(|y| {
                        (x..x + 70).any(|x| {
                            let index = (y * 160 + x) * 4;
                            frame.pixels[index + channel] > 64
                                && frame.pixels[index + channel]
                                    > frame.pixels[index + 1 - channel].saturating_mul(2)
                        })
                    })
                    .expect("each colored glyph is visible")
            };
            let small = ink_bottom(0);
            let large = ink_bottom(1);
            assert!(
                small.abs_diff(large) <= 1,
                "column {x}, line {top}: small baseline={small}, large baseline={large}"
            );
        }
    }
}
