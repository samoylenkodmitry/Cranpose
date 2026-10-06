use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

use super::*;

fn test_layer(local_bounds: Rect, children: Vec<RenderNode>) -> LayerNode {
    LayerNode {
        local_bounds,
        children,
        ..Default::default()
    }
}

#[test]
fn projective_transform_translation_maps_points() {
    let transform = ProjectiveTransform::translation(7.0, -3.5);
    let mapped = transform.map_point(Point { x: 2.0, y: 4.0 });
    assert!((mapped.x - 9.0).abs() < 1e-6);
    assert!((mapped.y - 0.5).abs() < 1e-6);
}

#[test]
fn projective_transform_then_composes_in_parent_order() {
    let child = ProjectiveTransform::translation(4.0, 2.0);
    let parent = ProjectiveTransform::translation(10.0, -1.0);
    let composed = child.then(parent);
    let mapped = composed.map_point(Point { x: 1.0, y: 1.0 });
    assert!((mapped.x - 15.0).abs() < 1e-6);
    assert!((mapped.y - 2.0).abs() < 1e-6);
}

#[test]
fn homography_maps_rect_corners_to_target_quad() {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 10.0,
    };
    let quad = [[5.0, 7.0], [25.0, 6.0], [7.0, 20.0], [28.0, 21.0]];
    let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);
    let mapped = transform.map_rect(rect);
    for (expected, actual) in quad.into_iter().zip(mapped) {
        assert!((expected[0] - actual[0]).abs() < 1e-4);
        assert!((expected[1] - actual[1]).abs() < 1e-4);
    }
}

#[test]
fn axis_aligned_rect_to_quad_keeps_exact_affine_matrix() {
    let rect = Rect {
        x: 2.0,
        y: 3.0,
        width: 20.0,
        height: 10.0,
    };
    let quad = [[12.0, 9.0], [32.0, 9.0], [12.0, 19.0], [32.0, 19.0]];
    let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);

    assert_eq!(
        transform.matrix(),
        [[1.0, 0.0, 10.0], [0.0, 1.0, 6.0], [0.0, 0.0, 1.0]]
    );
}

#[test]
fn axis_aligned_rect_to_quad_keeps_exact_axis_aligned_scale() {
    let rect = Rect {
        x: 4.0,
        y: 6.0,
        width: 10.0,
        height: 8.0,
    };
    let quad = [[20.0, 18.0], [50.0, 18.0], [20.0, 42.0], [50.0, 42.0]];
    let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);

    assert_eq!(
        transform.matrix(),
        [[3.0, 0.0, 8.0], [0.0, 3.0, 0.0], [0.0, 0.0, 1.0]]
    );
}

#[test]
fn retained_visual_observation_nodes_collect_layers_and_command_owners() {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
    };
    let command = |node_id| DrawCommandId {
        node_id,
        command_index: 0,
        placement: DrawPlacement::Behind,
    };
    let mut child = test_layer(
        bounds,
        vec![RenderNode::DrawRun(DrawRunNode::for_command(
            PrimitivePhase::BeforeChildren,
            Some(command(17)),
            Vec::new(),
        ))],
    );
    child.node_id = Some(13);
    let mut root = test_layer(
        bounds,
        vec![
            RenderNode::DrawRun(DrawRunNode::for_command(
                PrimitivePhase::BeforeChildren,
                Some(command(9)),
                Vec::new(),
            )),
            RenderNode::DrawRun(DrawRunNode::new(PrimitivePhase::BeforeChildren, Vec::new())),
            RenderNode::Layer(Box::new(child)),
        ],
    );

    root.node_id = Some(5);

    let mut graph = RenderGraph::new(root);
    let mut nodes = HashSet::from_iter([999]);
    graph.collect_retained_visual_observation_nodes(&mut nodes);
    assert_eq!(nodes, HashSet::from_iter([5, 9, 13, 17]));
    let capacity = nodes.capacity();
    graph.root.children.clear();
    graph.root.node_id = Some(23);
    graph.collect_retained_visual_observation_nodes(&mut nodes);
    assert_eq!(nodes, HashSet::from_iter([23]));
    assert_eq!(nodes.capacity(), capacity);
}

#[test]
fn render_graph_new_recomputes_manual_layer_hashes() {
    let primitive = PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
            primitive: DrawPrimitive::Rect {
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 8.0,
                    height: 6.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            },
            clip: None,
        })),
    };
    let mut root = test_layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        },
        vec![RenderNode::Primitive(primitive)],
    );
    root.graphics_layer.render_effect = Some(RenderEffect::blur(3.0));
    let mut expected = root.clone();
    expected.forget_raster_cache_hashes();

    let graph = RenderGraph::new(root);
    assert_eq!(
        graph.root.target_content_hash(),
        expected.target_content_hash()
    );
    assert_eq!(graph.root.effect_hash(), expected.effect_hash());
}

