use cranpose_render_common::{
    graph::{
        CachePolicy, DrawPrimitiveNode, IsolationReasons, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    },
    raster_cache::LayerRasterCacheHashes,
};
use cranpose_ui_graphics::{CornerRadii, ImageBitmap, ImageSampling};

use super::*;
use crate::scene::RasterScene;

fn with_test_app_context<R>(block: impl FnOnce() -> R) -> R {
    let app_context = cranpose_ui::AppContext::new();
    app_context.enter(block)
}

fn build_raster_scene_for_test(graph: &RenderGraph) -> RasterScene {
    let diagnostics = RenderDiagnostics::new();
    with_test_app_context(|| build_raster_scene(graph, &diagnostics))
}

fn push_text_style_draws_for_test(
    scene: &mut RasterScene,
    rect: Rect,
    text: &str,
    text_style: &TextStyle,
    clip: Option<Rect>,
) {
    let text = cranpose_ui::text::AnnotatedString::from(text);
    with_test_app_context(|| {
        push_text_style_draws(
            scene,
            7 as NodeId,
            rect,
            rect,
            &GraphicsLayer::default(),
            &text,
            text_style,
            14.0,
            TextLayoutOptions::default(),
            clip,
        );
    });
}

fn snapped_text_leaf_root(animated: bool, translated_content_context: bool) -> RenderGraph {
    let text_leaf = LayerNode {
        node_id: Some(77),
        wraps: None,
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 24.0,
        },
        transform_to_parent: ProjectiveTransform::translation(14.25, 16.5),
        motion_context_animated: animated,
        translated_content_context,
        translated_content_offset: Point::default(),
        content_offset: Point::default(),
        scene_children_origin: cranpose_ui_graphics::Point::default(),
        scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
        graphics_layer: GraphicsLayer::default(),
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        has_hit_targets: false,
        has_origin_sinks: false,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children: vec![
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::RoundRect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 48.0,
                            height: 24.0,
                        },
                        brush: Brush::solid(Color(0.28, 0.30, 0.46, 0.88)),
                        radii: CornerRadii::uniform(6.0),
                        stroke: None,
                    },
                    clip: None,
                }),
            }),
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Image {
                        rect: Rect {
                            x: 2.0,
                            y: 2.0,
                            width: 12.0,
                            height: 12.0,
                        },
                        image: ImageBitmap::from_rgba8(
                            2,
                            2,
                            vec![
                                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                            ],
                        )
                        .expect("image"),
                        alpha: 1.0,
                        color_filter: None,
                        sampling: ImageSampling::Linear,
                        src_rect: None,
                    },
                    clip: None,
                }),
            }),
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id: 77,
                    rect: Rect {
                        x: 6.0,
                        y: 4.0,
                        width: 36.0,
                        height: 16.0,
                    },
                    text: std::rc::Rc::new(cranpose_ui::text::AnnotatedString::from("48 px")),
                    text_style: TextStyle::default(),
                    font_size: 14.0,
                    layout_options: TextLayoutOptions::default(),
                    clip: None,
                })),
            }),
        ],
    };

    RenderGraph::new(LayerNode {
        node_id: None,
        wraps: None,
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 96.0,
            height: 64.0,
        },
        transform_to_parent: ProjectiveTransform::identity(),
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        content_offset: Point::default(),
        scene_children_origin: cranpose_ui_graphics::Point::default(),
        scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
        graphics_layer: GraphicsLayer::default(),
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        has_hit_targets: false,
        has_origin_sinks: false,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children: vec![RenderNode::Layer(Box::new(text_leaf))],
    })
}

#[test]
fn render_effect_support_matrix_is_explicit() {
    let blur = RenderEffect::blur(4.0);
    let offset = RenderEffect::offset(2.0, 3.0);
    let chain = blur.clone().then(offset.clone());

    assert!(!is_render_effect_supported(&blur));
    assert!(!is_render_effect_supported(&offset));
    assert!(!is_render_effect_supported(&chain));
}

