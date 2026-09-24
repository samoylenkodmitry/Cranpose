use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
use cranpose_ui::{
    text::TextMotion,
    text_layout_result::{GlyphLayout, LineLayout, TextLayoutData, TextLayoutResult},
};

use super::*;
use crate::scene::CompositorScene as Scene;

#[test]
fn shadow_casters_preserve_the_requested_cutout_blend() {
    let mut recorder = Arc::new(ShapeRecorder::default());
    let primitive = DrawPrimitive::Rect {
        rect: Rect {
            x: 2.0,
            y: 3.0,
            width: 8.0,
            height: 9.0,
        },
        brush: Brush::solid(Color::WHITE),
        stroke: None,
    };
    assert!(record_shadow_caster(
        &mut recorder,
        &primitive,
        &GraphicsLayer::default(),
        BlendMode::DstOut,
    ));
    assert_eq!(recorder.tables().segments.len(), 1);
    assert_eq!(recorder.tables().segments[0].blend, BlendMode::DstOut);
    assert_eq!(
        recorder.tables().shapes.get(0).unwrap().blend_mode(),
        BlendMode::DstOut
    );
}

fn synthetic_text_layout(
    text: &str,
    line_height: f32,
    lines: Vec<LineLayout>,
    glyph_layouts: Vec<GlyphLayout>,
) -> TextLayoutResult {
    let mut glyph_x_positions = Vec::new();
    let mut char_to_byte = Vec::new();
    for (byte_offset, _) in text.char_indices() {
        glyph_x_positions.push(0.0);
        char_to_byte.push(byte_offset);
    }
    glyph_x_positions.push(0.0);
    char_to_byte.push(text.len());

    let width = glyph_layouts
        .iter()
        .map(|glyph| (glyph.x + glyph.width).max(0.0))
        .fold(0.0, f32::max);
    let height = lines
        .iter()
        .map(|line| (line.y + line.height).max(0.0))
        .fold(line_height.max(0.0), f32::max);

    TextLayoutResult::new(
        text,
        TextLayoutData {
            width,
            height,
            line_height,
            glyph_x_positions,
            char_to_byte,
            lines,
            glyph_layouts,
        },
    )
}

fn with_test_app_context<R>(block: impl FnOnce() -> R) -> R {
    let app_context = cranpose_ui::AppContext::new();
    app_context.enter(block)
}

fn prepare_text_layout_for_test(
    text: &cranpose_ui::text::AnnotatedString,
    style: &TextStyle,
    options: TextLayoutOptions,
    max_width: Option<f32>,
) -> cranpose_ui::text::PreparedTextLayout {
    with_test_app_context(|| prepare_text_layout(text, style, options, max_width))
}

fn measure_text_for_test(
    text: &cranpose_ui::text::AnnotatedString,
    style: &TextStyle,
) -> cranpose_ui::TextMetrics {
    with_test_app_context(|| measure_text(text, style))
}

#[allow(clippy::too_many_arguments)]
fn push_text_style_draws_for_test(
    scene: &mut Scene,
    node_id: NodeId,
    rect: Rect,
    text_rect: Rect,
    content_layer: &GraphicsLayer,
    text: &cranpose_ui::text::AnnotatedString,
    text_style: &TextStyle,
    font_size: f32,
    options: TextLayoutOptions,
    text_clip: Option<Rect>,
) {
    with_test_app_context(|| {
        let mut text_layout = UiTextLayoutResolver;
        let text = Rc::new(text.clone());
        push_text_style_draws(
            scene,
            &mut text_layout,
            node_id,
            rect,
            text_rect,
            content_layer,
            &text,
            text_style,
            font_size,
            options,
            text_clip,
            None,
        );
    });
}

/// Every recorded shape of the scene with the placement it draws
/// under, the open loose run closed first.
fn shape_records(
    scene: &mut Scene,
) -> Vec<(cranpose_ui_graphics::ShapeRecord, crate::scene::Placement)> {
    scene.flush_loose();
    scene
        .runs
        .iter()
        .flat_map(|run| {
            run.tables()
                .shapes
                .iter()
                .map(move |record| (record, run.placement))
        })
        .collect()
}

fn record_rect(entry: &(cranpose_ui_graphics::ShapeRecord, crate::scene::Placement)) -> Rect {
    entry.1.translated_bounds(entry.0.stored_rect())
}

fn scene_bounds_for_test(scene: &mut Scene) -> Option<Rect> {
    scene.flush_loose();
    let mut bounds = None;
    for run in &scene.runs {
        bounds = union_rect(bounds, run.bounds);
    }
    for image in &scene.images {
        bounds = union_rect(bounds, image.rect);
    }
    for text in &scene.texts {
        bounds = union_rect(bounds, text.rect);
    }
    for shadow in &scene.shadow_draws {
        let mut shadow_bounds = None;
        if let Some(run) = &shadow.shapes {
            shadow_bounds = union_rect(shadow_bounds, run.bounds);
        }
        for text in &shadow.texts {
            shadow_bounds = union_rect(shadow_bounds, text.rect);
        }
        if let Some(shadow_bounds) = shadow_bounds {
            let shadow_bounds =
                expand_blurred_rect(shadow_bounds, shadow.blur_radius, 1.0, shadow.clip);
            if let Some(shadow_bounds) = shadow_bounds {
                bounds = union_rect(bounds, shadow_bounds);
            }
        }
    }
    for layer in &scene.effect_layers {
        bounds = union_rect(bounds, layer.rect);
    }
    for layer in &scene.backdrop_layers {
        bounds = union_rect(bounds, layer.rect);
    }
    bounds
}

#[test]
fn shadow_geometry_has_visible_expansion_and_offsets() {
    let mut scene = Scene::new();
    let layer = GraphicsLayer {
        shadow_elevation: 10.0,
        ambient_shadow_color: Color(0.2, 0.3, 0.4, 0.8),
        spot_shadow_color: Color(0.7, 0.6, 0.5, 0.9),
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
        ..Default::default()
    };
    let bounds = Rect {
        x: 20.0,
        y: 30.0,
        width: 40.0,
        height: 24.0,
    };

    push_layer_shadow(&mut scene, &layer, bounds, bounds, None);

    assert!(
        scene.shadow_draws.len() >= 2,
        "elevation shadow should emit ambient + spot blur draws"
    );

    let ambient = &scene.shadow_draws[0];
    assert!(
        ambient.blur_radius > 0.0,
        "ambient shadow should have a blur radius"
    );
    let ambient_shape = ambient.shapes.as_ref().expect("ambient caster");
    assert!(
        ambient_shape.bounds.x <= bounds.x - 2.0,
        "ambient shadow should clearly expand left"
    );
    assert!(
        ambient_shape.bounds.width > bounds.width,
        "ambient shadow should clearly expand width"
    );
    let ambient_peak_alpha = ambient_shape.tables().shapes.get(0).unwrap().color[3];
    assert!(
        ambient_peak_alpha > 0.02,
        "ambient alpha should remain visible"
    );

    let spot = &scene.shadow_draws[1];
    assert!(spot.blur_radius > 0.0, "spot shadow should have blur");
    let spot_shape = spot.shapes.as_ref().expect("spot caster");
    assert!(
        spot_shape.bounds.y > bounds.y,
        "spot shadow should be offset downward from source bounds"
    );
    let spot_alpha = spot_shape.tables().shapes.get(0).unwrap().color[3];
    assert!(spot_alpha > 0.02, "spot alpha should remain visible");
}

