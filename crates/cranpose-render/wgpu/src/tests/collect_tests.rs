use cranpose_ui_graphics::RoundedCornerShape;

use super::*;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn raster_scale(
    motion: &mut LayerMotion,
    node_id: Option<NodeId>,
    scale: f32,
    content_hash: u64,
    cacheable: bool,
) -> f32 {
    let raster = if cacheable {
        motion.retained_scale(node_id, scale, content_hash)
    } else {
        scale
    };
    motion.record_scale(node_id, raster, content_hash);
    raster
}

fn corners(width: f32, height: f32, radius: f32) -> RoundedClipCorners {
    RoundedClipCorners::of(
        LayerRoundedClip {
            rect: rect(0.0, 0.0, width, height),
            radii: [radius; 4],
        },
        RasterScale::Exact(1.0),
    )
}

fn drawn_node(primitive: DrawPrimitive) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(Box::new(cranpose_render_common::graph::DrawPrimitiveNode {
            primitive,
            clip: None,
        })),
    })
}

fn snap_test_text() -> DrawPrimitive {
    DrawPrimitive::Text(Box::new(cranpose_ui_graphics::TextPrimitive {
        rect: rect(0.0, 0.0, 60.0, 20.0),
        text: std::rc::Rc::from("pixel"),
        style: cranpose_ui_graphics::DrawTextStyle::new(16.0),
        color: cranpose_ui_graphics::Color::WHITE,
    }))
}

fn snap_test_rect() -> DrawPrimitive {
    DrawPrimitive::Rect {
        rect: rect(0.0, 0.0, 60.0, 20.0),
        brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
        stroke: None,
    }
}

fn phase_draw(x: f32, phase: PrimitivePhase, recorded: bool) -> RenderNode {
    let primitive = solid(rect(x, 0.0, 1.0, 1.0));
    if recorded {
        RenderNode::DrawRun(DrawRunNode::new(phase, vec![primitive]))
    } else {
        RenderNode::Primitive(PrimitiveEntry {
            phase,
            node: PrimitiveNode::Draw(Box::new(cranpose_render_common::graph::DrawPrimitiveNode {
                primitive,
                clip: None,
            })),
        })
    }
}

#[test]
fn deferred_draws_keep_their_order_after_interleaved_children() {
    let before = PrimitivePhase::BeforeChildren;
    let after = PrimitivePhase::AfterChildren;
    let layer = |children| LayerNode {
        local_bounds: rect(0.0, 0.0, 20.0, 20.0),
        children,
        ..Default::default()
    };
    let cases = [
        (Vec::new(), Vec::new()),
        (vec![phase_draw(1.0, before, true)], vec![1.0]),
        (vec![phase_draw(1.0, after, false)], vec![1.0]),
        (
            vec![
                phase_draw(1.0, before, true),
                phase_draw(2.0, after, false),
                RenderNode::Layer(Box::new(layer(vec![phase_draw(3.0, before, true)]))),
                phase_draw(4.0, after, true),
                phase_draw(5.0, before, true),
                phase_draw(6.0, after, false),
            ],
            vec![1.0, 3.0, 5.0, 2.0, 4.0, 6.0],
        ),
        (
            vec![
                phase_draw(1.0, after, true),
                RenderNode::Layer(Box::new(layer(vec![phase_draw(2.0, after, true)]))),
                phase_draw(3.0, before, true),
            ],
            vec![2.0, 3.0, 1.0],
        ),
    ];
    for (children, expected) in cases {
        let collected = collect_root(
            &layer(children),
            &mut crate::pipeline::UiTextLayoutResolver,
            &mut LayerMotion::default(),
            SceneCapacityHint::default(),
            1.0,
            &mut LayerSceneRecycler::default(),
        );
        let positions: Vec<_> = collected
            .scene
            .runs
            .iter()
            .map(|run| run.bounds.x)
            .collect();
        assert_eq!(positions, expected);
    }
}