#[test]
fn motion_source_content_hash_ignores_translated_content_offset() {
    let primitive = PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
            primitive: DrawPrimitive::Rect {
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 8.0,
                    height: 6.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            },
            clip: None,
        })),
    };
    let mut base = test_layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        },
        vec![RenderNode::Primitive(primitive)],
    );
    base.translated_content_context = true;
    base.translated_content_offset = Point::new(0.0, -24.0);
    base.forget_raster_cache_hashes();

    let mut moved = base.clone();
    moved.translated_content_offset = Point::new(0.0, -72.0);
    moved.forget_raster_cache_hashes();

    assert_ne!(base.target_content_hash(), moved.target_content_hash());
    assert_eq!(
        base.motion_source_content_hash(),
        moved.motion_source_content_hash()
    );
}

#[test]
fn render_nodes_stay_small_enough_to_walk_densely() {
    assert!(
        std::mem::size_of::<RenderNode>() <= 48,
        "a layer's children are walked every frame; keep large payloads boxed, got {} bytes",
        std::mem::size_of::<RenderNode>()
    );
}

fn contained_run(rect: Rect) -> RenderNode {
    RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        vec![DrawPrimitive::Rect {
            rect,
            brush: Brush::solid(Color::WHITE),
            stroke: None,
        }],
    ))
}

fn bounded(width: f32, height: f32, children: Vec<RenderNode>) -> LayerNode {
    LayerNode {
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        children,
        ..Default::default()
    }
}

fn inner(x: f32, y: f32, draws_within_bounds: bool) -> RenderNode {
    RenderNode::Layer(Box::new(LayerNode {
        transform_to_parent: ProjectiveTransform::translation(x, y),
        draws_within_bounds,
        ..bounded(20.0, 20.0, Vec::new())
    }))
}

#[test]
fn content_inside_the_bounds_draws_within_them() {
    let rect = |x, y, width, height| Rect {
        x,
        y,
        width,
        height,
    };
    assert!(
        bounded(50.0, 50.0, vec![contained_run(rect(0.0, 0.0, 50.0, 50.0))])
            .content_draws_within_bounds()
    );
    assert!(
        bounded(50.0, 50.0, vec![contained_run(rect(-0.5, 0.0, 50.0, 50.5))])
            .content_draws_within_bounds(),
        "antialiasing slack"
    );
    assert!(
        !bounded(50.0, 50.0, vec![contained_run(rect(0.0, 0.0, 60.0, 50.0))])
            .content_draws_within_bounds()
    );
    assert!(bounded(50.0, 50.0, vec![inner(10.0, 10.0, true)]).content_draws_within_bounds());
    assert!(
        !bounded(50.0, 50.0, vec![inner(40.0, 10.0, true)]).content_draws_within_bounds(),
        "a child placed partly outside"
    );
    assert!(
        !bounded(50.0, 50.0, vec![inner(10.0, 10.0, false)]).content_draws_within_bounds(),
        "a child that may draw anywhere"
    );
}

#[test]
fn a_clip_or_a_shadow_decides_containment_whatever_the_content() {
    let overflowing = inner(40.0, 40.0, false);
    assert!(
        LayerNode {
            clip_to_bounds: true,
            ..bounded(50.0, 50.0, vec![overflowing])
        }
        .content_draws_within_bounds()
    );
    let shadowed = RenderNode::Layer(Box::new(LayerNode {
        graphics_layer: GraphicsLayer {
            shadow_elevation: 6.0,
            ..Default::default()
        }
        .into(),
        draws_within_bounds: true,
        ..bounded(20.0, 20.0, Vec::new())
    }));
    assert!(!bounded(50.0, 50.0, vec![shadowed]).content_draws_within_bounds());
}

#[test]
fn a_group_opacity_layer_cuts_its_drawing_at_its_bounds_but_not_its_hits() {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 10.0,
    };
    let layer = LayerNode {
        isolation: IsolationReasons {
            group_opacity: true,
            ..Default::default()
        },
        ..test_layer(bounds, Vec::new())
    };
    assert_eq!(layer.clip_rect(), None);
    assert_eq!(layer.visual_clip_rect(), Some(bounds));
    assert_eq!(test_layer(bounds, Vec::new()).visual_clip_rect(), None);
}

#[test]
fn a_node_rect_is_its_own_rect_or_else_its_layer_bounds() {
    let layer_bounds = Rect {
        x: 4.0,
        y: 4.0,
        width: 12.0,
        height: 2.0,
    };
    let node = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 10.0,
    };
    assert_eq!(
        test_layer(layer_bounds, Vec::new()).node_rect(),
        layer_bounds
    );
    let split = LayerNode {
        node_bounds: Some(node),
        ..test_layer(layer_bounds, Vec::new())
    };
    assert_eq!(split.node_rect(), node);
}

#[test]
fn a_homogeneous_matrix_is_scaled_to_a_last_entry_of_one() {
    let doubled = [[2.0, 0.0, 4.0], [0.0, 2.0, 6.0], [0.0, 0.0, 2.0]];
    assert_eq!(
        ProjectiveTransform::from_homogeneous(doubled),
        ProjectiveTransform::translation(2.0, 3.0)
    );
    let vanishing = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
    assert_eq!(
        ProjectiveTransform::from_homogeneous(vanishing).matrix(),
        vanishing,
        "a matrix whose last entry is zero has no such scale and is kept"
    );
}
