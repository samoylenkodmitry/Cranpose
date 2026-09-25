use super::*;
use crate::{Color, FontStyle, FontWeight, ImageBitmap, RenderEffect};

#[test]
fn recorded_iterators_preserve_shapes_content_and_shadows() {
    let shape = DrawPrimitive::Rect {
        rect: Rect::from_size(Size::new(12.0, 8.0)),
        brush: Brush::solid(Color::RED),
        stroke: None,
    };
    let shadow = DrawPrimitive::Shadow(ShadowPrimitive::Drop {
        shape: Box::new(shape.clone()),
        cutout: None,
        blur_radius: 4.0,
        blend_mode: BlendMode::SrcOver,
    });
    let mut scope = DrawScopeDefault::new(Size::new(24.0, 24.0));
    scope.push_recorded(None);
    scope.push_recorded(Some(shadow.clone()));
    scope.push_recorded([DrawPrimitive::Content, shape.clone()]);
    scope.push_recorded(std::iter::empty());
    let recording = scope.finish();
    assert_eq!(recording.content_markers(), 1);
    assert_eq!(
        recording.into_primitives_with_markers(),
        vec![shadow, DrawPrimitive::Content, shape]
    );
}

#[test]
fn compact_recording_materializes_in_recorded_order() {
    let size = Size::new(100.0, 100.0);
    let solid = Brush::solid(Color::WHITE);
    let gradient = Brush::vertical_gradient(vec![Color::RED, Color::BLUE], 0.0, 100.0);
    let center = Point::new(50.0, 50.0);
    let stroke = Stroke::new(4.0);
    let rect = Rect {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 40.0,
    };
    let batch = vec![
        DrawPrimitive::Content,
        DrawPrimitive::Rect {
            rect,
            brush: solid.clone(),
            stroke: None,
        },
    ];

    let record = |scope: &mut DrawScopeDefault| {
        scope.draw_rect_at(rect, solid.clone());
        scope.draw_arc(solid.clone(), center, 30.0, 0.5, 1.5, stroke);
        scope.draw_rect_at(rect, gradient.clone());
        scope.draw_circle(solid.clone(), center, 12.0);
        scope.draw_arc(solid.clone(), center, 30.0, 0.5, 0.0, stroke);
        scope.draw_rect_at_blend(rect, solid.clone(), BlendMode::Plus);
        scope.draw_content();
        scope.draw_annular_sector(gradient.clone(), center, 10.0, 20.0, 0.0, 2.0);
        scope.push_recorded(batch.clone());
    };

    let mut compact = DrawScopeDefault::new(size);
    record(&mut compact);
    let finished = compact.finish();

    let arc_via_ordinary = |brush: Brush, radius: f32, start: f32, sweep: f32| {
        let mut scope = DrawScopeDefault::new(size);
        scope.draw_arc(brush, center, radius, start, sweep, stroke);
        scope.into_primitives().remove(0)
    };
    let expected = vec![
        DrawPrimitive::Rect {
            rect,
            brush: solid.clone(),
            stroke: None,
        },
        arc_via_ordinary(solid.clone(), 30.0, 0.5, 1.5),
        DrawPrimitive::Rect {
            rect,
            brush: gradient.clone(),
            stroke: None,
        },
        DrawPrimitive::RoundRect {
            rect: Rect {
                x: center.x - 12.0,
                y: center.y - 12.0,
                width: 24.0,
                height: 24.0,
            },
            brush: solid.clone(),
            radii: CornerRadii::uniform(12.0),
            stroke: None,
        },
        DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::Rect {
                rect,
                brush: solid.clone(),
                stroke: None,
            }),
            blend_mode: BlendMode::Plus,
        },
        DrawPrimitive::Content,
        {
            let mut scope = DrawScopeDefault::new(size);
            scope.draw_annular_sector(gradient.clone(), center, 10.0, 20.0, 0.0, 2.0);
            scope.into_primitives().remove(0)
        },
        DrawPrimitive::Content,
        DrawPrimitive::Rect {
            rect,
            brush: solid.clone(),
            stroke: None,
        },
    ];
    assert_eq!(finished.content_markers(), 2);
    assert_eq!(finished.into_primitives_with_markers(), expected);
}

#[test]
fn redrawing_the_same_text_shares_one_str_allocation() {
    let first = shared_text_str("BREAK THE RING");
    let second = shared_text_str("BREAK THE RING");
    assert!(Rc::ptr_eq(&first, &second));
    assert_eq!(&*second, "BREAK THE RING");
}

#[test]
fn different_text_gets_its_own_str() {
    let first = shared_text_str("340");
    let second = shared_text_str("350");
    assert!(!Rc::ptr_eq(&first, &second));
    assert_eq!(&*first, "340");
    assert_eq!(&*second, "350");
}

#[test]
fn the_text_pool_survives_overflowing_its_capacity() {
    for index in 0..600 {
        let text = format!("run-{index}");
        assert_eq!(&*shared_text_str(&text), text.as_str());
    }
    assert_eq!(&*shared_text_str("still correct"), "still correct");
}

fn assert_image_alpha(primitive: &DrawPrimitive, expected: f32) {
    match primitive {
        DrawPrimitive::Image { alpha, .. } => assert!((alpha - expected).abs() < 1e-5),
        DrawPrimitive::Blend { primitive, .. } => assert_image_alpha(primitive, expected),
        other => panic!("expected image primitive, got {other:?}"),
    }
}

fn unwrap_image(primitive: &DrawPrimitive) -> &DrawPrimitive {
    match primitive {
        DrawPrimitive::Image { .. } => primitive,
        DrawPrimitive::Blend { primitive, .. } => unwrap_image(primitive),
        other => panic!("expected image primitive, got {other:?}"),
    }
}

#[test]
fn draw_svg_path_emits_supersampled_image_over_path_bounds() {
    let mut scope = DrawScopeDefault::new(Size::new(32.0, 32.0));
    scope.draw_svg_path("M 4 4 H 20 V 20 H 4 Z", Brush::solid(Color::RED));

    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    let DrawPrimitive::Image { rect, image, .. } = &primitives[0] else {
        panic!("expected image primitive, got {:?}", primitives[0]);
    };

    assert_eq!((rect.x, rect.y), (3.0, 3.0));
    assert_eq!((rect.width, rect.height), (18.0, 18.0));
    assert_eq!((image.width(), image.height()), (36, 36));

    let pixels = image.pixels();
    let index = (18 * 36 + 18) * 4;
    assert_eq!(
        &pixels[index..index + 4],
        &[255, 0, 0, 255],
        "path interior must be opaque brush color"
    );
    assert_eq!(pixels[3], 0, "outside the path must stay transparent");
}