#[test]
fn graphics_layer_clip_is_not_reused_for_shadow_clip() {
    let bounds = Rect {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 18.0,
    };
    let content_clip = resolve_clip(None, Some(bounds));
    let shadow_clip = resolve_clip(None, None);
    assert_eq!(content_clip, Some(bounds));
    assert_eq!(
        shadow_clip, None,
        "graphics-layer clip should not clip layer shadow geometry"
    );
}

#[test]
fn clip_to_bounds_clips_shadow_and_content() {
    let parent = Rect {
        x: 0.0,
        y: 0.0,
        width: 40.0,
        height: 40.0,
    };
    let bounds = Rect {
        x: 20.0,
        y: 20.0,
        width: 30.0,
        height: 30.0,
    };
    let content_clip = resolve_clip(Some(parent), Some(bounds)).expect("content clip");
    let shadow_clip = resolve_clip(Some(parent), Some(bounds)).expect("shadow clip");
    assert_eq!(content_clip, shadow_clip);
    assert_eq!(
        content_clip,
        Rect {
            x: 20.0,
            y: 20.0,
            width: 20.0,
            height: 20.0,
        }
    );
}

#[test]
fn collect_hits_from_graph_only_populates_hit_regions() {
    let layer = cranpose_render_common::graph::LayerNode {
        node_id: Some(7),
        wraps: None,
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 24.0,
        },
        transform_to_parent: cranpose_render_common::graph::ProjectiveTransform::translation(
            12.0, 8.0,
        ),
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        content_offset: Point::default(),
        scene_children_origin: cranpose_ui_graphics::Point::default(),
        scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
        graphics_layer: GraphicsLayer::default(),
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: Some(cranpose_render_common::graph::HitTestNode {
            shape: None,
            click_actions: vec![Rc::new(|_point| {})],
            pointer_inputs: vec![],
            pointer_icon: None,
            clip: None,
        }),
        has_hit_targets: true,
        has_origin_sinks: false,
        isolation: cranpose_render_common::graph::IsolationReasons::default(),
        cache_policy: cranpose_render_common::graph::CachePolicy::None,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children: vec![],
    };
    let mut scene = crate::scene::Scene::new();

    collect_hits_from_graph(
        &layer,
        cranpose_render_common::graph::ProjectiveTransform::identity(),
        &mut scene,
        None,
    );

    assert_eq!(scene.hits.len(), 1);
    assert!(scene.graph.is_none());
}

#[test]
fn expand_text_bounds_for_baseline_shift_superscript_extends_top() {
    let style = TextStyle {
        span_style: cranpose_ui::text::SpanStyle {
            baseline_shift: Some(cranpose_ui::text::BaselineShift::SUPERSCRIPT),
            ..Default::default()
        },
        ..Default::default()
    };
    let text_bounds = Rect {
        x: 20.0,
        y: 20.0,
        width: 50.0,
        height: 18.0,
    };
    let expanded = expand_text_bounds_for_baseline_shift(text_bounds, &style, 20.0);
    assert!(expanded.y < text_bounds.y);
    assert!(expanded.height > text_bounds.height);
    assert_eq!(
        expanded.y + expanded.height,
        text_bounds.y + text_bounds.height
    );
}

#[test]
fn resolve_text_measure_width_expands_for_multiline_clip_text() {
    let padding = EdgeInsets {
        left: 4.0,
        top: 0.0,
        right: 4.0,
        bottom: 0.0,
    };
    let width = resolve_text_measure_width(130.0, padding, Some(180.0));
    assert!((width - 172.0).abs() < f32::EPSILON);
}

#[test]
fn resolve_text_measure_width_respects_tighter_measurement_constraint() {
    let padding = EdgeInsets {
        left: 4.0,
        top: 0.0,
        right: 4.0,
        bottom: 0.0,
    };
    let width = resolve_text_measure_width(130.0, padding, Some(100.0));
    assert!((width - 92.0).abs() < f32::EPSILON);
}

#[test]
fn resolve_text_measure_width_falls_back_to_content_width_without_constraint() {
    let padding = EdgeInsets {
        left: 4.0,
        top: 0.0,
        right: 4.0,
        bottom: 0.0,
    };
    let width = resolve_text_measure_width(130.0, padding, None);
    assert!((width - 130.0).abs() < f32::EPSILON);
}

#[test]
fn resolve_text_horizontal_offset_centers_text() {
    let style = cranpose_ui::TextStyle {
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_align: cranpose_ui::text::TextAlign::Center,
            ..Default::default()
        },
        ..Default::default()
    };
    let offset = resolve_text_horizontal_offset(&style, "hello", 120.0, 80.0);
    assert!((offset - 20.0).abs() < f32::EPSILON);
}

#[test]
fn resolve_text_horizontal_offset_uses_rtl_start() {
    let style = cranpose_ui::TextStyle {
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_align: cranpose_ui::text::TextAlign::Start,
            text_direction: cranpose_ui::text::TextDirection::Rtl,
            ..Default::default()
        },
        ..Default::default()
    };
    let offset = resolve_text_horizontal_offset(&style, "hello", 120.0, 80.0);
    assert!((offset - 40.0).abs() < f32::EPSILON);
}

#[test]
fn resolve_text_horizontal_offset_uses_start_for_unspecified_align() {
    let style = cranpose_ui::TextStyle {
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_align: cranpose_ui::text::TextAlign::Unspecified,
            text_direction: cranpose_ui::text::TextDirection::Rtl,
            ..Default::default()
        },
        ..Default::default()
    };
    let offset = resolve_text_horizontal_offset(&style, "hello", 120.0, 80.0);
    assert!((offset - 40.0).abs() < f32::EPSILON);
}

#[test]
fn measurement_constraint_width_prevents_spurious_wrap() {
    let padding = EdgeInsets {
        left: 4.0,
        top: 0.0,
        right: 4.0,
        bottom: 0.0,
    };
    let text = "Dynamic Modifiers";
    let style = cranpose_ui::TextStyle::default();
    let options = cranpose_ui::TextLayoutOptions::default();
    let content_width = 130.0;

    let wrapped_by_content = prepare_text_layout_for_test(
        &cranpose_ui::text::AnnotatedString::from(text),
        &style,
        options,
        Some(content_width),
    )
    .text;
    assert!(
        wrapped_by_content.text.contains('\n'),
        "control check expected wrapping at content width: {wrapped_by_content:?}"
    );

    let measure_width = resolve_text_measure_width(content_width, padding, Some(180.0));
    let prepared = prepare_text_layout_for_test(
        &cranpose_ui::text::AnnotatedString::from(text),
        &style,
        options,
        Some(measure_width),
    );
    assert!(
        !prepared.text.text.contains('\n'),
        "measurement width should prevent synthetic wrap: {:?}",
        prepared.text
    );
}

#[test]
fn text_has_visible_decoration_detects_global_and_span_styles() {
    let plain_style = cranpose_ui::TextStyle::default();
    let plain_text = cranpose_ui::text::AnnotatedString::from("Plain");
    assert!(!text_has_visible_decoration(&plain_text, &plain_style));

    let decorated_global_style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(text_has_visible_decoration(
        &plain_text,
        &decorated_global_style
    ));

    let span_decorated_text = cranpose_ui::text::AnnotatedString::builder()
        .push_style(cranpose_ui::text::SpanStyle {
            text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ..Default::default()
        })
        .append("Span")
        .pop()
        .to_annotated_string();
    assert!(text_has_visible_decoration(
        &span_decorated_text,
        &plain_style
    ));
}