#[test]
fn rigid_snap_distinguishes_own_draws_from_descendant_text() {
    let cases = [
        (Vec::new(), false, false),
        (vec![drawn_node(DrawPrimitive::Content)], false, false),
        (vec![drawn_node(snap_test_rect())], false, true),
        (vec![drawn_node(snap_test_text())], true, true),
        (
            vec![drawn_node(DrawPrimitive::Blend {
                primitive: Box::new(snap_test_text()),
                blend_mode: BlendMode::Multiply,
            })],
            true,
            true,
        ),
        (
            vec![RenderNode::DrawRun(DrawRunNode::new(
                PrimitivePhase::BeforeChildren,
                vec![snap_test_rect(), snap_test_text()],
            ))],
            true,
            true,
        ),
        (
            vec![RenderNode::Layer(Box::new(LayerNode {
                children: vec![drawn_node(snap_test_text())],
                ..Default::default()
            }))],
            false,
            false,
        ),
        (
            vec![
                drawn_node(DrawPrimitive::Content),
                drawn_node(snap_test_text()),
                drawn_node(DrawPrimitive::Content),
            ],
            true,
            true,
        ),
    ];
    for (children, stationary, translated) in cases {
        let layer = LayerNode {
            children,
            ..Default::default()
        };
        assert_eq!(layer_needs_rigid_snap(&layer, false), stationary);
        assert_eq!(layer_needs_rigid_snap(&layer, true), translated);
    }
}

#[test]
fn isolated_layers_snap_their_own_text_and_translating_text_descendants() {
    let context = cranpose_ui::AppContext::new();
    context.enter(|| {
        for (node, translated, expected) in [
            (drawn_node(snap_test_text()), false, true),
            (
                RenderNode::Layer(Box::new(LayerNode {
                    local_bounds: rect(0.0, 0.0, 60.0, 20.0),
                    transform_to_parent: ProjectiveTransform::translation(2.5, 3.5),
                    children: vec![drawn_node(snap_test_text())],
                    ..Default::default()
                })),
                false,
                true,
            ),
            (drawn_node(snap_test_rect()), false, false),
            (drawn_node(snap_test_rect()), true, true),
        ] {
            let layer = LayerNode {
                local_bounds: rect(0.0, 0.0, 100.0, 60.0),
                children: vec![node],
                ..Default::default()
            };
            let (child, _) = isolated_child(
                &layer,
                &mut crate::pipeline::UiTextLayoutResolver,
                &mut LayerMotion::default(),
                WalkContext {
                    offset: Point::new(0.3, 0.7),
                    visual_clip: None,
                    clip_radius: 0.0,
                    snap_anchor: None,
                    translated,
                    raster_scale: RasterScale::Exact(1.0),
                    light: ShadowLight::for_window(100.0, 100.0, 1.0, 1.0),
                    wants_pixel_sensitive: false,
                },
                &mut CompositorScene::new(),
                &mut LayerSceneRecycler::default(),
            );
            assert_eq!(child.snap_anchor.is_some(), expected);
        }
    });
}

#[test]
fn content_clear_of_every_corner_square_is_admitted() {
    assert!(corners(200.0, 100.0, 20.0).admits(rect(20.0, 20.0, 160.0, 60.0)));
}

#[test]
fn content_inside_the_corner_circle_is_admitted() {
    assert!(corners(200.0, 100.0, 20.0).admits(rect(14.0, 14.0, 60.0, 60.0)));
}

#[test]
fn content_reaching_the_corner_cut_is_refused() {
    assert!(!corners(200.0, 100.0, 20.0).admits(rect(2.0, 2.0, 60.0, 60.0)));
}

#[test]
fn content_touching_the_edge_between_corners_is_admitted() {
    assert!(corners(200.0, 100.0, 20.0).admits(rect(40.0, 0.0, 100.0, 100.0)));
}

