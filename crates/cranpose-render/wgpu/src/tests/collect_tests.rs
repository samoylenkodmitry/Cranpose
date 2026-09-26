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

fn corners(width: f32, height: f32, radius: f32) -> RoundedClipCorners {
    RoundedClipCorners::of(LayerRoundedClip {
        rect: rect(0.0, 0.0, width, height),
        radii: [radius; 4],
    })
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

#[test]
fn a_rounded_layer_whose_content_enters_a_corner_isolates() {
    let mut layer = LayerNode {
        local_bounds: rect(0.0, 0.0, 200.0, 100.0),
        graphics_layer: GraphicsLayer {
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(20.0)),
            ..Default::default()
        },
        ..Default::default()
    };
    layer.children.push(RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        vec![DrawPrimitive::Rect {
            rect: rect(0.0, 0.0, 200.0, 100.0),
            brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
            stroke: None,
        }],
    )));
    assert!(matches!(child_placement(&layer), Placement::Isolated));
    let RenderNode::DrawRun(run) = &mut layer.children[0] else {
        unreachable!()
    };
    *run = DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        vec![DrawPrimitive::Rect {
            rect: rect(14.0, 14.0, 172.0, 72.0),
            brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
            stroke: None,
        }],
    );
    assert!(matches!(child_placement(&layer), Placement::Direct(_)));
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
fn layer_motion_steps_a_scale_only_while_it_changes_over_the_same_content() {
    let mut motion = LayerMotion::default();
    assert_eq!(
        motion.raster_scale(Some(1), 0.9, 7, true),
        0.9,
        "a layer seen for the first time rasterizes at its own scale"
    );
    motion.end_frame();
    let stepped = motion.raster_scale(Some(1), 0.91, 7, true);
    assert_eq!(stepped, animated_raster_scale(0.91));
    assert!(stepped > 0.91);
    motion.end_frame();
    assert_eq!(
        motion.raster_scale(Some(1), 0.91, 7, true),
        0.91,
        "a scale that holds for a frame rasterizes exactly again"
    );
    motion.end_frame();
    assert_eq!(
        motion.raster_scale(Some(1), 0.92, 8, true),
        0.92,
        "new content is drawn afresh anyway, so it rasterizes at its own scale"
    );
    motion.end_frame();
    assert_eq!(
        motion.raster_scale(Some(1), 0.93, 8, false),
        0.93,
        "a layer the cache cannot hold gains nothing from a stepped scale"
    );
    assert_eq!(motion.raster_scale(None, 0.93, 8, true), 0.93);
    motion.end_frame();
    motion.end_frame();
    assert_eq!(
        motion.raster_scale(Some(1), 0.95, 8, true),
        0.95,
        "a layer missing from the last frame starts over"
    );
}

#[test]
fn a_scaling_layer_keeps_its_raster_while_it_covers_the_scale_within_an_octave() {
    let mut motion = LayerMotion::default();
    let mut frame = |scale: f32| {
        let raster = motion.raster_scale(Some(1), scale, 7, true);
        motion.end_frame();
        raster
    };
    assert_eq!(frame(1.0), 1.0);
    for scale in [0.95, 0.8, 0.6, 0.51, 0.7, 0.99] {
        assert_eq!(frame(scale), 1.0, "{scale} draws from the raster at 1.0");
    }
    let shrunk = frame(0.45);
    assert_eq!(
        shrunk,
        animated_raster_scale(0.45),
        "past an octave down the raster steps down"
    );
    assert_eq!(frame(0.3), shrunk);
    let grown = frame(0.5);
    assert_eq!(
        grown,
        animated_raster_scale(0.5),
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
        graphics_layer,
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
    );
    scene.children.pop().expect("a turned layer isolates")
}

#[test]
fn a_transform_that_only_turns_and_moves_is_rigid() {
    assert!(is_rigid(turn(33.0)));
    assert!(is_rigid(
        turn(-7.5).then(ProjectiveTransform::translation(12.0, -3.0))
    ));
    assert!(!is_rigid(ProjectiveTransform::uniform_scale(1.5)));
    assert!(!is_rigid(
        turn(20.0).then(ProjectiveTransform::uniform_scale(0.5))
    ));
    assert!(!is_rigid(ProjectiveTransform::from_rect_to_quad(
        rect(0.0, 0.0, 1.0, 1.0),
        [[0.0, 0.0], [1.0, 0.1], [0.0, 1.0], [0.8, 1.3]],
    )));
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
        node: PrimitiveNode::Draw(cranpose_render_common::graph::DrawPrimitiveNode {
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
        }),
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
fn a_turned_layer_reads_its_backdrop_in_its_own_space_under_its_turn() {
    let outer = collected(glass_layer(turn(20.0)));

    assert!(
        outer.backdrop.is_none(),
        "the turn carries no backdrop of its own"
    );
    assert!(outer.effect.is_none());
    assert!(outer.rounded_clip.is_none());
    assert_eq!(outer.alpha, GraphicsLayer::composite_alpha_8bit(0.5));
    let [inner] = outer.content.children.as_slice() else {
        panic!("the backdrop runs in one child in the layer's own space");
    };
    assert!(inner.backdrop.is_some());
    assert!(inner.rounded_clip.is_some());
    assert_eq!(inner.alpha, 1.0);
    assert_eq!(
        uniform_scale_translation(inner.transform),
        Some((1.0, Point::default())),
        "the inner child sits in the turned layer's own space"
    );
}

#[test]
fn a_moved_layer_resolves_its_backdrop_beside_its_surface() {
    let child = collected(glass_layer(ProjectiveTransform::translation(10.0, 20.0)));

    assert!(child.backdrop.is_some());
    assert!(child.rounded_clip.is_some());
    assert!(child.content.children.is_empty());
}
