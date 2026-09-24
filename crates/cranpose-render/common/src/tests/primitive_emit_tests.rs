use cranpose_ui_graphics::{Brush, Color, CornerRadii};

use super::*;

#[test]
fn resolve_clip_keeps_a_clip_that_meets_nothing() {
    let list = Rect {
        x: 0.0,
        y: 80.0,
        width: 200.0,
        height: 120.0,
    };
    let scrolled_out = Rect {
        x: 40.0,
        y: -60.0,
        width: 120.0,
        height: 120.0,
    };
    let shown = Rect {
        x: 40.0,
        y: 90.0,
        width: 120.0,
        height: 120.0,
    };

    assert_eq!(
        resolve_clip(Some(list), Some(scrolled_out)),
        Some(Rect::EMPTY)
    );
    assert_eq!(resolve_clip(Some(list), Some(shown)), list.intersect(shown));
    assert_eq!(resolve_clip(Some(list), None), Some(list));
    assert_eq!(resolve_clip(None, Some(shown)), Some(shown));
    assert_eq!(resolve_clip(None, None), None);
}

#[test]
fn draw_shape_params_for_primitive_returns_transformed_rect_shape() {
    let shape = draw_shape_params_for_primitive(
        &DrawPrimitive::Rect {
            rect: Rect {
                x: 2.0,
                y: 3.0,
                width: 8.0,
                height: 5.0,
            },
            brush: Brush::solid(Color::WHITE),
            stroke: None,
        },
        Rect {
            x: 10.0,
            y: 20.0,
            width: 40.0,
            height: 30.0,
        },
        &GraphicsLayer::default(),
        None,
        BlendMode::SrcOver,
    )
    .expect("rect shape");

    assert_eq!(
        shape.rect,
        Rect {
            x: 12.0,
            y: 23.0,
            width: 8.0,
            height: 5.0,
        }
    );
    assert!(shape.shape.is_none());
}

#[test]
fn draw_shape_params_for_primitive_resolves_blended_round_rect() {
    let shape = draw_shape_params_for_primitive(
        &DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::RoundRect {
                rect: Rect {
                    x: 1.0,
                    y: 1.0,
                    width: 10.0,
                    height: 6.0,
                },
                brush: Brush::solid(Color::BLACK),
                radii: CornerRadii::uniform(4.0),
                stroke: None,
            }),
            blend_mode: BlendMode::DstOut,
        },
        Rect::from_size(cranpose_ui_graphics::Size {
            width: 20.0,
            height: 20.0,
        }),
        &GraphicsLayer::default(),
        None,
        BlendMode::SrcOver,
    )
    .expect("round rect shape");

    assert_eq!(shape.blend_mode, BlendMode::SrcOver);
    assert!(shape.shape.is_some());
}

#[test]
fn draw_shape_params_for_primitive_rejects_non_shape_primitives() {
    assert!(
        draw_shape_params_for_primitive(
            &DrawPrimitive::Image {
                rect: Rect::from_size(cranpose_ui_graphics::Size {
                    width: 4.0,
                    height: 4.0,
                }),
                image: cranpose_ui_graphics::ImageBitmap::from_rgba8(
                    1,
                    1,
                    vec![255, 255, 255, 255],
                )
                .expect("image"),
                alpha: 1.0,
                color_filter: None,
                sampling: ImageSampling::Nearest,
                src_rect: None,
            },
            Rect::from_size(cranpose_ui_graphics::Size {
                width: 10.0,
                height: 10.0,
            }),
            &GraphicsLayer::default(),
            None,
            BlendMode::SrcOver,
        )
        .is_none()
    );
}

use std::f32::consts::FRAC_PI_2;

use cranpose_ui_graphics::{Stroke, StrokeCap, StrokeJoin};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn layer_bounds() -> Rect {
    Rect {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 100.0,
    }
}