#[test]
fn push_text_style_draws_emits_background_shadow_and_main_text() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(Color(0.9, 0.95, 1.0, 1.0)),
            background: Some(Color(0.2, 0.3, 0.52, 0.55)),
            shadow: Some(cranpose_ui::text::Shadow {
                color: Color(0.0, 0.0, 0.0, 0.95),
                offset: Point::new(2.0, 2.0),
                blur_radius: 3.0,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 10.0,
        width: 180.0,
        height: 28.0,
    };
    let clip = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 200.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        7 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Decorated shadow text"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        Some(clip),
    );

    let records = shape_records(&mut scene);
    assert_eq!(records.len(), 1, "span background should emit one shape");
    let expected = Color(0.2, 0.3, 0.52, 0.55).srgb_8bit();
    assert_eq!(
        records[0].0.color,
        [expected.0, expected.1, expected.2, expected.3]
    );

    assert_eq!(scene.texts.len(), 1, "content text expected");
    assert_eq!(scene.shadow_draws.len(), 1, "shadow draw expected");
    assert_eq!(scene.shadow_draws[0].texts.len(), 1, "shadow text expected");
    assert_eq!(
        scene.shadow_draws[0].texts[0].color,
        Color(0.0, 0.0, 0.0, 0.95).srgb_8bit()
    );
    assert_eq!(scene.texts[0].color, Color(0.9, 0.95, 1.0, 1.0).srgb_8bit());
    assert!(scene.shadow_draws[0].texts[0].rect.x > scene.texts[0].rect.x);
    assert!(scene.shadow_draws[0].texts[0].rect.y > scene.texts[0].rect.y);
    assert!(
        scene.effect_layers.is_empty(),
        "blurred text shadow does not use effect layer"
    );
    assert_eq!(
        scene.shadow_draws[0].blur_radius, 3.0,
        "shadow uses blurred shadow draw radius"
    );
}

#[test]
fn push_text_style_draws_hard_shadow_does_not_emit_effect_layer() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(Color(0.9, 0.95, 1.0, 1.0)),
            shadow: Some(cranpose_ui::text::Shadow {
                color: Color(0.0, 0.0, 0.0, 0.95),
                offset: Point::new(2.0, 2.0),
                blur_radius: 0.0,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 10.0,
        width: 180.0,
        height: 28.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        7 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Hard shadow"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(scene.texts.len(), 1, "content text expected");
    assert_eq!(scene.shadow_draws.len(), 1, "shadow draw expected");
    assert_eq!(scene.shadow_draws[0].texts.len(), 1, "shadow text expected");
    assert_eq!(
        scene.shadow_draws[0].blur_radius, 0.0,
        "hard shadow blur radius"
    );
    assert!(
        scene.effect_layers.is_empty(),
        "hard shadow should not allocate blur effect layer"
    );
}

#[test]
fn estimate_text_style_draw_bounds_matches_scene_emission() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            brush: Some(Brush::linear_gradient_range(
                vec![Color(0.2, 0.9, 1.0, 1.0), Color(1.0, 0.6, 0.4, 1.0)],
                Point::new(0.0, 0.0),
                Point::new(140.0, 0.0),
            )),
            shadow: Some(cranpose_ui::text::Shadow {
                color: Color(0.0, 0.0, 0.0, 0.85),
                offset: Point::new(2.5, 1.5),
                blur_radius: 3.0,
            }),
            text_decoration: Some(TextDecoration::UNDERLINE),
            baseline_shift: Some(cranpose_ui::text::BaselineShift::SUPERSCRIPT),
            ..Default::default()
        },
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_motion: Some(TextMotion::Static),
            ..Default::default()
        },
    };
    let text = cranpose_ui::text::AnnotatedString::from("Layer text bounds");
    let rect = Rect {
        x: 8.25,
        y: 12.5,
        width: 180.0,
        height: 30.0,
    };
    let clip = Some(Rect {
        x: 0.0,
        y: 0.0,
        width: 140.0,
        height: 48.0,
    });

    push_text_style_draws_for_test(
        &mut scene,
        31 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        clip,
    );

    let estimated = with_test_app_context(|| {
        estimate_text_style_draw_bounds(
            31 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &text,
            &style,
            14.0,
            TextLayoutOptions::default(),
            clip,
        )
    });

    assert_eq!(estimated, scene_bounds_for_test(&mut scene));
}

#[test]
fn push_text_style_draws_emits_decoration_shapes() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(Color(0.9, 0.95, 1.0, 1.0)),
            text_decoration: Some(
                cranpose_ui::text::TextDecoration::UNDERLINE
                    .combine(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 10.0,
        width: 180.0,
        height: 28.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        7 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Decorated"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(
        shape_records(&mut scene).len(),
        2,
        "underline + line-through expected"
    );
    assert_eq!(scene.texts.len(), 1, "main text expected");
}

#[test]
fn push_text_style_draws_preserves_subpixel_decoration_y() {
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = cranpose_ui::text::AnnotatedString::from("Underlined");
    let base_rect = Rect {
        x: 8.0,
        y: 10.10,
        width: 180.0,
        height: 28.0,
    };
    let shifted_rect = Rect {
        y: 10.45,
        ..base_rect
    };

    let mut base_scene = Scene::new();
    let mut shifted_scene = Scene::new();
    push_text_style_draws_for_test(
        &mut base_scene,
        70 as NodeId,
        base_rect,
        base_rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );
    push_text_style_draws_for_test(
        &mut shifted_scene,
        71 as NodeId,
        shifted_rect,
        shifted_rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    let base = shape_records(&mut base_scene);
    let shifted = shape_records(&mut shifted_scene);
    assert_eq!(base.len(), 1);
    assert_eq!(shifted.len(), 1);
    let delta = record_rect(&shifted[0]).y - record_rect(&base[0]).y;
    assert!(
        (delta - 0.35).abs() < 0.001,
        "text decoration y must move by the same fractional delta as the text; got {delta}"
    );
}

#[test]
fn push_text_style_draws_emits_multiline_decoration_shapes_per_visual_line() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = cranpose_ui::text::AnnotatedString::from("One\nTwo\nThree");
    let rect = Rect {
        x: 8.0,
        y: 10.0,
        width: 220.0,
        height: 72.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        22 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    let records = shape_records(&mut scene);
    assert_eq!(records.len(), 3, "one underline per visual line");
    let line_height = measure_text_for_test(&text, &style).line_height.max(1.0);
    let mut ys: Vec<f32> = records.iter().map(|entry| record_rect(entry).y).collect();
    ys.sort_by(f32::total_cmp);
    assert!(ys[1] > ys[0], "second underline should be below first line");
    assert!(ys[2] > ys[1], "third underline should be below second line");
    assert!(
        ((ys[1] - ys[0]) - line_height).abs() <= line_height * 0.35,
        "line 1->2 decoration delta should follow measured line height"
    );
    assert!(
        ((ys[2] - ys[1]) - line_height).abs() <= line_height * 0.35,
        "line 2->3 decoration delta should follow measured line height"
    );
}

#[test]
fn decoration_segments_from_glyph_layouts_line_through_preserves_bidi_visual_order() {
    let style = cranpose_ui::TextStyle::default();
    let text = cranpose_ui::text::AnnotatedString::builder()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ..Default::default()
        })
        .append("A")
        .pop()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color(0.0, 0.8, 0.0, 1.0)),
            text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ..Default::default()
        })
        .append("B")
        .pop()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::BLUE),
            text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ..Default::default()
        })
        .append("C")
        .pop()
        .to_annotated_string();

    let layout = synthetic_text_layout(
        "ABC",
        12.0,
        vec![LineLayout {
            start_offset: 0,
            end_offset: 3,
            y: 0.0,
            height: 12.0,
        }],
        vec![
            GlyphLayout {
                line_index: 0,
                start_offset: 0,
                end_offset: 1,
                x: 20.0,
                y: 0.0,
                width: 10.0,
                height: 12.0,
            },
            GlyphLayout {
                line_index: 0,
                start_offset: 1,
                end_offset: 2,
                x: 10.0,
                y: 0.0,
                width: 10.0,
                height: 12.0,
            },
            GlyphLayout {
                line_index: 0,
                start_offset: 2,
                end_offset: 3,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 12.0,
            },
        ],
    );

    let segments = decoration_segments_from_glyph_layouts(&text, &style, &layout);
    assert_eq!(
        segments.len(),
        3,
        "each differently styled glyph should emit one segment"
    );

    let red = segments
        .iter()
        .find(|segment| segment.span_style.color == Some(Color::RED))
        .expect("red segment");
    let green = segments
        .iter()
        .find(|segment| segment.span_style.color == Some(Color(0.0, 0.8, 0.0, 1.0)))
        .expect("green segment");
    let blue = segments
        .iter()
        .find(|segment| segment.span_style.color == Some(Color::BLUE))
        .expect("blue segment");

    assert!((red.x_start - 20.0).abs() < f32::EPSILON);
    assert!((green.x_start - 10.0).abs() < f32::EPSILON);
    assert!((blue.x_start - 0.0).abs() < f32::EPSILON);
    assert!(
        blue.x_start < green.x_start && green.x_start < red.x_start,
        "segments must follow visual x-order instead of logical span order"
    );
}

