use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

use super::*;
use crate::scene_store::{LayerRef, SceneStore};

// The root record of `store`.
fn root_of(store: &SceneStore) -> LayerRef<'_> {
    store.root_layer().expect("the store holds the layer")
}

fn test_layer(local_bounds: Rect, children: Vec<RenderNode>) -> LayerNode {
    LayerNode {
        size: local_bounds.size(),
        layer_bounds: Some(local_bounds),
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

    let mut moved = base.clone();
    moved.translated_content_offset = Point::new(0.0, -72.0);

    let (base, moved) = (
        SceneStore::from_layer(&base),
        SceneStore::from_layer(&moved),
    );
    assert_ne!(
        root_of(&base).target_content_hash(),
        root_of(&moved).target_content_hash()
    );
    assert_eq!(
        root_of(&base).motion_source_content_hash(),
        root_of(&moved).motion_source_content_hash()
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
        size: Size::new(width, height),
        children,
        ..Default::default()
    }
}

fn inner(x: f32, y: f32, draws_within_bounds: bool) -> RenderNode {
    let reach = if draws_within_bounds { 20.0 } else { 40.0 };
    RenderNode::Layer(Box::new(LayerNode {
        transform_to_parent: ProjectiveTransform::translation(x, y),
        ..bounded(
            20.0,
            20.0,
            vec![contained_run(Rect {
                x: 0.0,
                y: 0.0,
                width: reach,
                height: reach,
            })],
        )
    }))
}

// Whether the scene of `layer` draws within the layer's bounds.
fn draws_within_bounds(layer: &LayerNode) -> bool {
    root_of(&SceneStore::from_layer(layer)).draws_within_bounds()
}

#[test]
fn content_inside_the_bounds_draws_within_them() {
    let rect = |x, y, width, height| Rect {
        x,
        y,
        width,
        height,
    };
    let run = |area| bounded(50.0, 50.0, vec![contained_run(area)]);
    assert!(draws_within_bounds(&run(rect(0.0, 0.0, 50.0, 50.0))));
    assert!(
        draws_within_bounds(&run(rect(-0.5, 0.0, 50.0, 50.5))),
        "antialiasing slack"
    );
    assert!(!draws_within_bounds(&run(rect(0.0, 0.0, 60.0, 50.0))));
    let child = |child| bounded(50.0, 50.0, vec![child]);
    assert!(draws_within_bounds(&child(inner(10.0, 10.0, true))));
    assert!(
        !draws_within_bounds(&child(inner(40.0, 10.0, true))),
        "a child placed partly outside"
    );
    assert!(
        !draws_within_bounds(&child(inner(10.0, 10.0, false))),
        "a child that draws past its bounds"
    );
}

#[test]
fn a_clip_or_a_shadow_decides_containment_whatever_the_content() {
    let overflowing = inner(40.0, 40.0, false);
    assert!(draws_within_bounds(&LayerNode {
        clip_to_bounds: true,
        ..bounded(50.0, 50.0, vec![overflowing])
    }));
    let RenderNode::Layer(mut shadowed) = inner(10.0, 10.0, true) else {
        unreachable!("inner makes a layer");
    };
    shadowed.graphics_layer.shadow_elevation = 6.0;
    assert!(!draws_within_bounds(&bounded(
        50.0,
        50.0,
        vec![RenderNode::Layer(shadowed)]
    )));
}

#[test]
fn a_group_opacity_layer_cuts_its_drawing_at_its_bounds_but_not_its_hits() {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 10.0,
    };
    let layer = SceneStore::from_layer(&LayerNode {
        isolation: IsolationReasons {
            group_opacity: true,
            ..Default::default()
        },
        ..test_layer(bounds, Vec::new())
    });
    assert_eq!(root_of(&layer).clip_rect(), None);
    assert_eq!(root_of(&layer).visual_clip_rect(), Some(bounds));
    let plain = SceneStore::from_layer(&test_layer(bounds, Vec::new()));
    assert_eq!(root_of(&plain).visual_clip_rect(), None);
}

#[test]
fn a_node_rect_starts_at_its_origin_and_its_layer_bounds_may_lie_apart() {
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
    let split = SceneStore::from_layer(&LayerNode {
        size: node.size(),
        ..test_layer(layer_bounds, Vec::new())
    });
    assert_eq!(root_of(&split).node_rect(), node);
    assert_eq!(root_of(&split).local_bounds(), layer_bounds);
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