#[test]
fn fallback_detection_triggers_for_effects_and_offscreen() {
    let mut layer = GraphicsLayer::default();
    assert!(!layer_requires_effect_fallback(&layer));

    layer.render_effect = Some(RenderEffect::blur(4.0));
    assert!(layer_requires_effect_fallback(&layer));

    layer.render_effect = None;
    layer.backdrop_effect = Some(RenderEffect::offset(1.0, 2.0));
    assert!(layer_requires_effect_fallback(&layer));

    layer.backdrop_effect = None;
    layer.compositing_strategy = CompositingStrategy::Offscreen;
    assert!(layer_requires_effect_fallback(&layer));
}

#[test]
fn shadow_geometry_has_visible_expansion_and_offsets() {
    let mut scene = RasterScene::new();
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
    let geometry = layer_shadow_geometry(&layer, bounds);
    let ambient_pass = geometry.ambient.expect("ambient pass");
    let spot_pass = geometry.spot.expect("spot pass");
    let ambient_samples = blur_samples(ambient_pass.blur_radius.max(1.0));
    let spot_samples = blur_samples(spot_pass.blur_radius.max(1.0));

    push_layer_shadow(
        &mut scene,
        &layer,
        RasterLayerBounds::from_transformed_bounds(bounds, bounds),
        bounds,
        None,
    );

    assert!(
        scene.shapes.len() == ambient_samples.len() + spot_samples.len(),
        "pixels shadow blur should emit one sample per shared ambient/spot pass"
    );

    let ambient = &scene.shapes[0];
    let ambient_expansion = ambient_samples
        .last()
        .expect("ambient blur samples")
        .expansion;
    assert_eq!(
        ambient.rect,
        Rect {
            x: ambient_pass.rect.x - ambient_expansion,
            y: ambient_pass.rect.y - ambient_expansion,
            width: ambient_pass.rect.width + ambient_expansion * 2.0,
            height: ambient_pass.rect.height + ambient_expansion * 2.0,
        }
    );

    let spot = &scene.shapes[ambient_samples.len()];
    let spot_expansion = spot_samples.last().expect("spot blur samples").expansion;
    assert_eq!(
        spot.rect,
        Rect {
            x: spot_pass.rect.x - spot_expansion,
            y: spot_pass.rect.y - spot_expansion,
            width: spot_pass.rect.width + spot_expansion * 2.0,
            height: spot_pass.rect.height + spot_expansion * 2.0,
        }
    );
    let spot_peak_alpha = scene.shapes[ambient_samples.len()..]
        .iter()
        .filter_map(|shape| match &shape.brush {
            Brush::Solid(color) => Some(color.a()),
            _ => None,
        })
        .fold(0.0f32, f32::max);
    assert!(spot_peak_alpha > 0.02, "spot alpha should remain visible");
}

#[test]
fn build_raster_scene_uses_graph_transform_to_parent() {
    let graph = RenderGraph::new(LayerNode {
        node_id: None,
        wraps: None,
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 48.0,
        },
        transform_to_parent: ProjectiveTransform::identity(),
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        content_offset: Point::default(),
        scene_children_origin: cranpose_ui_graphics::Point::default(),
        scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
        graphics_layer: GraphicsLayer::default(),
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        has_hit_targets: false,
        has_origin_sinks: false,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        cache_hashes: LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children: vec![RenderNode::Layer(Box::new(LayerNode {
            node_id: None,
            wraps: None,
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 24.0,
                height: 18.0,
            },
            transform_to_parent: ProjectiveTransform::translation(17.0, 11.0),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            content_offset: Point::default(),
            scene_children_origin: cranpose_ui_graphics::Point::default(),
            scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            has_origin_sinks: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Rect {
                        rect: Rect {
                            x: 4.0,
                            y: 3.0,
                            width: 8.0,
                            height: 6.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                        stroke: None,
                    },
                    clip: None,
                }),
            })],
        }))],
    });

    let scene = build_raster_scene_for_test(&graph);
    let [shape] = scene.shapes.as_slice() else {
        panic!("expected exactly one translated child shape");
    };
    assert_eq!(
        shape.rect,
        Rect {
            x: 21.0,
            y: 14.0,
            width: 8.0,
            height: 6.0,
        }
    );
}