#[test]
fn stroked_rect_inflates_the_quad_by_half_the_width() {
    let params = draw_shape_params_for_primitive(
        &DrawPrimitive::Rect {
            rect: Rect {
                x: 5.0,
                y: 5.0,
                width: 40.0,
                height: 30.0,
            },
            brush: Brush::solid(Color::WHITE),
            stroke: Some(Stroke::new(6.0).with_join(StrokeJoin::Bevel)),
        },
        layer_bounds(),
        &GraphicsLayer::default(),
        None,
        BlendMode::SrcOver,
    )
    .expect("stroked rect");

    let stroke = params.stroke.expect("stroke must survive lowering");
    assert_eq!(stroke.width, 6.0);
    assert_eq!(stroke.join, StrokeJoin::Bevel);
    assert_eq!(
        params.local_rect,
        Rect {
            x: 12.0,
            y: 22.0,
            width: 46.0,
            height: 36.0,
        }
    );
    assert_eq!(params.rect, params.local_rect);
    assert!(params.arc.is_none());
}

#[test]
fn stroke_width_and_inflation_follow_the_layer_scale() {
    let layer = GraphicsLayer {
        scale: 2.0,
        transform_origin: cranpose_ui_graphics::TransformOrigin::new(0.0, 0.0),
        ..Default::default()
    };
    let params = draw_shape_params_for_primitive(
        &DrawPrimitive::Rect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            brush: Brush::solid(Color::WHITE),
            stroke: Some(Stroke::new(4.0)),
        },
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        },
        &layer,
        None,
        BlendMode::SrcOver,
    )
    .expect("stroked rect");

    assert_eq!(params.stroke.expect("stroke").width, 8.0);
    assert_eq!(
        params.local_rect,
        Rect {
            x: -4.0,
            y: -4.0,
            width: 48.0,
            height: 48.0,
        }
    );
}

#[test]
fn zero_width_stroke_emits_nothing() {
    for width in [0.0, -2.0, f32::NAN] {
        assert!(
            draw_shape_params_for_primitive(
                &DrawPrimitive::Rect {
                    rect: Rect::from_size(cranpose_ui_graphics::Size {
                        width: 10.0,
                        height: 10.0,
                    }),
                    brush: Brush::solid(Color::WHITE),
                    stroke: Some(Stroke::new(width)),
                },
                layer_bounds(),
                &GraphicsLayer::default(),
                None,
                BlendMode::SrcOver,
            )
            .is_none(),
            "stroke width {width} must not reach the renderer"
        );
    }
}

#[test]
fn arc_lowers_to_a_band_translated_into_layer_space() {
    let arc_rect = Rect {
        x: 50.0,
        y: 50.0,
        width: 12.0,
        height: 12.0,
    };
    let params = draw_shape_params_for_primitive(
        &DrawPrimitive::Arc {
            rect: arc_rect,
            brush: Brush::solid(Color::WHITE),
            center: Point::new(50.0, 50.0),
            radius: 12.0,
            start_angle: 0.0,
            sweep_angle: FRAC_PI_2,
            stroke: None,
            inner_radius: 6.0,
        },
        layer_bounds(),
        &GraphicsLayer::default(),
        None,
        BlendMode::SrcOver,
    )
    .expect("arc");

    let arc = params.arc.expect("arc geometry must survive lowering");
    assert_eq!(arc.center, Point::new(60.0, 70.0));
    assert_eq!(arc.inner_radius, 6.0);
    assert_eq!(arc.outer_radius, 12.0);
    assert_eq!(arc.cap, StrokeCap::Butt, "a filled sector has flat ends");
    assert!(approx(arc.sweep_angle, FRAC_PI_2));
    assert!(params.stroke.is_none());
    assert!(params.shape.is_none());
    assert_eq!(params.local_rect, arc_rect.translate(10.0, 20.0));
}

#[test]
fn stroked_arc_lowers_to_the_band_around_the_radius() {
    let params = draw_shape_params_for_primitive(
        &DrawPrimitive::Arc {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 60.0,
            },
            brush: Brush::solid(Color::WHITE),
            center: Point::new(30.0, 30.0),
            radius: 20.0,
            start_angle: 0.0,
            sweep_angle: 1.0,
            stroke: Some(Stroke::new(8.0).with_cap(StrokeCap::Round)),
            inner_radius: 0.0,
        },
        Rect::from_size(cranpose_ui_graphics::Size {
            width: 60.0,
            height: 60.0,
        }),
        &GraphicsLayer::default(),
        None,
        BlendMode::SrcOver,
    )
    .expect("stroked arc");

    let arc = params.arc.expect("arc geometry");
    assert_eq!(arc.inner_radius, 16.0);
    assert_eq!(arc.outer_radius, 24.0);
    assert_eq!(arc.cap, StrokeCap::Round);
    assert!(
        params.stroke.is_none(),
        "an arc carries its width in the band radii, not in `stroke`"
    );
}