fn rounded_layer(radius: f32, content: RenderNode) -> LayerNode {
    LayerNode {
        local_bounds: rect(0.0, 0.0, 200.0, 100.0),
        graphics_layer: GraphicsLayer {
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(radius)),
            ..Default::default()
        }
        .into(),
        children: vec![content],
        ..Default::default()
    }
}

fn white_rect(bounds: Rect) -> DrawPrimitive {
    DrawPrimitive::Rect {
        rect: bounds,
        brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
        stroke: None,
    }
}

fn shapes_run(primitives: Vec<DrawPrimitive>) -> RenderNode {
    RenderNode::DrawRun(DrawRunNode::new(PrimitivePhase::BeforeChildren, primitives))
}

#[test]
fn a_rounded_layer_whose_shapes_enter_a_corner_draws_in_place_rounded() {
    let full = rounded_layer(
        20.0,
        shapes_run(vec![white_rect(rect(0.0, 0.0, 200.0, 100.0))]),
    );
    assert!(matches!(
        child_placement(&full, RasterScale::Exact(1.0)),
        Placement::DirectRounded(_, radius) if radius == 20.0
    ));
    let loose = rounded_layer(20.0, drawn_node(white_rect(rect(0.0, 0.0, 120.0, 100.0))));
    assert!(matches!(
        child_placement(&loose, RasterScale::Exact(1.0)),
        Placement::DirectRounded(..)
    ));
    let inside = rounded_layer(
        20.0,
        shapes_run(vec![white_rect(rect(14.0, 14.0, 172.0, 72.0))]),
    );
    assert!(
        matches!(
            child_placement(&inside, RasterScale::Exact(1.0)),
            Placement::Direct(_)
        ),
        "content clear of the corners needs no rounding at all"
    );
}

#[test]
fn a_rounded_layer_whose_text_or_image_enters_a_corner_isolates() {
    let text = rounded_layer(20.0, drawn_node(snap_test_text()));
    assert!(matches!(
        child_placement(&text, RasterScale::Exact(1.0)),
        Placement::Isolated
    ));
    let mut mixed = rounded_layer(
        20.0,
        shapes_run(vec![white_rect(rect(0.0, 0.0, 200.0, 100.0))]),
    );
    mixed.children.push(drawn_node(snap_test_text()));
    assert!(matches!(
        child_placement(&mixed, RasterScale::Exact(1.0)),
        Placement::Isolated
    ));
}

#[test]
fn a_rounded_layer_with_uneven_corners_isolates_shapes_in_a_corner() {
    let mut layer = rounded_layer(
        20.0,
        shapes_run(vec![white_rect(rect(0.0, 0.0, 200.0, 100.0))]),
    );
    layer.graphics_layer.shape = LayerShape::Rounded(RoundedCornerShape::new(20.0, 20.0, 4.0, 4.0));
    assert!(matches!(
        child_placement(&layer, RasterScale::Exact(1.0)),
        Placement::Isolated
    ));
}

#[test]
fn a_rounded_layer_under_a_clip_that_cuts_it_or_another_rounded_clip_isolates() {
    let layer = rounded_layer(
        20.0,
        shapes_run(vec![white_rect(rect(0.0, 0.0, 200.0, 100.0))]),
    );
    let context = |visual_clip: Option<Rect>, clip_radius: f32| WalkContext {
        offset: Point::new(10.0, 10.0),
        visual_clip,
        clip_radius,
        snap_anchor: None,
        translated: false,
        raster_scale: RasterScale::Exact(1.0),
        light: ShadowLight::for_window(100.0, 100.0, 1.0, 1.0),
        wants_pixel_sensitive: false,
    };
    assert!(matches!(
        placement_in(&layer, &context(Some(rect(0.0, 0.0, 400.0, 400.0)), 0.0)),
        Placement::DirectRounded(..)
    ));
    assert!(matches!(
        placement_in(&layer, &context(Some(rect(0.0, 0.0, 150.0, 400.0)), 0.0)),
        Placement::Isolated
    ));
    assert!(matches!(
        placement_in(&layer, &context(Some(rect(0.0, 0.0, 400.0, 400.0)), 8.0)),
        Placement::Isolated
    ));
}