#[test]
fn decoration_segments_from_glyph_layouts_multiline_line_through_keeps_visual_line_boxes() {
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ..Default::default()
        },
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_align: cranpose_ui::text::TextAlign::End,
            ..Default::default()
        },
    };
    let text = cranpose_ui::text::AnnotatedString::from("AB\nCD");
    let layout = synthetic_text_layout(
        "AB\nCD",
        12.0,
        vec![
            LineLayout {
                start_offset: 0,
                end_offset: 2,
                y: 0.0,
                height: 12.0,
            },
            LineLayout {
                start_offset: 3,
                end_offset: 5,
                y: 22.0,
                height: 18.0,
            },
        ],
        vec![
            GlyphLayout {
                line_index: 0,
                start_offset: 0,
                end_offset: 1,
                x: 28.0,
                y: 0.0,
                width: 10.0,
                height: 12.0,
            },
            GlyphLayout {
                line_index: 0,
                start_offset: 1,
                end_offset: 2,
                x: 38.0,
                y: 0.0,
                width: 10.0,
                height: 12.0,
            },
            GlyphLayout {
                line_index: 1,
                start_offset: 3,
                end_offset: 4,
                x: 6.0,
                y: 22.0,
                width: 10.0,
                height: 18.0,
            },
            GlyphLayout {
                line_index: 1,
                start_offset: 4,
                end_offset: 5,
                x: 16.0,
                y: 22.0,
                width: 10.0,
                height: 18.0,
            },
        ],
    );

    let segments = decoration_segments_from_glyph_layouts(&text, &style, &layout);
    assert_eq!(
        segments.len(),
        2,
        "adjacent same-style glyphs should merge to one segment per visual line"
    );
    assert!((segments[0].x_start - 28.0).abs() < f32::EPSILON);
    assert!((segments[0].x_end - 48.0).abs() < f32::EPSILON);
    assert!((segments[0].line_top - 0.0).abs() < f32::EPSILON);
    assert!((segments[0].line_height - 12.0).abs() < f32::EPSILON);
    assert!((segments[1].x_start - 6.0).abs() < f32::EPSILON);
    assert!((segments[1].x_end - 26.0).abs() < f32::EPSILON);
    assert!((segments[1].line_top - 22.0).abs() < f32::EPSILON);
    assert!((segments[1].line_height - 18.0).abs() < f32::EPSILON);
    assert!(
        segments[1].line_top - segments[0].line_top > 20.0,
        "line-through boxes should preserve measured wrapped line spacing"
    );
}

#[test]
fn decoration_segments_from_glyph_layouts_bridge_letter_spacing_gaps() {
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = cranpose_ui::text::AnnotatedString::from("AB");
    let layout = synthetic_text_layout(
        "AB",
        12.0,
        vec![LineLayout {
            start_offset: 0,
            end_offset: 2,
            y: 0.0,
            height: 12.0,
        }],
        vec![
            GlyphLayout {
                line_index: 0,
                start_offset: 0,
                end_offset: 1,
                x: 0.0,
                y: 0.0,
                width: 9.0,
                height: 12.0,
            },
            GlyphLayout {
                line_index: 0,
                start_offset: 1,
                end_offset: 2,
                x: 14.0,
                y: 0.0,
                width: 9.0,
                height: 12.0,
            },
        ],
    );

    let segments = decoration_segments_from_glyph_layouts(&text, &style, &layout);
    assert_eq!(
        segments.len(),
        1,
        "letter spacing must not split one decorated run into dashed line segments"
    );
    assert!((segments[0].x_start - 0.0).abs() < f32::EPSILON);
    assert!((segments[0].x_end - 23.0).abs() < f32::EPSILON);
}

#[test]
fn decoration_segments_from_glyph_layouts_bridge_whitespace_gaps() {
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            text_decoration: Some(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = cranpose_ui::text::AnnotatedString::from("A B");
    let layout = synthetic_text_layout(
        "A B",
        12.0,
        vec![LineLayout {
            start_offset: 0,
            end_offset: 3,
            y: 0.0,
            height: 12.0,
        }],
        vec![
            GlyphLayout {
                line_index: 0,
                start_offset: 0,
                end_offset: 1,
                x: 0.0,
                y: 0.0,
                width: 9.0,
                height: 12.0,
            },
            GlyphLayout {
                line_index: 0,
                start_offset: 2,
                end_offset: 3,
                x: 18.0,
                y: 0.0,
                width: 9.0,
                height: 12.0,
            },
        ],
    );

    let segments = decoration_segments_from_glyph_layouts(&text, &style, &layout);
    assert_eq!(
        segments.len(),
        1,
        "whitespace inside one decorated run must not break line-through geometry"
    );
    assert!((segments[0].x_start - 0.0).abs() < f32::EPSILON);
    assert!((segments[0].x_end - 27.0).abs() < f32::EPSILON);
}

#[test]
fn push_text_style_draws_resolves_decoration_brush_with_span_alpha_and_layer_alpha() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle::default();
    let text = cranpose_ui::text::AnnotatedString::builder()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            alpha: Some(0.4),
            text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
            ..Default::default()
        })
        .append("Tinted")
        .pop()
        .to_annotated_string();
    let rect = Rect {
        x: 8.0,
        y: 10.0,
        width: 180.0,
        height: 28.0,
    };
    let layer = GraphicsLayer {
        alpha: 0.5,
        ..Default::default()
    };

    push_text_style_draws_for_test(
        &mut scene,
        23 as NodeId,
        rect,
        rect,
        &layer,
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    let records = shape_records(&mut scene);
    assert_eq!(records.len(), 1, "one underline expected");
    let color = records[0].0.color;
    assert!((color[0] - 1.0).abs() < 1e-6);
    assert!(color[1] < 1e-6);
    assert!(color[2] < 1e-6);
    assert!(
        (color[3] - 0.2).abs() < 1e-3,
        "span alpha and layer alpha should both modulate decoration alpha"
    );
}