#[test]
fn arc_radii_and_center_follow_the_layer_transform() {
    let layer = GraphicsLayer {
        scale: 3.0,
        transform_origin: cranpose_ui_graphics::TransformOrigin::new(0.0, 0.0),
        ..Default::default()
    };
    let params = draw_shape_params_for_primitive(
        &DrawPrimitive::Arc {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            brush: Brush::solid(Color::WHITE),
            center: Point::new(10.0, 10.0),
            radius: 10.0,
            start_angle: 0.0,
            sweep_angle: 1.0,
            stroke: None,
            inner_radius: 4.0,
        },
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        },
        &layer,
        None,
        BlendMode::SrcOver,
    )
    .expect("arc");

    let arc = params.arc.expect("arc geometry");
    assert_eq!(arc.center, Point::new(30.0, 30.0));
    assert_eq!(arc.inner_radius, 12.0);
    assert_eq!(arc.outer_radius, 30.0);
    assert_eq!(
        params.local_rect,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 60.0,
        }
    );
}

#[test]
fn degenerate_arcs_emit_nothing() {
    let base_rect = Rect::from_size(cranpose_ui_graphics::Size {
        width: 20.0,
        height: 20.0,
    });
    let cases: [(f32, f32, f32, Option<Stroke>); 4] = [
        (10.0, 10.0, 1.0, None),
        (10.0, 0.0, 0.0, None),
        (0.0, 0.0, 1.0, None),
        (10.0, 0.0, 1.0, Some(Stroke::new(0.0))),
    ];
    for (radius, inner_radius, sweep_angle, stroke) in cases {
        assert!(
            draw_shape_params_for_primitive(
                &DrawPrimitive::Arc {
                    rect: base_rect,
                    brush: Brush::solid(Color::WHITE),
                    center: Point::new(10.0, 10.0),
                    radius,
                    start_angle: 0.0,
                    sweep_angle,
                    stroke,
                    inner_radius,
                },
                layer_bounds(),
                &GraphicsLayer::default(),
                None,
                BlendMode::SrcOver,
            )
            .is_none(),
            "degenerate arc (r={radius}, inner={inner_radius}, sweep={sweep_angle}) \
             must not reach the renderer"
        );
    }
}

#[test]
fn fills_still_lower_without_stroke_or_arc() {
    let params = draw_shape_params_for_primitive(
        &DrawPrimitive::RoundRect {
            rect: Rect::from_size(cranpose_ui_graphics::Size {
                width: 10.0,
                height: 10.0,
            }),
            brush: Brush::solid(Color::WHITE),
            radii: CornerRadii::uniform(2.0),
            stroke: None,
        },
        layer_bounds(),
        &GraphicsLayer::default(),
        None,
        BlendMode::SrcOver,
    )
    .expect("round rect");
    assert!(params.stroke.is_none());
    assert!(params.arc.is_none());
    assert!(params.shape.is_some());
}

use std::rc::Rc as StdRc;

use cranpose_ui_graphics::{DrawTextStyle, FontWeight as DrawFontWeight, TextPrimitive};

#[derive(Default)]
struct CollectingTextSink {
    texts: Vec<TextDrawParams>,
}

impl DrawPrimitiveSink for CollectingTextSink {
    fn push_shape(&mut self, _params: ShapeDrawParams) {}
    fn push_image(&mut self, _params: ImageDrawParams) {}
    fn push_shadow(
        &mut self,
        _shadow_primitive: &ShadowPrimitive,
        _layer_bounds: Rect,
        _layer: &GraphicsLayer,
        _clip: Option<Rect>,
    ) {
    }
    fn push_text(&mut self, params: TextDrawParams) {
        self.texts.push(params);
    }
}

fn text_primitive(rect: Rect, text: &str, style: DrawTextStyle) -> DrawPrimitive {
    DrawPrimitive::Text(Box::new(TextPrimitive {
        rect,
        text: StdRc::from(text),
        style,
        color: Color::WHITE,
    }))
}

fn sample_text_primitive() -> DrawPrimitive {
    text_primitive(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 30.0,
            height: 14.0,
        },
        "AB",
        DrawTextStyle::new(10.0),
    )
}