#[test]
fn a_rounded_layer_its_parent_clip_holds_draws_in_place_whatever_the_float_sums() {
    // From the workspace port on a phone: the bar lies inside the panel's
    // clip, but the panel clip met with the bar's rect sums its width an
    // ulp away from the bar's own.
    let bar = LayerNode {
        local_bounds: rect(0.0, 0.0, 218.33333, 4.0),
        graphics_layer: GraphicsLayer {
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(2.0)),
            ..Default::default()
        }
        .into(),
        transform_to_parent: ProjectiveTransform::translation(979.3333, 220.33334),
        children: vec![shapes_run(vec![white_rect(rect(0.0, 0.0, 218.33333, 4.0))])],
        ..Default::default()
    };
    let panel = WalkContext {
        offset: Point::default(),
        visual_clip: Some(rect(900.0, 204.33334, 380.0, 147.66666)),
        clip_radius: 0.0,
        snap_anchor: None,
        translated: false,
        raster_scale: RasterScale::Exact(1.0),
        light: ShadowLight::for_window(100.0, 100.0, 1.0, 1.0),
        wants_pixel_sensitive: false,
    };
    assert!(matches!(
        placement_in(&bar, &panel),
        Placement::DirectRounded(..)
    ));
    let rounded = WalkContext {
        visual_clip: Some(rect(979.3333, 220.33334, 218.33333, 4.0)),
        clip_radius: 2.0,
        ..panel
    };
    assert_eq!(
        radius_within(Some(rect(979.3333, 220.33334, 218.33333, 4.0)), &rounded),
        2.0
    );
    assert_eq!(
        radius_within(None, &rounded),
        2.0,
        "a layer with no clip keeps the rounding"
    );
    assert_eq!(
        radius_within(Some(rect(979.3333, 220.33334, 100.0, 4.0)), &rounded),
        0.0,
        "a clip that cuts the rounded one keeps none"
    );
}

#[test]
fn shapes_of_a_rounded_layer_drawn_in_place_take_its_radius_and_nothing_else_does() {
    let mut parent = LayerNode {
        local_bounds: rect(0.0, 0.0, 400.0, 300.0),
        ..Default::default()
    };
    parent
        .children
        .push(RenderNode::Layer(Box::new(rounded_layer(
            20.0,
            shapes_run(vec![white_rect(rect(0.0, 0.0, 200.0, 100.0))]),
        ))));
    parent
        .children
        .push(shapes_run(vec![white_rect(rect(0.0, 200.0, 50.0, 50.0))]));
    let scene = collect_root(
        &parent,
        &mut crate::pipeline::UiTextLayoutResolver,
        &mut LayerMotion::default(),
        SceneCapacityHint::default(),
        1.0,
        &mut LayerSceneRecycler::default(),
    );
    assert!(
        scene.children.is_empty(),
        "the rounded layer composites nothing"
    );
    let radii: Vec<f32> = scene
        .scene
        .runs
        .iter()
        .map(|run| run.placement.clip_radius)
        .collect();
    assert_eq!(radii, vec![20.0, 0.0]);
    assert_eq!(
        scene.scene.runs[0].placement.clip,
        Some(rect(0.0, 0.0, 200.0, 100.0))
    );
}

#[test]
fn an_animated_raster_scale_is_never_smaller_and_at_most_one_step_larger() {
    let step = 2f32.powf(1.0 / ANIMATED_RASTER_STEPS_PER_OCTAVE);
    for index in 0..400 {
        let scale = 0.25 + index as f32 * 0.01;
        let raster = animated_raster_scale(scale);
        assert!(raster >= scale, "{scale} rasterized at {raster}");
        assert!(
            raster <= scale * step * 1.0001,
            "{scale} rasterized at {raster}, more than one step up"
        );
    }
    assert_eq!(animated_raster_scale(0.9), animated_raster_scale(0.905));
    assert_eq!(animated_raster_scale(1.0), 1.0);
}