#[test]
fn push_text_style_draws_baseline_shift_offsets_decoration_y_position() {
    let mut base_scene = Scene::new();
    let mut shifted_scene = Scene::new();
    let base_style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
            ..Default::default()
        },
        ..Default::default()
    };
    let shifted_style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            baseline_shift: Some(cranpose_ui::text::BaselineShift::SUPERSCRIPT),
            text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 18.0,
        width: 180.0,
        height: 28.0,
    };
    let text = cranpose_ui::text::AnnotatedString::from("Shifted");

    push_text_style_draws_for_test(
        &mut base_scene,
        24 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &base_style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );
    push_text_style_draws_for_test(
        &mut shifted_scene,
        25 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &shifted_style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    let base = shape_records(&mut base_scene);
    let shifted = shape_records(&mut shifted_scene);
    assert_eq!(base.len(), 1, "base underline expected");
    assert_eq!(shifted.len(), 1, "shifted underline expected");
    assert!(
        record_rect(&shifted[0]).y < record_rect(&base[0]).y,
        "baseline shift should move decoration geometry with shifted text"
    );
}

#[test]
fn push_text_style_draws_applies_baseline_shift() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(Color(0.9, 0.95, 1.0, 1.0)),
            baseline_shift: Some(cranpose_ui::text::BaselineShift::SUPERSCRIPT),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 180.0,
        height: 28.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        7 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Shifted"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(scene.texts.len(), 1);
    assert!(
        scene.texts[0].rect.y < rect.y,
        "superscript baseline shift should move text up"
    );
}

#[test]
fn push_text_style_draws_non_solid_brush_contract_uses_gpu_shader_mask() {
    let mut scene = Scene::new();
    let first_stop = Color(1.0, 0.0, 0.0, 1.0);
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            brush: Some(Brush::linear_gradient_range(
                vec![first_stop, Color(0.0, 0.0, 1.0, 1.0)],
                Point::new(0.0, 0.0),
                Point::new(180.0, 0.0),
            )),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 180.0,
        height: 28.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        7 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Gradient text"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(
        scene.texts.len(),
        1,
        "gradient text should use raster text mask"
    );
    assert_eq!(
        scene.effect_layers.len(),
        1,
        "gradient text should be wrapped in a shader effect layer"
    );
    assert_eq!(
        scene.images.len(),
        0,
        "gradient text should not fall back to software raster image draws"
    );

    let layer = &scene.effect_layers[0];
    assert_eq!(layer.z_start + 1, layer.z_end);
    let Some(RenderEffect::Shader { shader }) = layer.effect.as_ref() else {
        panic!("expected runtime shader effect for non-solid brush text");
    };
    let uniforms = shader.uniforms();

    let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
    assert!((uniform(0) - 0.0).abs() < f32::EPSILON, "linear brush type");
    assert!(
        (uniform(1) - 2.0).abs() < f32::EPSILON,
        "two gradient stops"
    );

    let stop0_color_index = GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT * 4;
    let stop1_color_index = (GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT + 2) * 4;
    assert!(
        uniform(stop0_color_index) > 0.95 && uniform(stop0_color_index + 2) < 0.05,
        "first stop should remain red-dominant in shader uniforms"
    );
    assert!(
        uniform(stop1_color_index + 2) > 0.95 && uniform(stop1_color_index) < 0.05,
        "second stop should remain blue-dominant in shader uniforms"
    );
}

#[test]
fn push_text_style_draws_default_linear_gradient_fill_resolves_infinite_endpoints() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            brush: Some(Brush::linear_gradient(vec![
                Color(0.42, 0.94, 1.0, 1.0),
                Color(1.0, 0.76, 0.54, 1.0),
            ])),
            draw_style: Some(cranpose_ui::text::TextDrawStyle::Fill),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 220.0,
        height: 32.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        13 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Gradient fill"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(
        scene.images.len(),
        0,
        "gradient fill should not route through software image rendering"
    );
    assert_eq!(
        scene.effect_layers.len(),
        1,
        "gradient fill should emit runtime shader effect layer"
    );
    let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
        panic!("expected runtime shader effect for gradient fill");
    };

    let uniforms = shader.uniforms();
    let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
    let start_x = uniform(8);
    let start_y = uniform(9);
    let end_x = uniform(10);
    let end_y = uniform(11);

    assert!(start_x.is_finite() && start_y.is_finite());
    assert!(end_x.is_finite() && end_y.is_finite());
    assert!(end_x > start_x, "x endpoint should span gradient range");
    assert!(end_y > start_y, "y endpoint should span gradient range");
}

#[test]
fn push_text_style_draws_stroke_contract_uses_gpu_shader_mask() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 5.0 }),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 180.0,
        height: 32.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        8 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Stroke text"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(
        scene.texts.len(),
        1,
        "stroke should emit a glyph mask text draw"
    );
    assert_eq!(
        scene.effect_layers.len(),
        1,
        "stroke should route through runtime shader effect layer"
    );
    assert_eq!(
        scene.images.len(),
        0,
        "stroke should not render via software image fallback"
    );
    let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
        panic!("expected runtime shader effect for stroke text");
    };
    let uniforms = shader.uniforms();
    let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
    let material_slot = GPU_TEXT_BRUSH_EFFECT_MATERIAL_SLOT * 4;
    assert_eq!(
        uniform(material_slot),
        GPU_TEXT_DRAW_MODE_STROKE,
        "stroke path should set stroke draw mode in shader uniforms"
    );
    assert!(
        (uniform(material_slot + 1) - 5.0).abs() < f32::EPSILON,
        "stroke width should be packed into shader uniforms"
    );
    let expected_padding = stroke_effect_padding_local(5.0);
    assert!(
        (uniform(material_slot + 2) - expected_padding).abs() < f32::EPSILON,
        "stroke effect should pack outline padding into shader uniforms"
    );
    let mask_rect = scene.texts[0].rect;
    assert!(
        (scene.effect_layers[0].rect.x - (mask_rect.x - expected_padding)).abs() < f32::EPSILON
            && (scene.effect_layers[0].rect.y - (mask_rect.y - expected_padding)).abs()
                < f32::EPSILON
            && (scene.effect_layers[0].rect.width - (mask_rect.width + expected_padding * 2.0))
                .abs()
                < f32::EPSILON
            && (scene.effect_layers[0].rect.height - (mask_rect.height + expected_padding * 2.0))
                .abs()
                < f32::EPSILON,
        "stroke effects should expand effect bounds to avoid outline clipping"
    );
}

#[test]
fn push_text_style_draws_stroke_material_scales_width_with_layer_scale() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 3.0 }),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 180.0,
        height: 32.0,
    };
    let scaled_layer = GraphicsLayer {
        scale: 2.0,
        ..Default::default()
    };

    push_text_style_draws_for_test(
        &mut scene,
        18 as NodeId,
        rect,
        rect,
        &scaled_layer,
        &cranpose_ui::text::AnnotatedString::from("Scaled stroke"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(
        scene.effect_layers.len(),
        1,
        "expected one shader effect layer"
    );
    let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
        panic!("expected runtime shader effect for scaled stroke");
    };
    let uniforms = shader.uniforms();
    let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
    let material_slot = GPU_TEXT_BRUSH_EFFECT_MATERIAL_SLOT * 4;
    assert_eq!(uniform(material_slot), GPU_TEXT_DRAW_MODE_STROKE);
    assert!(
        (uniform(material_slot + 1) - 6.0).abs() < f32::EPSILON,
        "stroke width should scale with graphics layer scale"
    );
    let expected_padding = stroke_effect_padding_local(6.0);
    assert!(
        (uniform(material_slot + 2) - expected_padding).abs() < f32::EPSILON,
        "scaled stroke should update shader padding with scaled width"
    );
    let mask_rect = scene.texts[0].rect;
    assert!(
        (scene.effect_layers[0].rect.x - (mask_rect.x - expected_padding)).abs() < f32::EPSILON
            && (scene.effect_layers[0].rect.y - (mask_rect.y - expected_padding)).abs()
                < f32::EPSILON
            && (scene.effect_layers[0].rect.width - (mask_rect.width + expected_padding * 2.0))
                .abs()
                < f32::EPSILON
            && (scene.effect_layers[0].rect.height - (mask_rect.height + expected_padding * 2.0))
                .abs()
                < f32::EPSILON,
        "scaled stroke effects should expand effect bounds by scaled outline padding"
    );
}