#[test]
fn direct_text_leaf_snaps_modifier_background_and_text_with_one_anchor() {
    let scene = build_raster_scene_for_test(&snapped_text_leaf_root(false, false));

    assert_eq!(scene.shapes.len(), 1);
    assert_eq!(scene.images.len(), 1);
    assert_eq!(scene.texts.len(), 1);
    let expected_anchor = Some(Point::new(14.25, 16.5));
    assert_eq!(scene.shapes[0].snap_anchor, expected_anchor);
    assert_eq!(scene.images[0].snap_anchor, expected_anchor);
    assert_eq!(scene.texts[0].snap_anchor, expected_anchor);
}

#[test]
fn animated_translated_content_text_leaf_uses_content_snap() {
    let scene = build_raster_scene_for_test(&snapped_text_leaf_root(true, true));

    assert_eq!(scene.shapes.len(), 1);
    assert_eq!(scene.images.len(), 1);
    assert_eq!(scene.texts.len(), 1);
    let expected_anchor = Some(Point::new(14.25, 16.5));
    assert_eq!(
        scene.shapes[0].snap_anchor, expected_anchor,
        "active scroll shapes should render with the same content-origin snap phase as settled content"
    );
    assert_eq!(
        scene.images[0].snap_anchor, expected_anchor,
        "active scroll images should render with the same content-origin snap phase as settled content"
    );
    assert_eq!(
        scene.texts[0].snap_anchor, expected_anchor,
        "active scroll text should render with the same content-origin snap phase as settled content"
    );
}

#[test]
fn rested_translated_content_text_leaf_snaps_for_crisp_scroll_rest() {
    let scene = build_raster_scene_for_test(&snapped_text_leaf_root(false, true));

    assert_eq!(scene.shapes.len(), 1);
    assert_eq!(scene.images.len(), 1);
    assert_eq!(scene.texts.len(), 1);
    let expected_anchor = Some(Point::new(14.25, 16.5));
    assert_eq!(
        scene.shapes[0].snap_anchor, expected_anchor,
        "rested scroll content should snap back to device pixels"
    );
    assert_eq!(
        scene.images[0].snap_anchor, expected_anchor,
        "rested scroll images should snap back to device pixels"
    );
    assert_eq!(
        scene.texts[0].snap_anchor, expected_anchor,
        "rested scroll text should snap back to device pixels"
    );
}

fn clipped_layer(placement: Point, children: Vec<RenderNode>) -> LayerNode {
    LayerNode {
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 120.0,
        },
        transform_to_parent: ProjectiveTransform::translation(placement.x, placement.y),
        clip_to_bounds: true,
        children,
        ..Default::default()
    }
}

fn red_fill() -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 120.0,
                    height: 120.0,
                },
                brush: Brush::solid(Color(0.86, 0.12, 0.12, 1.0)),
                stroke: None,
            },
            clip: None,
        }),
    })
}

fn shapes_in_list(child: LayerNode) -> usize {
    let mut list = clipped_layer(Point::new(0.0, 80.0), vec![]);
    list.local_bounds.width = 200.0;
    list.children.push(RenderNode::Layer(Box::new(child)));
    build_raster_scene_for_test(&RenderGraph::new(list))
        .shapes
        .len()
}

#[test]
fn a_layer_clipped_away_by_its_parent_paints_nothing() {
    let shown = shapes_in_list(clipped_layer(Point::new(40.0, 10.0), vec![red_fill()]));
    assert_eq!(shown, 1, "the fill must paint while its layer is on show");

    let scrolled_out = shapes_in_list(clipped_layer(Point::new(40.0, -140.0), vec![red_fill()]));
    assert_eq!(
        scrolled_out, 0,
        "a layer the list has scrolled past its edge lies outside the list's clip and \
         paints nothing, however far inside the window it sits"
    );
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
fn push_text_style_draws_emits_background_shadow_and_main_text() {
    let mut scene = RasterScene::new();
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
        rect,
        "Decorated shadow text",
        &style,
        Some(clip),
    );

    assert_eq!(
        scene.shapes.len(),
        1,
        "span background should emit one shape"
    );
    let Brush::Solid(background) = &scene.shapes[0].brush else {
        panic!("background draw should use a solid brush");
    };
    assert_eq!(*background, Color(0.2, 0.3, 0.52, 0.55).srgb_8bit());

    assert_eq!(scene.texts.len(), 2, "shadow + content text expected");
    assert_eq!(scene.texts[0].color, Color::TRANSPARENT);
    let shadow_style = scene.texts[0]
        .text_style
        .span_style
        .shadow
        .expect("shadow draw should carry style shadow");
    assert_eq!(shadow_style.color, Color(0.0, 0.0, 0.0, 0.95).srgb_8bit());
    assert_eq!(shadow_style.offset, Point::new(0.0, 0.0));
    assert!((shadow_style.blur_radius - 3.0).abs() < f32::EPSILON);
    assert_eq!(scene.texts[1].color, Color(0.9, 0.95, 1.0, 1.0).srgb_8bit());
    assert!(scene.texts[0].rect.x > scene.texts[1].rect.x);
    assert!(scene.texts[0].rect.y > scene.texts[1].rect.y);
}