#[test]
fn layer_motion_keeps_the_raster_for_unchanged_content_across_small_scale_changes() {
    let mut motion = LayerMotion::default();
    assert_eq!(
        raster_scale(&mut motion, Some(1), 0.9, 7, true),
        0.9,
        "a layer seen for the first time rasterizes at its own scale"
    );
    motion.end_frame();
    assert_eq!(raster_scale(&mut motion, Some(1), 0.91, 7, true), 0.9);
    motion.end_frame();
    assert_eq!(
        raster_scale(&mut motion, Some(1), 0.91, 7, true),
        0.9,
        "a repeated scale must not reposition the content on a different raster grid"
    );
    motion.end_frame();
    assert_eq!(
        raster_scale(&mut motion, Some(1), 0.92, 8, true),
        0.92,
        "new content is drawn afresh anyway, so it rasterizes at its own scale"
    );
    motion.end_frame();
    assert_eq!(
        raster_scale(&mut motion, Some(1), 0.93, 8, false),
        0.93,
        "a layer the cache cannot hold gains nothing from a stepped scale"
    );
    assert_eq!(raster_scale(&mut motion, None, 0.93, 8, true), 0.93);
    motion.end_frame();
    motion.end_frame();
    assert_eq!(
        raster_scale(&mut motion, Some(1), 0.95, 8, true),
        0.95,
        "a layer missing from the last frame starts over"
    );
}

#[test]
fn a_scaling_layer_keeps_its_raster_within_the_resolution_range() {
    let mut motion = LayerMotion::default();
    let mut frame = |scale: f32| {
        let raster = raster_scale(&mut motion, Some(1), scale, 7, true);
        motion.end_frame();
        raster
    };
    assert_eq!(frame(1.0), 1.0);
    for scale in [0.95, 0.8, 0.6, 0.51, 0.7, 0.99, 1.04, 1.08, 1.08] {
        assert_eq!(frame(scale), 1.0, "{scale} draws from the raster at 1.0");
    }
    let shrunk = frame(0.45);
    assert_eq!(
        shrunk,
        animated_raster_scale(0.45),
        "past an octave down the raster steps down"
    );
    assert_eq!(frame(0.3), shrunk);
    let grown = frame(0.55);
    assert_eq!(
        grown,
        animated_raster_scale(0.55),
        "a scale the raster no longer covers steps up"
    );
    assert!(grown > shrunk);
}

fn turn(degrees: f32) -> ProjectiveTransform {
    let (sin, cos) = degrees.to_radians().sin_cos();
    ProjectiveTransform::from_rect_to_quad(
        rect(0.0, 0.0, 1.0, 1.0),
        [[0.0, 0.0], [cos, sin], [-sin, cos], [cos - sin, sin + cos]],
    )
}

fn solid(primitive_rect: Rect) -> DrawPrimitive {
    DrawPrimitive::Rect {
        rect: primitive_rect,
        brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
        stroke: None,
    }
}

fn run(primitives: Vec<DrawPrimitive>) -> RenderNode {
    RenderNode::DrawRun(DrawRunNode::new(PrimitivePhase::BeforeChildren, primitives))
}

fn turned_layer(
    transform: ProjectiveTransform,
    graphics_layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        local_bounds: rect(0.0, 0.0, 60.0, 40.0),
        transform_to_parent: transform,
        graphics_layer: graphics_layer.into(),
        children,
        ..Default::default()
    }
}