#[test]
fn push_text_style_draws_gradient_stroke_contract_uses_gpu_shader_mask() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            brush: Some(Brush::linear_gradient_range(
                vec![Color(0.2, 0.9, 1.0, 1.0), Color(1.0, 0.7, 0.5, 1.0)],
                Point::new(0.0, 0.0),
                Point::new(180.0, 0.0),
            )),
            draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 2.8 }),
            ..Default::default()
        },
        ..Default::default()
    };
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 220.0,
        height: 32.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        12 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Gradient stroke"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(
        scene.texts.len(),
        1,
        "gradient + stroke should emit a glyph mask text draw"
    );
    assert_eq!(
        scene.effect_layers.len(),
        1,
        "gradient + stroke should emit one runtime shader effect layer"
    );
    assert_eq!(
        scene.images.len(),
        0,
        "gradient + stroke should not use software text image path"
    );
    let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
        panic!("expected runtime shader effect for gradient stroke");
    };
    let uniforms = shader.uniforms();
    let uniform = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
    let material_slot = GPU_TEXT_BRUSH_EFFECT_MATERIAL_SLOT * 4;
    assert_eq!(
        uniform(material_slot),
        GPU_TEXT_DRAW_MODE_STROKE,
        "gradient + stroke should keep stroke draw mode"
    );
    let expected_padding = stroke_effect_padding_local(2.8);
    let mask_rect = scene.texts[0].rect;
    assert!(
        (scene.effect_layers[0].rect.x - (mask_rect.x - expected_padding)).abs() < f32::EPSILON
            && (scene.effect_layers[0].rect.y - (mask_rect.y - expected_padding)).abs()
                < f32::EPSILON
            && (scene.effect_layers[0].rect.width - (mask_rect.width + expected_padding * 2.0))
                .abs()
                < f32::EPSILON
            && (scene.effect_layers[0].rect.height - (mask_rect.height + expected_padding * 2.0))
                .abs()
                < f32::EPSILON,
        "gradient stroke should also expand effect bounds to preserve edges"
    );
}

#[test]
fn push_text_style_draws_span_gradient_without_paint_override_uses_gpu_shader_mask() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            brush: Some(Brush::linear_gradient_range(
                vec![Color(0.15, 0.9, 1.0, 1.0), Color(1.0, 0.65, 0.45, 1.0)],
                Point::new(0.0, 0.0),
                Point::new(200.0, 0.0),
            )),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = cranpose_ui::text::AnnotatedString::builder()
        .append("GPU ")
        .push_style(cranpose_ui::text::SpanStyle {
            font_weight: Some(cranpose_ui::text::FontWeight::BOLD),
            ..Default::default()
        })
        .append("Mask")
        .pop()
        .to_annotated_string();
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 220.0,
        height: 32.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        19 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(
        scene.images.len(),
        0,
        "span gradient should not use image path"
    );
    assert_eq!(scene.texts.len(), 1, "expected one mask text draw");
    assert_eq!(
        scene.effect_layers.len(),
        1,
        "span gradient without paint overrides should use gpu effect path"
    );
    let mask_text = &scene.texts[0].text;
    assert!(
        mask_text
            .span_styles
            .iter()
            .all(|span| span.item.color.is_none()
                && span.item.brush.is_none()
                && span.item.alpha.is_none()
                && span.item.draw_style.is_none()),
        "mask text span styles should clear paint fields for uniform material sampling"
    );
    assert!(
        mask_text
            .span_styles
            .iter()
            .any(|span| span.item.font_weight == Some(cranpose_ui::text::FontWeight::BOLD)),
        "mask text should preserve non-paint span styling"
    );
}

#[test]
fn push_text_style_draws_span_gradient_with_paint_override_uses_gpu_shader_mask_batches() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            brush: Some(Brush::linear_gradient_range(
                vec![Color(0.15, 0.9, 1.0, 1.0), Color(1.0, 0.65, 0.45, 1.0)],
                Point::new(0.0, 0.0),
                Point::new(200.0, 0.0),
            )),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = cranpose_ui::text::AnnotatedString::builder()
        .append("GPU ")
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            font_weight: Some(cranpose_ui::text::FontWeight::BOLD),
            ..Default::default()
        })
        .append("Mask")
        .pop()
        .to_annotated_string();
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 220.0,
        height: 32.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        20 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(scene.images.len(), 0, "no software fallback should be used");
    assert!(
        scene.texts.len() >= 2,
        "span paint overrides should split into per-material mask draws"
    );
    assert_eq!(
        scene.texts.len(),
        scene.effect_layers.len(),
        "each mask draw should be wrapped by one runtime shader layer"
    );
    assert!(
        scene
            .effect_layers
            .iter()
            .all(|layer| layer.z_start + 1 == layer.z_end),
        "each per-span material layer should isolate exactly one mask draw"
    );
    assert!(
        scene
            .texts
            .iter()
            .all(|draw| draw.color == Color(1.0, 1.0, 1.0, 0.0)),
        "per-material mask draws should default to transparent non-target glyphs"
    );
    assert!(
        scene
            .texts
            .iter()
            .all(
                |draw| draw.text_style.span_style.color == Some(Color(1.0, 1.0, 1.0, 0.0))
                    && draw.text_style.span_style.brush.is_none()
                    && draw.text_style.span_style.alpha.is_none()
                    && draw.text_style.span_style.draw_style == Some(TextDrawStyle::Fill)
            ),
        "mask text style should force fill-mode alpha masks for each batch"
    );
    assert!(
        scene.texts.iter().all(|draw| draw
            .text
            .span_styles
            .iter()
            .any(|span| span.item.color == Some(Color::WHITE))),
        "each batch should include explicit visible-range white mask spans"
    );
    assert!(
        scene.texts.iter().all(|draw| draw
            .text
            .span_styles
            .iter()
            .any(|span| span.item.font_weight == Some(cranpose_ui::text::FontWeight::BOLD))),
        "mask text should preserve non-paint span styling"
    );
    assert!(
        scene.texts.iter().all(|draw| draw
            .text
            .span_styles
            .iter()
            .all(|span| span.item.color != Some(Color::RED))),
        "original span paint overrides should not leak directly into mask attrs"
    );

    let mut brush_kinds = Vec::new();
    for layer in &scene.effect_layers {
        let Some(RenderEffect::Shader { shader }) = layer.effect.as_ref() else {
            panic!("expected runtime shader effect for span material batch");
        };
        brush_kinds.push(shader.uniforms().first().copied().unwrap_or_default());
    }
    assert_eq!(
        brush_kinds,
        vec![GPU_TEXT_BRUSH_KIND_LINEAR, GPU_TEXT_BRUSH_KIND_SOLID],
        "global gradient and span solid override should map to separate shader batches"
    );
}

#[test]
fn push_text_style_draws_adjacent_span_color_overrides_use_direct_path() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle::default();
    let text = cranpose_ui::text::AnnotatedString::builder()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            ..Default::default()
        })
        .append("GP")
        .pop()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            ..Default::default()
        })
        .append("U!")
        .pop()
        .to_annotated_string();
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 220.0,
        height: 32.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        21 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(scene.images.len(), 0, "no software fallback should be used");
    assert_eq!(scene.texts.len(), 1);
    assert_eq!(
        scene.effect_layers.len(),
        0,
        "solid color spans use software text raster colors, no GPU shader needed"
    );
    assert!(
        scene.texts[0]
            .text
            .span_styles
            .iter()
            .filter(|span| span.item.color == Some(Color::RED))
            .count()
            >= 1,
        "span colors should be preserved for software text raster rendering"
    );
}