#[test]
fn draw_svg_path_ignores_invalid_data() {
    let mut scope = DrawScopeDefault::new(Size::new(16.0, 16.0));
    scope.draw_svg_path("definitely not a path", Brush::solid(Color::WHITE));
    assert!(scope.into_primitives().is_empty());
}

#[test]
fn draw_vector_path_applies_brush_alpha() {
    let path = crate::VectorPath::parse("M 0 0 H 8 V 8 H 0 Z").expect("valid path");
    let mut scope = DrawScopeDefault::new(Size::new(16.0, 16.0));
    scope.draw_vector_path(&path, Brush::solid(Color::rgba(0.0, 0.0, 1.0, 0.5)));

    let primitives = scope.into_primitives();
    let DrawPrimitive::Image { image, .. } = &primitives[0] else {
        panic!("expected image primitive");
    };
    let pixels = image.pixels();
    let width = image.width() as usize;
    let index = ((image.height() as usize / 2) * width + width / 2) * 4;
    assert_eq!(&pixels[index..index + 3], &[0, 0, 255]);
    let alpha = pixels[index + 3];
    assert!(
        (alpha as i32 - 128).abs() <= 2,
        "interior alpha must honor the brush alpha, got {alpha}"
    );
}

#[test]
fn the_same_path_and_color_reuse_one_raster() {
    let path = crate::VectorPath::parse("M 0 0 H 7 V 7 H 0 Z").expect("valid path");
    let raster_of = |brush: Brush| {
        let mut scope = DrawScopeDefault::new(Size::new(16.0, 16.0));
        scope.draw_vector_path(&path, brush);
        let primitives = scope.into_primitives();
        let DrawPrimitive::Image { image, .. } = &primitives[0] else {
            panic!("expected image primitive");
        };
        image.clone()
    };

    let first = raster_of(Brush::solid(Color::rgba(0.0, 0.0, 1.0, 1.0)));
    let second = raster_of(Brush::solid(Color::rgba(0.0, 0.0, 1.0, 1.0)));
    assert_eq!(first.id(), second.id());

    let other_color = raster_of(Brush::solid(Color::rgba(1.0, 0.0, 0.0, 1.0)));
    assert_ne!(first.id(), other_color.id());

    let wider = crate::VectorPath::parse("M 0 0 H 9 V 7 H 0 Z").expect("valid path");
    let mut scope = DrawScopeDefault::new(Size::new(16.0, 16.0));
    scope.draw_vector_path(&wider, Brush::solid(Color::rgba(0.0, 0.0, 1.0, 1.0)));
    let primitives = scope.into_primitives();
    let DrawPrimitive::Image { image, .. } = &primitives[0] else {
        panic!("expected image primitive");
    };
    assert_ne!(first.id(), image.id());
}

#[test]
fn draw_content_inserts_content_marker() {
    let mut scope = DrawScopeDefault::new(Size::new(8.0, 8.0));
    scope.draw_rect(Brush::solid(Color::WHITE));
    scope.draw_content();
    scope.draw_rect_blend(Brush::solid(Color::BLACK), BlendMode::DstOut);

    let primitives = scope.into_primitives();
    assert!(matches!(primitives[1], DrawPrimitive::Content));
    assert!(matches!(
        primitives[2],
        DrawPrimitive::Blend {
            blend_mode: BlendMode::DstOut,
            ..
        }
    ));
}

#[test]
fn a_text_block_is_placed_by_its_alignment_inside_the_rect() {
    let rect = Rect {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 40.0,
    };
    let measurement = TextMeasurement {
        size: Size::new(60.0, 16.0),
        line_height: 16.0,
        first_baseline: 12.0,
        line_count: 1,
    };
    let style = |align, vertical| {
        DrawTextStyle::default()
            .with_align(align)
            .with_vertical_align(vertical)
    };

    let left = align_text_block(
        rect,
        measurement,
        &style(TextAlign::Left, TextVerticalAlign::Top),
    );
    assert_eq!(left, Point::new(10.0, 20.0));

    let centered = align_text_block(
        rect,
        measurement,
        &style(TextAlign::Center, TextVerticalAlign::Center),
    );
    assert_eq!(centered, Point::new(10.0 + 20.0, 20.0 + 12.0));

    let right = align_text_block(
        rect,
        measurement,
        &style(TextAlign::Right, TextVerticalAlign::Bottom),
    );
    assert_eq!(right, Point::new(50.0, 44.0));

    let baseline = align_text_block(
        rect,
        measurement,
        &style(TextAlign::Left, TextVerticalAlign::Baseline),
    );
    assert_eq!(baseline, Point::new(10.0, 20.0 - 12.0));
}

#[test]
fn draw_rect_blend_wraps_non_default_modes() {
    let mut scope = DrawScopeDefault::new(Size::new(10.0, 10.0));
    scope.draw_rect_blend(Brush::solid(Color::RED), BlendMode::DstOut);

    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match &primitives[0] {
        DrawPrimitive::Blend {
            primitive,
            blend_mode,
        } => {
            assert_eq!(*blend_mode, BlendMode::DstOut);
            assert!(matches!(**primitive, DrawPrimitive::Rect { .. }));
        }
        other => panic!("expected blended primitive, got {other:?}"),
    }
}

#[test]
fn draw_circle_records_centered_round_rect() {
    let mut scope = DrawScopeDefault::new(Size::new(40.0, 40.0));
    scope.draw_circle(Brush::solid(Color::BLUE), Point::new(12.0, 16.0), 5.0);

    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match &primitives[0] {
        DrawPrimitive::RoundRect { rect, radii, .. } => {
            assert_eq!(
                *rect,
                Rect {
                    x: 7.0,
                    y: 11.0,
                    width: 10.0,
                    height: 10.0,
                }
            );
            assert_eq!(*radii, CornerRadii::uniform(5.0));
        }
        other => panic!("expected circular round rect, got {other:?}"),
    }
}