/// The isolated child `layer` collects into under an unturned root.
fn collected(layer: LayerNode) -> ChildLayer {
    let root = LayerNode {
        local_bounds: rect(0.0, 0.0, 200.0, 200.0),
        children: vec![RenderNode::Layer(Box::new(layer))],
        ..Default::default()
    };
    let mut scene = collect_root(
        &root,
        &mut crate::pipeline::UiTextLayoutResolver,
        &mut LayerMotion::default(),
        SceneCapacityHint::default(),
        1.0,
        &mut LayerSceneRecycler::default(),
    );
    scene.children.pop().expect("a turned layer isolates")
}

#[test]
fn a_transform_that_scales_evenly_turns_and_moves_has_its_scale() {
    let scale_of = |transform| similarity_scale(transform).map(|scale| (scale * 1e4).round() / 1e4);
    assert_eq!(scale_of(turn(33.0)), Some(1.0));
    assert_eq!(
        scale_of(turn(-7.5).then(ProjectiveTransform::translation(12.0, -3.0))),
        Some(1.0)
    );
    assert_eq!(scale_of(ProjectiveTransform::uniform_scale(1.5)), Some(1.5));
    assert_eq!(
        scale_of(turn(20.0).then(ProjectiveTransform::uniform_scale(0.5))),
        Some(0.5)
    );
    assert_eq!(scale_of(ProjectiveTransform::uniform_scale(0.0)), None);
    assert_eq!(
        scale_of(ProjectiveTransform::from_rect_to_quad(
            rect(0.0, 0.0, 1.0, 1.0),
            [[0.0, 0.0], [2.0, 0.0], [0.0, 1.0], [2.0, 1.0]],
        )),
        None,
        "an uneven scale resamples"
    );
    assert_eq!(
        scale_of(ProjectiveTransform::from_rect_to_quad(
            rect(0.0, 0.0, 1.0, 1.0),
            [[0.0, 0.0], [1.0, 0.1], [0.0, 1.0], [0.8, 1.3]],
        )),
        None
    );
}

#[test]
fn a_scaled_layer_draws_in_place_at_its_raster_scale_until_its_scale_moves_within_a_held_raster() {
    assert_eq!(in_place_content_scale(1.0, 1.4), Some(1.0));
    assert_eq!(in_place_content_scale(0.7, 0.7), Some(0.7));
    assert_eq!(in_place_content_scale(0.7, 0.9), None);
}

#[test]
fn a_layer_that_scales_evenly_at_its_raster_scale_draws_in_place() {
    let scaled = |transform| {
        collected(turned_layer(
            transform,
            GraphicsLayer {
                scale: 0.7,
                ..Default::default()
            },
            vec![run(vec![solid(rect(0.0, 0.0, 60.0, 40.0))])],
        ))
    };
    assert!(scaled(ProjectiveTransform::uniform_scale(0.7)).in_place);
    assert!(scaled(turn(20.0).then(ProjectiveTransform::uniform_scale(0.7))).in_place);
    assert!(
        !scaled(ProjectiveTransform::uniform_scale(0.5)).in_place,
        "a transform scaling past the layer's raster scale composites its surface"
    );
}

#[test]
fn a_layer_that_only_turns_draws_in_place() {
    let child = collected(turned_layer(
        turn(20.0),
        GraphicsLayer::default(),
        vec![run(vec![
            solid(rect(0.0, 0.0, 60.0, 40.0)),
            solid(rect(4.0, 4.0, 8.0, 8.0)),
        ])],
    ));
    assert!(child.in_place);
}

#[test]
fn a_turned_layer_that_needs_a_surface_for_more_than_its_turn_keeps_it() {
    let content = || vec![run(vec![solid(rect(0.0, 0.0, 60.0, 40.0))])];
    for graphics_layer in [
        GraphicsLayer {
            alpha: 0.5,
            ..Default::default()
        },
        GraphicsLayer {
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        },
        GraphicsLayer {
            blend_mode: BlendMode::Multiply,
            ..Default::default()
        },
        GraphicsLayer {
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
            ..Default::default()
        },
    ] {
        let child = collected(turned_layer(turn(20.0), graphics_layer.clone(), content()));
        assert!(!child.in_place, "{graphics_layer:?} needs a surface");
    }
    let scaled = collected(turned_layer(
        turn(20.0).then(ProjectiveTransform::uniform_scale(1.5)),
        GraphicsLayer::default(),
        content(),
    ));
    assert!(
        !scaled.in_place,
        "a scaled layer would resample what it draws"
    );
}