#[test]
fn push_text_style_draws_span_color_override_uses_direct_per_glyph_color() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle::default();
    let text = cranpose_ui::text::AnnotatedString::builder()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            ..Default::default()
        })
        .append("Tint")
        .pop()
        .to_annotated_string();
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 220.0,
        height: 32.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        24 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(scene.images.len(), 0, "no software fallback should be used");
    assert_eq!(scene.texts.len(), 1);
    assert_eq!(
        scene.effect_layers.len(),
        0,
        "solid color span overrides use software text raster colors, no GPU shader needed"
    );
    assert!(
        scene.texts[0]
            .text
            .span_styles
            .iter()
            .any(|span| span.item.color == Some(Color::RED)),
        "span color should be preserved in the text for software text raster rendering"
    );
}

#[test]
fn push_text_style_draws_span_alpha_override_modulates_gpu_material_alpha() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(Color::BLUE),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = cranpose_ui::text::AnnotatedString::builder()
        .push_style(cranpose_ui::text::SpanStyle {
            alpha: Some(0.25),
            ..Default::default()
        })
        .append("Fade")
        .pop()
        .to_annotated_string();
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 220.0,
        height: 32.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        25 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(scene.images.len(), 0, "no software fallback should be used");
    assert_eq!(scene.texts.len(), 1);
    assert_eq!(scene.effect_layers.len(), 1);
    let Some(RenderEffect::Shader { shader }) = scene.effect_layers[0].effect.as_ref() else {
        panic!("expected runtime shader effect for span alpha override");
    };
    let uniforms = shader.uniforms();
    let uniform = |index: usize| uniforms.get(index).copied().unwrap_or_default();
    let color_slot = GPU_TEXT_BRUSH_EFFECT_FIRST_STOP_SLOT * 4;
    assert_eq!(uniform(0), GPU_TEXT_BRUSH_KIND_SOLID);
    assert!((uniform(color_slot) - Color::BLUE.r()).abs() < f32::EPSILON);
    assert!((uniform(color_slot + 1) - Color::BLUE.g()).abs() < f32::EPSILON);
    assert!((uniform(color_slot + 2) - Color::BLUE.b()).abs() < f32::EPSILON);
    assert!((uniform(color_slot + 3) - Color::BLUE.a()).abs() < f32::EPSILON);
    assert!((uniform(3) - 0.25).abs() < f32::EPSILON);
}

#[test]
fn push_text_style_draws_wrap_newline_gap_color_spans_use_direct_path() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle::default();
    let text = cranpose_ui::text::AnnotatedString::builder()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            ..Default::default()
        })
        .append("ABC")
        .pop()
        .append("\n")
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            ..Default::default()
        })
        .append("DEF")
        .pop()
        .to_annotated_string();
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 220.0,
        height: 56.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        26 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &text,
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(scene.images.len(), 0, "no software fallback should be used");
    assert_eq!(scene.texts.len(), 1);
    assert_eq!(
        scene.effect_layers.len(),
        0,
        "solid color spans use software text raster colors, no GPU shader needed"
    );
    let red_ranges: Vec<_> = scene.texts[0]
        .text
        .span_styles
        .iter()
        .filter(|span| span.item.color == Some(Color::RED))
        .map(|span| span.range.clone())
        .collect();
    assert_eq!(
        red_ranges.len(),
        2,
        "span colors should be preserved for both wrapped lines"
    );
}

#[test]
fn push_text_style_draws_mixed_bidi_wrapped_color_spans_use_direct_path() {
    let mut scene = Scene::new();
    let style = cranpose_ui::TextStyle::default();
    let source_text = cranpose_ui::text::AnnotatedString::builder()
        .push_style(cranpose_ui::text::SpanStyle {
            color: Some(Color::RED),
            ..Default::default()
        })
        .append("abc אבג def דהו ghi jkl mno")
        .pop()
        .to_annotated_string();
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: true,
        max_lines: usize::MAX,
        min_lines: 1,
    };
    let prepared = prepare_text_layout_for_test(&source_text, &style, options, Some(64.0));
    assert!(
        prepared.text.text.contains('\n'),
        "test setup should produce wrapped multiline text: {:?}",
        prepared.text
    );
    let rect = Rect {
        x: 8.0,
        y: 20.0,
        width: 240.0,
        height: 96.0,
    };

    push_text_style_draws_for_test(
        &mut scene,
        27 as NodeId,
        rect,
        rect,
        &GraphicsLayer::default(),
        &prepared.text,
        &style,
        14.0,
        options,
        None,
    );

    assert_eq!(scene.images.len(), 0, "no software fallback should be used");
    assert_eq!(scene.texts.len(), 1);
    assert_eq!(
        scene.effect_layers.len(),
        0,
        "solid color spans use software text raster colors, no GPU shader needed"
    );
    assert!(
        scene.texts[0]
            .text
            .span_styles
            .iter()
            .any(|span| span.item.color == Some(Color::RED)),
        "span color should be preserved for software text raster rendering"
    );
}