#[test]
fn draw_circle_blend_wraps_non_default_modes() {
    let mut scope = DrawScopeDefault::new(Size::new(10.0, 10.0));
    scope.draw_circle_blend(
        Brush::solid(Color::RED),
        Point::new(5.0, 5.0),
        3.0,
        BlendMode::Plus,
    );

    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match &primitives[0] {
        DrawPrimitive::Blend {
            primitive,
            blend_mode,
        } => {
            assert_eq!(*blend_mode, BlendMode::Plus);
            assert!(matches!(**primitive, DrawPrimitive::RoundRect { .. }));
        }
        other => panic!("expected blended circle primitive, got {other:?}"),
    }
}

#[test]
fn rect_is_empty_without_area() {
    assert!(Rect::EMPTY.is_empty());
    assert!(
        Rect {
            x: 4.0,
            y: 5.0,
            width: 0.0,
            height: 6.0,
        }
        .is_empty()
    );
    assert!(
        Rect {
            x: 4.0,
            y: 5.0,
            width: 6.0,
            height: -1.0,
        }
        .is_empty()
    );
    assert!(
        !Rect {
            x: 4.0,
            y: 5.0,
            width: 0.5,
            height: 0.5,
        }
        .is_empty()
    );
}

#[test]
fn rect_union_encloses_both_inputs() {
    let lhs = Rect {
        x: 10.0,
        y: 5.0,
        width: 8.0,
        height: 4.0,
    };
    let rhs = Rect {
        x: 4.0,
        y: 7.0,
        width: 10.0,
        height: 6.0,
    };

    assert_eq!(
        lhs.union(rhs),
        Rect {
            x: 4.0,
            y: 5.0,
            width: 14.0,
            height: 8.0,
        }
    );
}

#[test]
fn draw_image_uses_scope_size_as_default_rect() {
    let mut scope = DrawScopeDefault::new(Size::new(40.0, 24.0));
    let image = ImageBitmap::from_rgba8(2, 2, vec![255; 16]).expect("image");
    scope.draw_image(image.clone());
    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match unwrap_image(&primitives[0]) {
        DrawPrimitive::Image {
            rect,
            image: actual,
            alpha,
            color_filter,
            sampling,
            src_rect,
        } => {
            assert_eq!(*rect, Rect::from_size(Size::new(40.0, 24.0)));
            assert_eq!(*actual, image);
            assert_eq!(*alpha, 1.0);
            assert!(color_filter.is_none());
            assert_eq!(*sampling, ImageSampling::Nearest);
            assert!(src_rect.is_none());
        }
        other => panic!("expected image primitive, got {other:?}"),
    }
}

#[test]
fn draw_image_src_stores_src_rect() {
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
    let image = ImageBitmap::from_rgba8(64, 64, vec![255; 64 * 64 * 4]).expect("image");
    let src = Rect {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 40.0,
    };
    let dst = Rect {
        x: 0.0,
        y: 0.0,
        width: 60.0,
        height: 80.0,
    };
    scope.draw_image_src(image.clone(), src, dst, 0.8, None);
    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match unwrap_image(&primitives[0]) {
        DrawPrimitive::Image {
            rect,
            image: actual,
            alpha,
            sampling,
            src_rect,
            ..
        } => {
            assert_eq!(*rect, dst);
            assert_eq!(*actual, image);
            assert!((alpha - 0.8).abs() < 1e-5);
            assert_eq!(*sampling, ImageSampling::Nearest);
            assert_eq!(*src_rect, Some(src));
        }
        other => panic!("expected image primitive, got {other:?}"),
    }
}

#[test]
fn draw_image_at_sampled_records_requested_sampling() {
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
    let image = ImageBitmap::from_rgba8(8, 8, vec![255; 8 * 8 * 4]).expect("image");
    let dst = Rect {
        x: 2.0,
        y: 3.0,
        width: 40.0,
        height: 30.0,
    };

    scope.draw_image_at_sampled(dst, image.clone(), 0.7, None, ImageSampling::Linear);

    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match unwrap_image(&primitives[0]) {
        DrawPrimitive::Image {
            rect,
            image: actual,
            alpha,
            sampling,
            src_rect,
            ..
        } => {
            assert_eq!(*rect, dst);
            assert_eq!(*actual, image);
            assert!((alpha - 0.7).abs() < 1e-5);
            assert_eq!(*sampling, ImageSampling::Linear);
            assert!(src_rect.is_none());
        }
        other => panic!("expected image primitive, got {other:?}"),
    }
}

#[test]
fn draw_image_src_sampled_records_requested_sampling() {
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
    let image = ImageBitmap::from_rgba8(64, 64, vec![255; 64 * 64 * 4]).expect("image");
    let src = Rect {
        x: 4.0,
        y: 6.0,
        width: 16.0,
        height: 20.0,
    };
    let dst = Rect {
        x: 8.0,
        y: 10.0,
        width: 32.0,
        height: 40.0,
    };

    scope.draw_image_src_sampled(image.clone(), src, dst, 0.5, None, ImageSampling::Linear);

    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match unwrap_image(&primitives[0]) {
        DrawPrimitive::Image {
            rect,
            image: actual,
            alpha,
            sampling,
            src_rect,
            ..
        } => {
            assert_eq!(*rect, dst);
            assert_eq!(*actual, image);
            assert!((alpha - 0.5).abs() < 1e-5);
            assert_eq!(*sampling, ImageSampling::Linear);
            assert_eq!(*src_rect, Some(src));
        }
        other => panic!("expected image primitive, got {other:?}"),
    }
}

#[test]
fn draw_image_at_clamps_alpha() {
    let mut scope = DrawScopeDefault::new(Size::new(10.0, 10.0));
    let image = ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image");
    scope.draw_image_at(
        Rect::from_origin_size(Point::new(2.0, 3.0), Size::new(5.0, 6.0)),
        image,
        3.0,
        Some(ColorFilter::Tint(Color::from_rgba_u8(128, 128, 255, 255))),
    );
    assert_image_alpha(&scope.into_primitives()[0], 1.0);
}