fn lower_text(primitive: DrawPrimitive, layer: &GraphicsLayer) -> Vec<TextDrawParams> {
    let mut sink = CollectingTextSink::default();
    emit_draw_primitive(
        &primitive,
        layer_bounds(),
        layer,
        None,
        &mut sink,
        None,
        false,
    );
    sink.texts
}

#[test]
fn text_lowers_into_the_layer_translated_block_the_scope_measured() {
    let params = lower_text(
        text_primitive(
            Rect {
                x: 5.0,
                y: 6.0,
                width: 40.0,
                height: 18.0,
            },
            "SCORE",
            DrawTextStyle::new(12.0),
        ),
        &GraphicsLayer::default(),
    );
    assert_eq!(params.len(), 1);
    assert_eq!(
        params[0].rect,
        Rect {
            x: 15.0,
            y: 26.0,
            width: 40.0,
            height: 18.0,
        }
    );
    assert_eq!(params[0].text.text, "SCORE");
    assert_eq!(params[0].font_size, 12.0);
    assert_eq!(params[0].scale, 1.0);
    assert_eq!(params[0].color, Color::WHITE);
}

#[test]
fn text_lowering_carries_the_uniform_layer_scale_for_rasterization() {
    let layer = GraphicsLayer {
        scale: 2.0,
        transform_origin: cranpose_ui_graphics::TransformOrigin::new(0.0, 0.0),
        ..Default::default()
    };
    let params = lower_text(sample_text_primitive(), &layer);
    assert_eq!(params[0].scale, 2.0, "glyphs rasterize at the layer scale");
    assert!(approx(params[0].rect.width, 60.0), "{:?}", params[0].rect);
}

#[test]
fn text_lowering_folds_the_layer_alpha_into_the_glyph_color() {
    let layer = GraphicsLayer {
        alpha: 0.5,
        ..Default::default()
    };
    let params = lower_text(sample_text_primitive(), &layer);
    assert!(approx(params[0].color.3, 0.5));
}

#[test]
fn text_lowering_uses_the_same_style_translation_the_draw_scope_measured_with() {
    let style = DrawTextStyle::new(18.0)
        .with_font_family("Fira Sans")
        .with_weight(DrawFontWeight::BOLD);
    let params = lower_text(
        text_primitive(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 20.0,
            },
            "AB",
            style.clone(),
        ),
        &GraphicsLayer::default(),
    );
    assert_eq!(params[0].text_style, text_style_for_draw_style(&style));
}

#[test]
fn lowered_text_is_never_re_wrapped_against_the_box_it_was_measured_into() {
    let params = lower_text(
        text_primitive(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 14.0,
            },
            "a very long line",
            DrawTextStyle::new(10.0),
        ),
        &GraphicsLayer::default(),
    );
    assert!(!params[0].layout_options.soft_wrap);
    assert_eq!(params[0].layout_options.overflow, TextOverflow::Visible);
}

#[test]
fn degenerate_text_never_reaches_a_sink() {
    let invisible_layer = GraphicsLayer {
        alpha: 0.0,
        ..Default::default()
    };
    let cases: [(DrawPrimitive, GraphicsLayer); 3] = [
        (
            text_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 10.0,
                },
                "",
                DrawTextStyle::new(10.0),
            ),
            GraphicsLayer::default(),
        ),
        (
            text_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 10.0,
                },
                "AB",
                DrawTextStyle::new(10.0),
            ),
            GraphicsLayer::default(),
        ),
        (
            text_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 10.0,
                },
                "AB",
                DrawTextStyle::new(10.0),
            ),
            invisible_layer,
        ),
    ];
    for (primitive, layer) in cases {
        assert!(
            lower_text(primitive, &layer).is_empty(),
            "degenerate text must not reach the renderer"
        );
    }
}

#[test]
fn blended_text_still_lowers_because_glyphs_composite_src_over() {
    let params = lower_text(
        DrawPrimitive::Blend {
            primitive: Box::new(text_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 10.0,
                },
                "AB",
                DrawTextStyle::new(10.0),
            )),
            blend_mode: BlendMode::DstOut,
        },
        &GraphicsLayer::default(),
    );
    assert_eq!(params.len(), 1);
}