#[test]
fn content_that_blends_through_to_the_page_keeps_its_surface() {
    let child = collected(turned_layer(
        turn(20.0),
        GraphicsLayer::default(),
        vec![run(vec![
            solid(rect(0.0, 0.0, 60.0, 40.0)),
            DrawPrimitive::Blend {
                primitive: Box::new(solid(rect(10.0, 10.0, 20.0, 20.0))),
                blend_mode: BlendMode::DstOut,
            },
        ])],
    ));
    assert!(
        !child.in_place,
        "a hole punched in place would reach the pixels beneath the layer"
    );
}

#[test]
fn a_turned_layer_whose_clip_would_cut_a_turned_child_keeps_its_surface() {
    let inner = turned_layer(
        turn(30.0),
        GraphicsLayer::default(),
        vec![run(vec![solid(rect(0.0, 0.0, 60.0, 40.0))])],
    );
    let clipping = collected(turned_layer(
        turn(-10.0),
        GraphicsLayer {
            clip: true,
            ..Default::default()
        },
        vec![RenderNode::Layer(Box::new(inner.clone()))],
    ));
    assert!(
        clipping.content.children[0].in_place,
        "the inner layer only turns"
    );
    assert!(
        !clipping.in_place,
        "a clip turned off the pixel grid is no scissor for the child it cuts"
    );
    let open = collected(turned_layer(
        turn(-10.0),
        GraphicsLayer::default(),
        vec![
            run(vec![solid(rect(0.0, 0.0, 60.0, 40.0))]),
            RenderNode::Layer(Box::new(inner)),
        ],
    ));
    assert!(open.in_place);
}

#[test]
fn a_turned_layer_with_an_image_keeps_its_surface() {
    let image = RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(Box::new(cranpose_render_common::graph::DrawPrimitiveNode {
            primitive: DrawPrimitive::Image {
                rect: rect(0.0, 0.0, 60.0, 40.0),
                image: cranpose_ui_graphics::ImageBitmap::from_rgba8(1, 1, vec![255; 4])
                    .expect("a one-pixel bitmap"),
                alpha: 1.0,
                color_filter: None,
                sampling: cranpose_ui_graphics::ImageSampling::Nearest,
                src_rect: None,
            },
            clip: None,
        })),
    });
    let child = collected(turned_layer(
        turn(20.0),
        GraphicsLayer::default(),
        vec![image],
    ));
    assert!(
        !child.in_place,
        "an image drawn turned in place would have hard edges its filtered surface does not"
    );
}

fn glass_layer(transform: ProjectiveTransform) -> LayerNode {
    turned_layer(
        transform,
        GraphicsLayer {
            alpha: 0.5,
            backdrop_effect: Some(RenderEffect::blur(4.0)),
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
            ..Default::default()
        },
        Vec::new(),
    )
}

#[test]
fn a_detached_backdrop_keeps_its_original_capture_reach_and_paint_order() {
    let mut layer = glass_layer(ProjectiveTransform::translation(10.0, 20.0));
    layer.graphics_layer.alpha = 1.0;
    layer.graphics_layer.render_effect = Some(RenderEffect::blur(1.0));
    let root = LayerNode {
        local_bounds: rect(0.0, 0.0, 100.0, 100.0),
        clip_to_bounds: true,
        children: vec![RenderNode::Layer(Box::new(layer))],
        ..Default::default()
    };
    let collected = collect_root(
        &root,
        &mut crate::pipeline::UiTextLayoutResolver,
        &mut LayerMotion::default(),
        SceneCapacityHint::default(),
        1.0,
        &mut LayerSceneRecycler::default(),
    );
    let [backdrop] = collected.scene.backdrop_layers.as_slice() else {
        panic!("the isolated layer's backdrop must be batched");
    };
    let [child] = collected.children.as_slice() else {
        panic!("one foreground surface");
    };
    assert!(child.backdrop.is_none());
    assert_eq!(
        backdrop.reach, None,
        "the attached path reads past the inherited clip"
    );
    assert_eq!(backdrop.clip, child.clip);
    assert!(
        backdrop.z_index < child.z_index,
        "glass reads the page before its foreground"
    );
}