#[test]
fn graphics_layer_clone_with_render_effect() {
    let layer = GraphicsLayer {
        render_effect: Some(RenderEffect::blur(10.0)),
        backdrop_effect: Some(RenderEffect::blur(6.0)),
        color_filter: Some(ColorFilter::tint(Color::from_rgba_u8(128, 200, 255, 255))),
        alpha: 0.5,
        rotation_z: 12.0,
        shadow_elevation: 4.0,
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(6.0)),
        clip: true,
        compositing_strategy: CompositingStrategy::Offscreen,
        blend_mode: BlendMode::SrcOver,
        ..Default::default()
    };
    let cloned = layer.clone();
    assert_eq!(cloned.alpha, 0.5);
    assert!(cloned.render_effect.is_some());
    assert!(cloned.backdrop_effect.is_some());
    assert_eq!(layer.color_filter, cloned.color_filter);
    assert_eq!(layer.render_effect, cloned.render_effect);
    assert_eq!(layer.backdrop_effect, cloned.backdrop_effect);
    assert!((cloned.rotation_z - 12.0).abs() < 1e-6);
    assert!((cloned.shadow_elevation - 4.0).abs() < 1e-6);
    assert_eq!(
        cloned.shape,
        LayerShape::Rounded(RoundedCornerShape::uniform(6.0))
    );
    assert!(cloned.clip);
    assert_eq!(cloned.compositing_strategy, CompositingStrategy::Offscreen);
    assert_eq!(cloned.blend_mode, BlendMode::SrcOver);
}

#[test]
fn graphics_layer_default_has_no_effect() {
    let layer = GraphicsLayer::default();
    assert!(layer.color_filter.is_none());
    assert!(layer.render_effect.is_none());
    assert!(layer.backdrop_effect.is_none());
    assert_eq!(layer.compositing_strategy, CompositingStrategy::Auto);
    assert_eq!(layer.blend_mode, BlendMode::SrcOver);
    assert_eq!(layer.alpha, 1.0);
    assert_eq!(layer.transform_origin, TransformOrigin::CENTER);
    assert!((layer.camera_distance - 8.0).abs() < 1e-6);
    assert_eq!(layer.shape, LayerShape::Rectangle);
    assert!(!layer.clip);
    assert_eq!(layer.ambient_shadow_color, Color::BLACK);
    assert_eq!(layer.spot_shadow_color, Color::BLACK);
}

#[test]
fn transform_origin_construction() {
    let origin = TransformOrigin::new(0.25, 0.75);
    assert!((origin.pivot_fraction_x - 0.25).abs() < 1e-6);
    assert!((origin.pivot_fraction_y - 0.75).abs() < 1e-6);
}

#[test]
fn layer_shape_default_is_rectangle() {
    assert_eq!(LayerShape::default(), LayerShape::Rectangle);
}

use std::f32::consts::{FRAC_PI_2, PI};

use crate::{StrokeCap, StrokeJoin};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.25
}

fn scope(size: f32) -> DrawScopeDefault {
    DrawScopeDefault::new(Size::new(size, size))
}

#[test]
fn draw_rect_stroked_records_scope_rect_and_stroke() {
    let mut scope = scope(20.0);
    scope.draw_rect_stroked(
        Brush::solid(Color::RED),
        Stroke::new(3.0).with_join(StrokeJoin::Bevel),
    );

    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match &primitives[0] {
        DrawPrimitive::Rect {
            rect,
            stroke: Some(stroke),
            ..
        } => {
            assert_eq!(*rect, Rect::from_size(Size::new(20.0, 20.0)));
            assert_eq!(stroke.width, 3.0);
            assert_eq!(stroke.join, StrokeJoin::Bevel);
        }
        other => panic!("expected stroked rect, got {other:?}"),
    }
}

#[test]
fn draw_rect_at_stroked_records_requested_rect() {
    let mut scope = scope(50.0);
    let rect = Rect {
        x: 4.0,
        y: 6.0,
        width: 12.0,
        height: 9.0,
    };
    scope.draw_rect_at_stroked(rect, Brush::solid(Color::BLUE), Stroke::new(2.0));
    match &scope.into_primitives()[0] {
        DrawPrimitive::Rect {
            rect: actual,
            stroke: Some(stroke),
            ..
        } => {
            assert_eq!(*actual, rect);
            assert_eq!(stroke.width, 2.0);
        }
        other => panic!("expected stroked rect, got {other:?}"),
    }
}

#[test]
fn draw_round_rect_stroked_keeps_radii_and_stroke() {
    let mut scope = scope(30.0);
    scope.draw_round_rect_stroked(
        Brush::solid(Color::GREEN),
        CornerRadii::uniform(5.0),
        Stroke::new(4.0).with_join(StrokeJoin::Round),
    );
    match &scope.into_primitives()[0] {
        DrawPrimitive::RoundRect {
            rect,
            radii,
            stroke: Some(stroke),
            ..
        } => {
            assert_eq!(*rect, Rect::from_size(Size::new(30.0, 30.0)));
            assert_eq!(*radii, CornerRadii::uniform(5.0));
            assert_eq!(stroke.width, 4.0);
            assert_eq!(stroke.join, StrokeJoin::Round);
        }
        other => panic!("expected stroked round rect, got {other:?}"),
    }
}

#[test]
fn draw_round_rect_at_stroked_records_requested_rect() {
    let mut scope = scope(60.0);
    let rect = Rect {
        x: 1.0,
        y: 2.0,
        width: 20.0,
        height: 10.0,
    };
    scope.draw_round_rect_at_stroked(
        rect,
        Brush::solid(Color::WHITE),
        CornerRadii::uniform(3.0),
        Stroke::new(1.5),
    );
    match &scope.into_primitives()[0] {
        DrawPrimitive::RoundRect {
            rect: actual,
            radii,
            stroke: Some(stroke),
            ..
        } => {
            assert_eq!(*actual, rect);
            assert_eq!(*radii, CornerRadii::uniform(3.0));
            assert_eq!(stroke.width, 1.5);
        }
        other => panic!("expected stroked round rect, got {other:?}"),
    }
}

#[test]
fn draw_circle_stroked_lowers_to_stroked_round_rect() {
    let mut scope = scope(40.0);
    scope.draw_circle_stroked(
        Brush::solid(Color::BLUE),
        Point::new(12.0, 16.0),
        5.0,
        Stroke::new(2.0),
    );
    match &scope.into_primitives()[0] {
        DrawPrimitive::RoundRect {
            rect,
            radii,
            stroke: Some(stroke),
            ..
        } => {
            assert_eq!(
                *rect,
                Rect {
                    x: 7.0,
                    y: 11.0,
                    width: 10.0,
                    height: 10.0,
                }
            );
            assert_eq!(*radii, CornerRadii::uniform(5.0));
            assert_eq!(stroke.width, 2.0);
        }
        other => panic!("expected stroked circular round rect, got {other:?}"),
    }
}