#[test]
fn push_text_style_draws_keeps_static_and_animated_text_geometry_in_scene_space() {
    let base_rect = Rect {
        x: 8.25,
        y: 10.75,
        width: 180.0,
        height: 28.0,
    };
    let static_style = cranpose_ui::TextStyle::from_paragraph_style(cranpose_ui::ParagraphStyle {
        text_motion: Some(cranpose_ui::text::TextMotion::Static),
        ..Default::default()
    });
    let animated_style =
        cranpose_ui::TextStyle::from_paragraph_style(cranpose_ui::ParagraphStyle {
            text_motion: Some(cranpose_ui::text::TextMotion::Animated),
            ..Default::default()
        });

    let mut static_scene = Scene::new();
    push_text_style_draws_for_test(
        &mut static_scene,
        9 as NodeId,
        base_rect,
        base_rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Motion"),
        &static_style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    let mut animated_scene = Scene::new();
    push_text_style_draws_for_test(
        &mut animated_scene,
        10 as NodeId,
        base_rect,
        base_rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Motion"),
        &animated_style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(static_scene.texts.len(), 1);
    assert_eq!(animated_scene.texts.len(), 1);
    assert!((static_scene.texts[0].rect.x - base_rect.x).abs() < f32::EPSILON);
    assert!((static_scene.texts[0].rect.y - base_rect.y).abs() < f32::EPSILON);
    assert!((animated_scene.texts[0].rect.x - base_rect.x).abs() < f32::EPSILON);
    assert!((animated_scene.texts[0].rect.y - base_rect.y).abs() < f32::EPSILON);
}

#[test]
fn push_text_style_draws_stroke_keeps_mask_and_effect_bounds_in_scene_space() {
    let base_rect = Rect {
        x: 8.25,
        y: 10.75,
        width: 180.0,
        height: 28.0,
    };
    let static_style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            draw_style: Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 4.0 }),
            ..Default::default()
        },
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_motion: Some(cranpose_ui::text::TextMotion::Static),
            ..Default::default()
        },
    };
    let animated_style = cranpose_ui::TextStyle {
        span_style: static_style.span_style.clone(),
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_motion: Some(cranpose_ui::text::TextMotion::Animated),
            ..Default::default()
        },
    };

    let mut static_scene = Scene::new();
    push_text_style_draws_for_test(
        &mut static_scene,
        22 as NodeId,
        base_rect,
        base_rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Stroke Motion"),
        &static_style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    let mut animated_scene = Scene::new();
    push_text_style_draws_for_test(
        &mut animated_scene,
        23 as NodeId,
        base_rect,
        base_rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Stroke Motion"),
        &animated_style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert_eq!(static_scene.texts.len(), 1);
    assert_eq!(static_scene.effect_layers.len(), 1);
    assert_eq!(animated_scene.texts.len(), 1);
    assert_eq!(animated_scene.effect_layers.len(), 1);

    let expected_padding = stroke_effect_padding_local(4.0);
    assert!((static_scene.texts[0].rect.x - base_rect.x).abs() < f32::EPSILON);
    assert!((static_scene.texts[0].rect.y - base_rect.y).abs() < f32::EPSILON);
    let static_mask_rect = static_scene.texts[0].rect;
    assert!(
        (static_scene.effect_layers[0].rect.x - (static_mask_rect.x - expected_padding)).abs()
            < f32::EPSILON
            && (static_scene.effect_layers[0].rect.y - (static_mask_rect.y - expected_padding))
                .abs()
                < f32::EPSILON
    );

    assert!((animated_scene.texts[0].rect.x - base_rect.x).abs() < f32::EPSILON);
    assert!((animated_scene.texts[0].rect.y - base_rect.y).abs() < f32::EPSILON);
    let animated_mask_rect = animated_scene.texts[0].rect;
    assert!(
        (animated_scene.effect_layers[0].rect.x - (animated_mask_rect.x - expected_padding)).abs()
            < f32::EPSILON
            && (animated_scene.effect_layers[0].rect.y - (animated_mask_rect.y - expected_padding))
                .abs()
                < f32::EPSILON
    );
}

#[test]
fn push_text_style_draws_gradient_keeps_mask_and_effect_bounds_in_scene_space() {
    let mut scene = Scene::new();
    let base_rect = Rect {
        x: 8.25,
        y: 10.75,
        width: 180.2,
        height: 28.4,
    };
    let style = cranpose_ui::TextStyle {
        span_style: cranpose_ui::SpanStyle {
            brush: Some(Brush::linear_gradient_range(
                vec![Color(0.2, 0.9, 1.0, 1.0), Color(1.0, 0.7, 0.5, 1.0)],
                Point::new(0.0, 0.0),
                Point::new(180.0, 0.0),
            )),
            ..Default::default()
        },
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_motion: Some(cranpose_ui::text::TextMotion::Static),
            ..Default::default()
        },
    };

    push_text_style_draws_for_test(
        &mut scene,
        11 as NodeId,
        base_rect,
        base_rect,
        &GraphicsLayer::default(),
        &cranpose_ui::text::AnnotatedString::from("Gradient static"),
        &style,
        14.0,
        TextLayoutOptions::default(),
        None,
    );

    assert!(
        scene.images.is_empty(),
        "gradient text should not use rasterized image fallback"
    );
    assert_eq!(
        scene.texts.len(),
        1,
        "gradient should emit a raster mask text draw"
    );
    assert_eq!(
        scene.effect_layers.len(),
        1,
        "gradient should emit one runtime shader effect layer"
    );

    let text_draw = &scene.texts[0];
    assert!((text_draw.rect.x - base_rect.x).abs() < f32::EPSILON);
    assert!((text_draw.rect.y - base_rect.y).abs() < f32::EPSILON);

    let effect_layer = &scene.effect_layers[0];
    assert!((effect_layer.rect.x - base_rect.x).abs() < f32::EPSILON);
    assert!((effect_layer.rect.y - base_rect.y).abs() < f32::EPSILON);
    assert!((effect_layer.rect.width - base_rect.width).abs() < 1e-3);
    assert!((effect_layer.rect.height - base_rect.height).abs() < 1e-3);
    let Some(RenderEffect::Shader { .. }) = effect_layer.effect.as_ref() else {
        panic!("gradient text should use runtime shader effect");
    };
}

#[test]
fn single_line_overflow_ellipsizes_at_the_measurement_width() {
    let padding = EdgeInsets {
        left: 4.0,
        top: 0.0,
        right: 4.0,
        bottom: 0.0,
    };
    let text = "Overflow sample: Supercalifragilisticexpialidocious";
    let style = cranpose_ui::TextStyle::default();
    let options = TextLayoutOptions {
        overflow: TextOverflow::Ellipsis,
        soft_wrap: false,
        max_lines: 1,
        min_lines: 1,
    };
    let content_width = 130.0;
    let measure_width = resolve_text_measure_width(content_width, padding, Some(180.0));
    let prepared = prepare_text_layout_for_test(
        &cranpose_ui::text::AnnotatedString::from(text),
        &style,
        options,
        Some(measure_width),
    );
    assert!(
        prepared.text.text.contains('\u{2026}'),
        "ellipsis should remain active: {:?}",
        prepared.text
    );
}

fn text_draw_primitive(rect: Rect, text: &str) -> DrawPrimitive {
    DrawPrimitive::Text(Box::new(cranpose_ui_graphics::TextPrimitive {
        rect,
        text: std::rc::Rc::from(text),
        style: cranpose_ui_graphics::DrawTextStyle::new(16.0),
        color: cranpose_ui_graphics::Color::WHITE,
    }))
}

#[test]
fn a_text_draw_primitive_joins_the_scene_text_list_the_text_nodes_use() {
    let mut scene = Scene::new();
    push_draw_primitive(
        &text_draw_primitive(
            Rect {
                x: 4.0,
                y: 5.0,
                width: 60.0,
                height: 20.0,
            },
            "SCORE",
        ),
        Rect {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 100.0,
        },
        &GraphicsLayer::default(),
        None,
        None,
        &mut scene,
        None,
        false,
    );

    assert_eq!(scene.texts.len(), 1, "text must reach the glyph-atlas path");
    assert!(
        shape_records(&mut scene).is_empty() && scene.images.is_empty(),
        "text must not be lowered into a shape or a rasterized image"
    );
    let text = &scene.texts[0];
    assert_eq!(text.text.text, "SCORE");
    assert_eq!(text.rect.x, 14.0);
    assert_eq!(text.rect.y, 25.0);
    assert_eq!(text.color, cranpose_ui_graphics::Color::WHITE);
    assert!(
        scene
            .draw_ops
            .iter()
            .any(|op| matches!(op.kind, crate::scene::DrawOpKind::Text(0))),
        "the run must be ordered with the rest of the layer's draws"
    );
}

#[test]
fn text_draw_primitives_keep_their_z_order_against_the_shapes_around_them() {
    let mut scene = Scene::new();
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    let backdrop = DrawPrimitive::Rect {
        rect: bounds,
        brush: Brush::solid(cranpose_ui_graphics::Color::BLACK),
        stroke: None,
    };
    for primitive in [
        backdrop,
        text_draw_primitive(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 20.0,
            },
            "HUD",
        ),
    ] {
        push_draw_primitive(
            &primitive,
            bounds,
            &GraphicsLayer::default(),
            None,
            None,
            &mut scene,
            None,
            false,
        );
    }

    assert_eq!(shape_records(&mut scene).len(), 1);
    assert_eq!(scene.texts.len(), 1);
    let z_of = |kind: fn(&crate::scene::DrawOpKind) -> bool| {
        scene
            .draw_ops
            .iter()
            .find(|op| kind(&op.kind))
            .map(|op| op.z_index)
            .expect("op present")
    };
    assert!(
        z_of(|kind| matches!(kind, crate::scene::DrawOpKind::Text(_)))
            > z_of(|kind| matches!(kind, crate::scene::DrawOpKind::Run(_))),
        "text drawn after a rect must composite above it"
    );
}