/// The draw ops a root clipped to 100×100 collects around `child`.
fn draw_ops_under_clip(child: LayerNode) -> usize {
    let root = LayerNode {
        local_bounds: rect(0.0, 0.0, 100.0, 100.0),
        clip_to_bounds: true,
        children: vec![RenderNode::Layer(Box::new(child))],
        ..Default::default()
    };
    collect_root(
        &root,
        &mut crate::pipeline::UiTextLayoutResolver,
        &mut LayerMotion::default(),
        SceneCapacityHint::default(),
        1.0,
        &mut LayerSceneRecycler::default(),
    )
    .scene
    .draw_ops
    .len()
}

fn row_at(y: f32, draws_within_bounds: bool, shadow_elevation: f32) -> LayerNode {
    LayerNode {
        local_bounds: rect(0.0, 0.0, 100.0, 40.0),
        transform_to_parent: ProjectiveTransform::translation(0.0, y),
        graphics_layer: GraphicsLayer {
            shadow_elevation,
            ..Default::default()
        }
        .into(),
        draws_within_bounds,
        children: vec![run(vec![solid(rect(0.0, 0.0, 100.0, 40.0))])],
        ..Default::default()
    }
}

#[test]
fn a_contained_row_the_clip_leaves_out_is_not_collected() {
    assert!(
        draw_ops_under_clip(row_at(30.0, true, 0.0)) > 0,
        "a visible row draws"
    );
    assert_eq!(draw_ops_under_clip(row_at(300.0, true, 0.0)), 0);
}

#[test]
fn a_row_that_may_draw_past_its_bounds_is_collected_offscreen() {
    assert!(
        draw_ops_under_clip(row_at(300.0, false, 0.0)) > 0,
        "without the promise its draws may reach the clip"
    );
    assert!(
        draw_ops_under_clip(row_at(101.5, true, 4.0)) > 0,
        "a shadow reaches past the row's bounds"
    );
}

fn text_root(style: cranpose_ui::TextStyle) -> (LayerNode, std::sync::Arc<cranpose_ui::TextStyle>) {
    let style = std::sync::Arc::new(style);
    let text = cranpose_render_common::graph::TextPrimitiveNode {
        node_id: 5,
        rect: rect(0.0, 0.0, 80.0, 20.0),
        text: cranpose_ui::text::shared_plain_annotated_string("shared"),
        render_text: cranpose_ui::text::shared_plain_render_string("shared"),
        text_style: std::sync::Arc::clone(&style),
        font_size: 14.0,
        layout_options: cranpose_ui::TextLayoutOptions::default(),
        clip: None,
    };
    let root = LayerNode {
        local_bounds: rect(0.0, 0.0, 100.0, 100.0),
        children: vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(text)),
        })],
        ..Default::default()
    };
    (root, style)
}

#[test]
fn a_plain_texts_draw_carries_its_nodes_style() {
    let (root, style) = text_root(cranpose_ui::TextStyle::default());
    let scene = collect_root(
        &root,
        &mut crate::pipeline::UiTextLayoutResolver,
        &mut LayerMotion::default(),
        SceneCapacityHint::default(),
        1.0,
        &mut LayerSceneRecycler::default(),
    )
    .scene;
    assert_eq!(scene.texts.len(), 1);
    assert!(std::sync::Arc::ptr_eq(&scene.texts[0].text_style, &style));
}