#[test]
fn draw_arc_records_arc_primitive_with_tight_bounds() {
    let mut scope = scope(200.0);
    scope.draw_arc(
        Brush::solid(Color::RED),
        Point::new(100.0, 100.0),
        50.0,
        0.0,
        FRAC_PI_2,
        Stroke::new(10.0),
    );
    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    match &primitives[0] {
        DrawPrimitive::Arc {
            rect,
            center,
            radius,
            start_angle,
            sweep_angle,
            stroke: Some(stroke),
            inner_radius,
            ..
        } => {
            assert_eq!(*center, Point::new(100.0, 100.0));
            assert_eq!(*radius, 50.0);
            assert_eq!(*start_angle, 0.0);
            assert!(approx(*sweep_angle, FRAC_PI_2));
            assert_eq!(stroke.width, 10.0);
            assert_eq!(*inner_radius, 0.0);
            assert!(approx(rect.x, 100.0), "{rect:?}");
            assert!(approx(rect.y, 100.0), "{rect:?}");
            assert!(approx(rect.width, 55.0), "{rect:?}");
            assert!(approx(rect.height, 55.0), "{rect:?}");
        }
        other => panic!("expected arc primitive, got {other:?}"),
    }
}

#[test]
fn draw_arc_bounds_cover_a_quadrant_spanning_sweep() {
    let mut scope = scope(200.0);
    scope.draw_arc(
        Brush::solid(Color::RED),
        Point::new(100.0, 100.0),
        50.0,
        0.0,
        3.0 * FRAC_PI_2,
        Stroke::new(4.0),
    );
    let DrawPrimitive::Arc { rect, .. } = &scope.into_primitives()[0] else {
        panic!("expected arc primitive");
    };
    assert!(approx(rect.x, 48.0), "{rect:?}");
    assert!(approx(rect.y, 48.0), "{rect:?}");
    assert!(approx(rect.width, 104.0), "{rect:?}");
    assert!(approx(rect.height, 104.0), "{rect:?}");
}

#[test]
fn draw_annular_sector_records_inner_radius_and_no_stroke() {
    let mut scope = scope(200.0);
    scope.draw_annular_sector(
        Brush::solid(Color::WHITE),
        Point::new(100.0, 100.0),
        30.0,
        50.0,
        0.0,
        PI,
    );
    match &scope.into_primitives()[0] {
        DrawPrimitive::Arc {
            rect,
            center,
            radius,
            inner_radius,
            stroke,
            sweep_angle,
            ..
        } => {
            assert!(stroke.is_none(), "annular sectors are filled, not stroked");
            assert_eq!(*center, Point::new(100.0, 100.0));
            assert_eq!(*radius, 50.0);
            assert_eq!(*inner_radius, 30.0);
            assert!(approx(*sweep_angle, PI));
            assert!(approx(rect.x, 50.0), "{rect:?}");
            assert!(approx(rect.y, 100.0), "{rect:?}");
            assert!(approx(rect.width, 100.0), "{rect:?}");
            assert!(approx(rect.height, 50.0), "{rect:?}");
        }
        other => panic!("expected arc primitive, got {other:?}"),
    }
}

#[test]
fn draw_arc_blend_wraps_non_default_modes() {
    let mut scope = scope(100.0);
    scope.draw_arc_blend(
        Brush::solid(Color::RED),
        Point::new(50.0, 50.0),
        20.0,
        0.0,
        1.0,
        Stroke::new(2.0),
        BlendMode::DstOut,
    );
    match &scope.into_primitives()[0] {
        DrawPrimitive::Blend {
            primitive,
            blend_mode,
        } => {
            assert_eq!(*blend_mode, BlendMode::DstOut);
            assert!(matches!(**primitive, DrawPrimitive::Arc { .. }));
        }
        other => panic!("expected blended arc, got {other:?}"),
    }
}

#[test]
fn draw_annular_sector_blend_wraps_non_default_modes() {
    let mut scope = scope(100.0);
    scope.draw_annular_sector_blend(
        Brush::solid(Color::RED),
        Point::new(50.0, 50.0),
        5.0,
        20.0,
        0.0,
        1.0,
        BlendMode::Plus,
    );
    assert!(matches!(
        &scope.into_primitives()[0],
        DrawPrimitive::Blend {
            blend_mode: BlendMode::Plus,
            ..
        }
    ));
}

#[test]
fn stroked_blend_variants_wrap_non_default_modes() {
    let mut scope = scope(20.0);
    scope.draw_rect_stroked_blend(
        Brush::solid(Color::RED),
        Stroke::new(2.0),
        BlendMode::DstOut,
    );
    scope.draw_round_rect_stroked_blend(
        Brush::solid(Color::RED),
        CornerRadii::uniform(2.0),
        Stroke::new(2.0),
        BlendMode::DstOut,
    );
    scope.draw_circle_stroked_blend(
        Brush::solid(Color::RED),
        Point::new(10.0, 10.0),
        5.0,
        Stroke::new(2.0),
        BlendMode::DstOut,
    );
    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 3);
    for primitive in &primitives {
        assert!(
            matches!(
                primitive,
                DrawPrimitive::Blend {
                    blend_mode: BlendMode::DstOut,
                    ..
                }
            ),
            "expected blended primitive, got {primitive:?}"
        );
    }
}

#[test]
fn negative_sweeps_and_overlong_sweeps_produce_finite_bounds() {
    let mut scope = scope(200.0);
    scope.draw_arc(
        Brush::solid(Color::RED),
        Point::new(100.0, 100.0),
        40.0,
        FRAC_PI_2,
        -FRAC_PI_2,
        Stroke::new(4.0),
    );
    scope.draw_arc(
        Brush::solid(Color::RED),
        Point::new(100.0, 100.0),
        40.0,
        0.3,
        crate::stroke::TAU * 4.0,
        Stroke::new(4.0),
    );
    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 2);

    let DrawPrimitive::Arc { rect: negative, .. } = &primitives[0] else {
        panic!("expected arc");
    };
    assert!(approx(negative.x, 100.0), "{negative:?}");
    assert!(approx(negative.y, 100.0), "{negative:?}");
    assert!(approx(negative.width, 42.0), "{negative:?}");

    let DrawPrimitive::Arc { rect: full, .. } = &primitives[1] else {
        panic!("expected arc");
    };
    assert!(approx(full.x, 58.0), "{full:?}");
    assert!(approx(full.width, 84.0), "{full:?}");
    assert!(approx(full.height, 84.0), "{full:?}");
}