#[test]
fn push_text_style_draws_emits_decoration_shapes() {
    let mut scene = RasterScene::new();
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

    push_text_style_draws_for_test(&mut scene, rect, "Decorated", &style, None);

    assert_eq!(scene.shapes.len(), 2, "underline + line-through expected");
    assert_eq!(scene.texts.len(), 1, "main text expected");
    assert!(
        scene.shapes.iter().all(|shape| shape.snap_to_pixel_grid),
        "text decoration shapes should snap after inherited translated-content anchors"
    );
    assert!(
        scene
            .shapes
            .iter()
            .all(|shape| shape.z_index > scene.texts[0].z_index),
        "text decoration shapes should render over foreground glyph ink"
    );
}

#[test]
fn push_text_style_draws_applies_baseline_shift() {
    let mut scene = RasterScene::new();
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

    push_text_style_draws_for_test(&mut scene, rect, "Shifted", &style, None);

    assert_eq!(scene.texts.len(), 1);
    assert!(
        scene.texts[0].rect.y < rect.y,
        "superscript baseline shift should move text up"
    );
}

#[test]
fn push_text_style_draws_non_solid_brush_contract_does_not_fallback_to_first_stop() {
    let mut scene = RasterScene::new();
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

    push_text_style_draws_for_test(&mut scene, rect, "Gradient text", &style, None);

    assert_eq!(scene.texts.len(), 1);
    assert_ne!(
        scene.texts[0].color, first_stop,
        "non-solid brush text should not degrade to first-stop fallback color"
    );
}

#[test]
fn drop_shadow_primitive_blur_emits_multiple_samples() {
    let mut scene = RasterScene::new();
    let layer_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 24.0,
        height: 16.0,
    };

    push_shadow_primitive(
        &cranpose_ui_graphics::ShadowPrimitive::Drop {
            shape: Box::new(DrawPrimitive::Rect {
                rect: Rect {
                    x: 2.0,
                    y: 3.0,
                    width: 12.0,
                    height: 8.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            }),
            cutout: None,
            blur_radius: 6.0,
            blend_mode: BlendMode::SrcOver,
        },
        layer_bounds,
        &GraphicsLayer::default(),
        None,
        &mut scene,
    );

    assert!(
        scene.shapes.len() > 1,
        "blurred drop shadow should emit multiple weighted samples"
    );
}

#[test]
fn inner_shadow_primitive_blur_emits_multiple_samples() {
    let mut scene = RasterScene::new();
    let layer_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 24.0,
        height: 16.0,
    };

    push_shadow_primitive(
        &cranpose_ui_graphics::ShadowPrimitive::Inner {
            fill: Box::new(DrawPrimitive::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 24.0,
                    height: 16.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            }),
            cutout: Box::new(DrawPrimitive::Rect {
                rect: Rect {
                    x: 3.0,
                    y: 4.0,
                    width: 12.0,
                    height: 6.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            }),
            blur_radius: 5.0,
            blend_mode: BlendMode::SrcOver,
            clip_rect: layer_bounds,
        },
        layer_bounds,
        &GraphicsLayer::default(),
        None,
        &mut scene,
    );

    assert!(
        scene.shapes.len() > 2,
        "blurred inner shadow should emit repeated fill/cutout samples"
    );
}