#[test]
fn degenerate_stroke_and_arc_inputs_emit_nothing_and_never_panic() {
    let mut scope = scope(50.0);
    let brush = Brush::solid(Color::RED);
    let center = Point::new(25.0, 25.0);

    scope.draw_rect_stroked(brush.clone(), Stroke::new(0.0));
    scope.draw_rect_stroked(brush.clone(), Stroke::new(-4.0));
    scope.draw_rect_stroked(brush.clone(), Stroke::new(f32::NAN));
    scope.draw_round_rect_stroked(brush.clone(), CornerRadii::uniform(2.0), Stroke::new(0.0));
    scope.draw_circle_stroked(brush.clone(), center, 10.0, Stroke::new(0.0));
    scope.draw_circle_stroked(brush.clone(), center, f32::NAN, Stroke::new(2.0));
    scope.draw_arc(brush.clone(), center, 10.0, 0.0, 0.0, Stroke::new(2.0));
    scope.draw_arc(brush.clone(), center, 10.0, 0.0, f32::NAN, Stroke::new(2.0));
    scope.draw_arc(
        brush.clone(),
        center,
        f32::INFINITY,
        0.0,
        1.0,
        Stroke::new(2.0),
    );
    scope.draw_arc(brush.clone(), center, 10.0, 0.0, 1.0, Stroke::new(0.0));
    scope.draw_arc(brush.clone(), center, 0.0, 0.0, 1.0, Stroke::new(0.0));
    scope.draw_annular_sector(brush.clone(), center, 10.0, 10.0, 0.0, 1.0);
    scope.draw_annular_sector(brush.clone(), center, 20.0, 10.0, 0.0, 1.0);
    scope.draw_annular_sector(brush.clone(), center, 0.0, 0.0, 0.0, 1.0);
    scope.draw_annular_sector(brush.clone(), center, 0.0, 10.0, 0.0, 0.0);
    scope.draw_annular_sector(brush, center, f32::NAN, 10.0, 0.0, 1.0);

    assert!(
        scope.into_primitives().is_empty(),
        "degenerate stroke/arc requests must not reach the renderer"
    );
}

#[test]
fn zero_radius_arc_with_positive_width_stays_finite() {
    let mut scope = scope(50.0);
    scope.draw_arc(
        Brush::solid(Color::RED),
        Point::new(25.0, 25.0),
        0.0,
        0.0,
        FRAC_PI_2,
        Stroke::new(6.0).with_cap(StrokeCap::Round),
    );
    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    let DrawPrimitive::Arc { rect, .. } = &primitives[0] else {
        panic!("expected arc");
    };
    for value in [rect.x, rect.y, rect.width, rect.height] {
        assert!(value.is_finite(), "{rect:?}");
    }
    assert!(rect.width > 0.0 && rect.height > 0.0, "{rect:?}");
}

struct FixedAdvanceTextMeasurer {
    advance: f32,
    line_height: f32,
    calls: std::cell::Cell<usize>,
}

impl FixedAdvanceTextMeasurer {
    fn shared(advance: f32, line_height: f32) -> Rc<Self> {
        Rc::new(Self {
            advance,
            line_height,
            calls: std::cell::Cell::new(0),
        })
    }
}

impl DrawTextMeasurer for FixedAdvanceTextMeasurer {
    fn measure_text(&self, text: &str, _style: &DrawTextStyle) -> TextMeasurement {
        self.calls.set(self.calls.get() + 1);
        let lines: Vec<&str> = text.split('\n').collect();
        let width = lines
            .iter()
            .map(|line| line.chars().count() as f32 * self.advance)
            .fold(0.0_f32, f32::max);
        TextMeasurement {
            size: Size::new(width, lines.len() as f32 * self.line_height),
            line_height: self.line_height,
            first_baseline: self.line_height * 0.75,
            line_count: lines.len(),
        }
    }
}

fn text_scope(size: Size) -> (DrawScopeDefault, Rc<FixedAdvanceTextMeasurer>) {
    let measurer = FixedAdvanceTextMeasurer::shared(10.0, 20.0);
    (
        DrawScopeDefault::with_text_measurer(size, measurer.clone()),
        measurer,
    )
}

fn unwrap_text(primitive: &DrawPrimitive) -> &TextPrimitive {
    match primitive {
        DrawPrimitive::Text(text) => text,
        other => panic!("expected text primitive, got {other:?}"),
    }
}

#[test]
fn drawn_text_occupies_exactly_the_box_measure_text_reported() {
    let (mut scope, _) = text_scope(Size::new(200.0, 100.0));
    let style = DrawTextStyle::new(16.0);
    let measured = scope.measure_text("ABCD", &style);

    scope.draw_text_from(
        Point::new(7.0, 11.0),
        Brush::solid(Color::WHITE),
        "ABCD",
        &style,
    );

    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 1);
    let text = unwrap_text(&primitives[0]);
    assert_eq!(
        text.rect,
        Rect {
            x: 7.0,
            y: 11.0,
            width: measured.size.width,
            height: measured.size.height,
        },
        "the drawn block must be the measured block, or callers cannot center text"
    );
    assert_eq!(&*text.text, "ABCD");
    assert_eq!(text.color, Color::WHITE);
}

#[test]
fn text_alignment_positions_the_measured_block_inside_the_box() {
    let box_rect = Rect {
        x: 100.0,
        y: 50.0,
        width: 200.0,
        height: 80.0,
    };
    let cases = [
        (TextAlign::Left, TextVerticalAlign::Top, 100.0, 50.0),
        (TextAlign::Center, TextVerticalAlign::Center, 190.0, 80.0),
        (TextAlign::Right, TextVerticalAlign::Bottom, 280.0, 110.0),
    ];
    for (align, vertical_align, expected_x, expected_y) in cases {
        let (mut scope, _) = text_scope(Size::new(400.0, 400.0));
        let style = DrawTextStyle::new(16.0)
            .with_align(align)
            .with_vertical_align(vertical_align);
        scope.draw_text_at(box_rect, Brush::solid(Color::WHITE), "AB", &style);
        let primitives = scope.into_primitives();
        let text = unwrap_text(&primitives[0]);
        assert!(
            approx(text.rect.x, expected_x) && approx(text.rect.y, expected_y),
            "{align:?}/{vertical_align:?} placed the block at {:?}",
            text.rect
        );
        assert!(approx(text.rect.width, 20.0) && approx(text.rect.height, 20.0));
    }
}

#[test]
fn baseline_aligned_text_hangs_above_the_box_edge() {
    let (mut scope, _) = text_scope(Size::new(200.0, 200.0));
    let style = DrawTextStyle::new(16.0).with_vertical_align(TextVerticalAlign::Baseline);
    let measured = scope.measure_text("Ag", &style);
    scope.draw_text_at(
        Rect {
            x: 0.0,
            y: 100.0,
            width: 200.0,
            height: 0.0,
        },
        Brush::solid(Color::WHITE),
        "Ag",
        &style,
    );
    let primitives = scope.into_primitives();
    let text = unwrap_text(&primitives[0]);
    assert!(
        approx(text.rect.y, 100.0 - measured.first_baseline),
        "{:?}",
        text.rect
    );
}

#[test]
fn draw_text_fills_the_whole_scope_rect() {
    let (mut scope, _) = text_scope(Size::new(120.0, 60.0));
    let style = DrawTextStyle::new(16.0)
        .with_align(TextAlign::Right)
        .with_vertical_align(TextVerticalAlign::Bottom);
    scope.draw_text(Brush::solid(Color::WHITE), "AB", &style);
    let primitives = scope.into_primitives();
    let text = unwrap_text(&primitives[0]);
    assert!(
        approx(text.rect.x, 100.0) && approx(text.rect.y, 40.0),
        "{:?}",
        text.rect
    );
}

#[test]
fn draw_text_from_ignores_alignment_and_anchors_the_top_left() {
    let (mut scope, _) = text_scope(Size::new(400.0, 400.0));
    let style = DrawTextStyle::new(16.0)
        .with_align(TextAlign::Center)
        .with_vertical_align(TextVerticalAlign::Bottom);
    scope.draw_text_from(
        Point::new(30.0, 40.0),
        Brush::solid(Color::WHITE),
        "AB",
        &style,
    );
    let primitives = scope.into_primitives();
    let text = unwrap_text(&primitives[0]);
    assert!(
        approx(text.rect.x, 30.0) && approx(text.rect.y, 40.0),
        "{:?}",
        text.rect
    );
}

#[test]
fn multiline_text_measures_the_widest_line_and_stacks_the_lines() {
    let (mut scope, _) = text_scope(Size::new(400.0, 400.0));
    let style = DrawTextStyle::new(16.0);
    scope.draw_text_from(Point::ZERO, Brush::solid(Color::WHITE), "AB\nABCDE", &style);
    let primitives = scope.into_primitives();
    let text = unwrap_text(&primitives[0]);
    assert!(approx(text.rect.width, 50.0), "{:?}", text.rect);
    assert!(approx(text.rect.height, 40.0), "{:?}", text.rect);
}

#[test]
fn empty_text_draws_nothing_and_never_measures() {
    let (mut scope, measurer) = text_scope(Size::new(100.0, 100.0));
    scope.draw_text(Brush::solid(Color::WHITE), "", &DrawTextStyle::new(16.0));
    scope.draw_text_at(
        Rect::from_size(Size::new(10.0, 10.0)),
        Brush::solid(Color::WHITE),
        "",
        &DrawTextStyle::new(16.0),
    );
    scope.draw_text_from(
        Point::ZERO,
        Brush::solid(Color::WHITE),
        "",
        &DrawTextStyle::new(16.0),
    );
    assert!(scope.into_primitives().is_empty());
    assert_eq!(
        measurer.calls.get(),
        0,
        "an empty string must not cost a measurement"
    );
}

#[test]
fn invisible_text_draws_nothing() {
    let (mut scope, _) = text_scope(Size::new(100.0, 100.0));
    let style = DrawTextStyle::new(16.0);
    scope.draw_text(Brush::solid(Color(1.0, 1.0, 1.0, 0.0)), "AB", &style);
    scope.draw_text(
        Brush::LinearGradient {
            colors: Vec::new(),
            stops: None,
            start: Point::ZERO,
            end: Point::new(1.0, 1.0),
            tile_mode: crate::render_effect::TileMode::Clamp,
        },
        "AB",
        &style,
    );
    assert!(scope.into_primitives().is_empty());
}

#[test]
fn gradient_text_brushes_fall_back_to_their_first_stop() {
    let (mut scope, _) = text_scope(Size::new(100.0, 100.0));
    scope.draw_text(
        Brush::linear_gradient(vec![Color::RED, Color::BLUE]),
        "AB",
        &DrawTextStyle::new(16.0),
    );
    let primitives = scope.into_primitives();
    assert_eq!(unwrap_text(&primitives[0]).color, Color::RED);
}

#[test]
fn a_scope_without_a_measurer_falls_back_to_the_font_free_estimate() {
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
    let style = DrawTextStyle::new(16.0);
    assert_eq!(
        scope.measure_text("ABC", &style),
        crate::estimate_text_measurement("ABC", &style)
    );
    scope.draw_text_from(Point::ZERO, Brush::solid(Color::WHITE), "ABC", &style);
    let primitives = scope.into_primitives();
    let text = unwrap_text(&primitives[0]);
    assert!(text.rect.width > 0.0 && text.rect.height > 0.0);
}

#[test]
fn degenerate_text_geometry_emits_nothing_and_never_panics() {
    struct DegenerateTextMeasurer;
    impl DrawTextMeasurer for DegenerateTextMeasurer {
        fn measure_text(&self, _text: &str, _style: &DrawTextStyle) -> TextMeasurement {
            TextMeasurement {
                size: Size::new(f32::NAN, 0.0),
                line_height: f32::NAN,
                first_baseline: f32::NAN,
                line_count: 1,
            }
        }
    }

    let mut scope = DrawScopeDefault::with_text_measurer(
        Size::new(50.0, 50.0),
        Rc::new(DegenerateTextMeasurer),
    );
    scope.draw_text(Brush::solid(Color::WHITE), "AB", &DrawTextStyle::new(16.0));
    scope.draw_text_at(
        Rect {
            x: f32::NAN,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        },
        Brush::solid(Color::WHITE),
        "AB",
        &DrawTextStyle::new(16.0),
    );
    assert!(
        scope.into_primitives().is_empty(),
        "unmeasurable text must not reach the renderer"
    );
}

#[test]
fn text_style_survives_lowering_into_the_primitive() {
    let (mut scope, _) = text_scope(Size::new(100.0, 100.0));
    let style = DrawTextStyle::new(21.0)
        .with_font_family("Fira Sans")
        .with_weight(FontWeight::BOLD)
        .with_style(FontStyle::Italic)
        .with_letter_spacing(2.0)
        .with_line_height(26.0);
    scope.draw_text(Brush::solid(Color::WHITE), "AB", &style);
    let primitives = scope.into_primitives();
    assert_eq!(unwrap_text(&primitives[0]).style, style);
}

#[test]
fn a_layers_composite_alpha_is_a_truncated_byte() {
    for byte in 0..=255u32 {
        let exact = byte as f32 / 255.0;
        assert!(
            (GraphicsLayer::composite_alpha_8bit(exact) - exact).abs() < 1e-6,
            "byte {byte} moved"
        );
        if byte < 255 {
            let nearly_next = (byte as f32 + 0.999) / 255.0;
            assert!(
                (GraphicsLayer::composite_alpha_8bit(nearly_next) - exact).abs() < 1e-6,
                "byte {byte} + 0.999 did not truncate"
            );
        }
    }
    assert_eq!(GraphicsLayer::composite_alpha_8bit(1.0), 1.0);
    assert_eq!(GraphicsLayer::composite_alpha_8bit(0.0), 0.0);
    assert_eq!(GraphicsLayer::composite_alpha_8bit(-3.0), 0.0);
    assert_eq!(GraphicsLayer::composite_alpha_8bit(7.0), 1.0);
}

#[test]
fn inset_rect_keeps_the_padded_part_and_never_goes_negative() {
    let insets = EdgeInsets::from_components(1.0, 2.0, 3.0, 4.0);
    assert_eq!(
        insets.inset_rect(Size::new(10.0, 10.0)),
        Rect {
            x: 1.0,
            y: 2.0,
            width: 6.0,
            height: 4.0,
        }
    );
    assert_eq!(
        insets.inset_rect(Size::new(2.0, 2.0)),
        Rect {
            x: 1.0,
            y: 2.0,
            width: 0.0,
            height: 0.0,
        }
    );
}

#[test]
fn a_translated_primitive_moves_every_position_it_carries() {
    let rect = Rect::from_size(Size::new(4.0, 4.0));
    let moved = rect.translate(2.0, 3.0);
    let fill = DrawPrimitive::Rect {
        rect,
        brush: Brush::solid(Color::RED),
        stroke: None,
    };
    let arc = DrawPrimitive::Arc {
        rect,
        brush: Brush::solid(Color::RED),
        center: Point::new(2.0, 2.0),
        radius: 2.0,
        start_angle: 0.0,
        sweep_angle: 1.0,
        stroke: None,
        inner_radius: 0.0,
    };
    assert_eq!(
        DrawPrimitive::Blend {
            primitive: Box::new(fill.clone()),
            blend_mode: BlendMode::Multiply,
        }
        .translate(2.0, 3.0),
        DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::Rect {
                rect: moved,
                brush: Brush::solid(Color::RED),
                stroke: None,
            }),
            blend_mode: BlendMode::Multiply,
        }
    );
    let DrawPrimitive::Arc {
        rect: arc_rect,
        center,
        ..
    } = arc.translate(2.0, 3.0)
    else {
        panic!("an arc stays an arc");
    };
    assert_eq!((arc_rect, center), (moved, Point::new(4.0, 5.0)));
    let DrawPrimitive::Shadow(ShadowPrimitive::Inner {
        fill: inner,
        cutout,
        clip_rect,
        ..
    }) = DrawPrimitive::Shadow(ShadowPrimitive::Inner {
        fill: Box::new(fill.clone()),
        cutout: Box::new(fill.clone()),
        blur_radius: 1.0,
        blend_mode: BlendMode::SrcOver,
        clip_rect: rect,
    })
    .translate(2.0, 3.0)
    else {
        panic!("an inner shadow stays an inner shadow");
    };
    assert_eq!(clip_rect, moved);
    for shape in [*inner, *cutout] {
        assert!(matches!(shape, DrawPrimitive::Rect { rect, .. } if rect == moved));
    }
    let DrawPrimitive::Shadow(ShadowPrimitive::Drop { shape, cutout, .. }) =
        DrawPrimitive::Shadow(ShadowPrimitive::Drop {
            shape: Box::new(fill.clone()),
            cutout: Some(Box::new(fill)),
            blur_radius: 1.0,
            blend_mode: BlendMode::SrcOver,
        })
        .translate(2.0, 3.0)
    else {
        panic!("a drop shadow stays a drop shadow");
    };
    assert!(matches!(*shape, DrawPrimitive::Rect { rect, .. } if rect == moved));
    assert!(matches!(cutout.as_deref(), Some(DrawPrimitive::Rect { rect, .. }) if *rect == moved));
    let image = ImageBitmap::from_rgba8(1, 1, vec![255; 4]).expect("image");
    assert!(matches!(
        DrawPrimitive::Image {
            rect,
            image,
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Linear,
            src_rect: Some(rect),
        }
        .translate(2.0, 3.0),
        DrawPrimitive::Image { rect: image_rect, src_rect: Some(source), .. }
            if image_rect == moved && source == rect
    ));
    assert_eq!(
        DrawPrimitive::Content.translate(2.0, 3.0),
        DrawPrimitive::Content
    );
}

#[test]
fn an_inset_scope_draws_inside_the_insets_and_keeps_content_markers() {
    let mut scope = DrawScopeDefault::new(Size::new(20.0, 10.0));
    let mut seen = None;
    scope.inset(EdgeInsets::from_components(1.0, 2.0, 3.0, 4.0), |inner| {
        seen = Some(inner.size());
        inner.draw_content();
        inner.draw_round_rect(Brush::solid(Color::RED), CornerRadii::uniform(2.0));
    });
    assert_eq!(seen, Some(Size::new(16.0, 4.0)));
    assert_eq!(scope.content_marker_count(), 1);
    let primitives = scope.into_primitives();
    assert_eq!(primitives[0], DrawPrimitive::Content);
    assert!(matches!(
        primitives[1],
        DrawPrimitive::RoundRect { rect, .. } if rect == Rect { x: 1.0, y: 2.0, width: 16.0, height: 4.0 }
    ));
}
