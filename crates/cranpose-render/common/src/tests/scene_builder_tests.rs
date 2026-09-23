use std::{cell::RefCell, rc::Rc};

use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    Color, Column, ColumnSpec, DrawCommand, LayoutEngine, LazyColumn, LazyColumnSpec,
    LinearArrangement, Modifier, Point, Rect, RoundedCornerShape, ScrollState, Size, Spacer, Text,
    TextStyle,
    text::{AnnotatedString, BaselineShift, SpanStyle, TextAlign, TextDirection, TextMotion},
};
use cranpose_ui_graphics::{
    Brush, DrawPrimitive, DrawScope as _, DrawScopeDefault, GraphicsLayer, RenderEffect,
};

use super::*;

#[test]
fn scene_snapshot_preserves_children_beyond_inline_capacity_in_order() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            for index in 0..12 {
                Text(
                    format!("child-{index}"),
                    Modifier::empty().height(20.0),
                    TextStyle::default(),
                );
            }
        });
    });
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(
            root,
            Size {
                width: 240.0,
                height: 300.0,
            },
        )
        .expect("layout");
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("graph");
    applier.clear_runtime_handle();
    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);
    assert_eq!(
        labels,
        (0..12)
            .map(|index| format!("child-{index}"))
            .collect::<Vec<_>>()
    );
}

fn find_text_motion(layer: &LayerNode, label: &str) -> Option<Option<TextMotion>> {
    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => {
                let PrimitiveNode::Text(text) = &primitive.node else {
                    continue;
                };
                if text.text.text == label {
                    return Some(text.text_style.paragraph_style.text_motion);
                }
            }
            RenderNode::Layer(child_layer) => {
                if let Some(motion) = find_text_motion(child_layer, label) {
                    return Some(motion);
                }
            }
            RenderNode::DrawRun(_) => {}
        }
    }

    None
}

fn collect_text_labels(layer: &LayerNode, labels: &mut Vec<String>) {
    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => {
                let PrimitiveNode::Text(text) = &primitive.node else {
                    continue;
                };
                labels.push(text.text.text.clone());
            }
            RenderNode::Layer(child_layer) => collect_text_labels(child_layer, labels),
            RenderNode::DrawRun(_) => {}
        }
    }
}

fn find_text_top(layer: &LayerNode, label: &str) -> Option<f32> {
    fn search(layer: &LayerNode, label: &str, transform: ProjectiveTransform) -> Option<f32> {
        for child in &layer.children {
            match child {
                RenderNode::Primitive(primitive) => {
                    let PrimitiveNode::Text(text) = &primitive.node else {
                        continue;
                    };
                    if text.text.text == label {
                        let quad = transform.map_rect(text.rect);
                        let top = quad
                            .iter()
                            .map(|point| point[1])
                            .fold(f32::INFINITY, f32::min);
                        return top.is_finite().then_some(top);
                    }
                }
                RenderNode::Layer(child_layer) => {
                    let child_transform = child_layer.transform_to_parent.then(transform);
                    if let Some(top) = search(child_layer, label, child_transform) {
                        return Some(top);
                    }
                }
                RenderNode::DrawRun(_) => {}
            }
        }
        None
    }

    search(layer, label, ProjectiveTransform::identity())
}

fn find_layer_by_node_id(layer: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
    if layer.node_id == Some(node_id) {
        return Some(layer);
    }
    layer.children.iter().find_map(|child| match child {
        RenderNode::Layer(child_layer) => find_layer_by_node_id(child_layer, node_id),
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
    })
}

fn find_layer_origin(layer: &LayerNode, node_id: NodeId) -> Option<Point> {
    fn search(layer: &LayerNode, node_id: NodeId, transform: ProjectiveTransform) -> Option<Point> {
        if layer.node_id == Some(node_id) {
            return Some(transform.map_point(Point::default()));
        }
        layer.children.iter().find_map(|child| match child {
            RenderNode::Layer(child_layer) => search(
                child_layer,
                node_id,
                child_layer.transform_to_parent.then(transform),
            ),
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
        })
    }

    search(layer, node_id, ProjectiveTransform::identity())
}

fn find_translated_content_offset(layer: &LayerNode) -> Option<Point> {
    if layer.translated_content_context {
        return Some(layer.translated_content_offset);
    }
    for child in &layer.children {
        if let RenderNode::Layer(child_layer) = child
            && let Some(offset) = find_translated_content_offset(child_layer)
        {
            return Some(offset);
        }
    }
    None
}

fn graph_has_runtime_shader_effect(layer: &LayerNode) -> bool {
    layer
        .graphics_layer
        .render_effect
        .as_ref()
        .is_some_and(RenderEffect::contains_runtime_shader)
        || layer.children.iter().any(|child| match child {
            RenderNode::Layer(child_layer) => graph_has_runtime_shader_effect(child_layer),
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
        })
}

fn build_layer_node_for_test(
    snapshot: BuildNodeSnapshot,
    scale: f32,
    has_external_backdrop_input: bool,
) -> LayerNode {
    let app_context = cranpose_ui::AppContext::new();
    app_context.enter(|| build_layer_node(snapshot, scale, has_external_backdrop_input))
}

fn snapshot_with_translation(tx: f32) -> BuildNodeSnapshot {
    let child_command = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
        scope.push_recorded(vec![DrawPrimitive::Rect {
            rect: Rect {
                x: 3.0,
                y: 4.0,
                width: 20.0,
                height: 8.0,
            },
            brush: Brush::solid(Color::WHITE),
            stroke: None,
        }]);
    }));

    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 11.0, y: 7.0 },
        size: Size {
            width: 40.0,
            height: 20.0,
        },
        draw_commands: vec![child_command],
        ..Default::default()
    };

    BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 80.0,
            height: 50.0,
        },
        graphics_layer: Some(GraphicsLayer {
            translation_x: tx,
            ..GraphicsLayer::default()
        }),
        children: vec![child],
        ..Default::default()
    }
}

#[test]
fn parent_translation_changes_layer_transform_but_not_child_local_geometry() {
    let static_graph = build_layer_node_for_test(snapshot_with_translation(0.0), 1.0, false);
    let moved_graph = build_layer_node_for_test(snapshot_with_translation(23.5), 1.0, false);

    let RenderNode::Layer(static_child) = &static_graph.children[0] else {
        panic!("expected child layer");
    };
    let RenderNode::Layer(moved_child) = &moved_graph.children[0] else {
        panic!("expected child layer");
    };
    let RenderNode::DrawRun(static_run) = &static_child.children[0] else {
        panic!("expected draw run");
    };
    let static_draw = static_run.primitives().next().expect("a recorded draw");
    let RenderNode::DrawRun(moved_run) = &moved_child.children[0] else {
        panic!("expected draw run");
    };
    let moved_draw = moved_run.primitives().next().expect("a recorded draw");

    assert_ne!(
        static_graph.transform_to_parent, moved_graph.transform_to_parent,
        "parent transform should encode translation"
    );
    assert_eq!(
        static_draw, moved_draw,
        "child local primitive geometry must stay stable under parent translation"
    );
}

#[test]
fn stored_content_hash_ignores_parent_translation() {
    let static_graph = build_layer_node_for_test(snapshot_with_translation(0.0), 1.0, false);
    let moved_graph = build_layer_node_for_test(snapshot_with_translation(23.5), 1.0, false);

    assert_eq!(
        static_graph.target_content_hash(),
        moved_graph.target_content_hash(),
        "parent rigid motion must not invalidate the subtree content hash"
    );
}

#[test]
fn parent_content_offset_is_encoded_in_child_transform() {
    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 11.0, y: 7.0 },
        size: Size {
            width: 40.0,
            height: 20.0,
        },
        ..Default::default()
    };

    let parent = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 80.0,
            height: 50.0,
        },
        content_offset: Point { x: 13.0, y: -9.0 },
        children: vec![child],
        ..Default::default()
    };

    let graph = build_layer_node_for_test(parent, 1.0, false);
    let RenderNode::Layer(child) = &graph.children[0] else {
        panic!("expected child layer");
    };

    let top_left = child.transform_to_parent.map_point(Point::default());
    assert_eq!(top_left, Point { x: 24.0, y: -2.0 });
}

#[test]
fn translated_content_offset_changes_visual_position_and_full_surface_hash() {
    fn parent_with_offset(offset: Point, motion_context_animated: bool) -> BuildNodeSnapshot {
        let child_command = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
            scope.push_recorded(vec![DrawPrimitive::Rect {
                rect: Rect {
                    x: 3.0,
                    y: 4.0,
                    width: 20.0,
                    height: 8.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            }]);
        }));

        let child = BuildNodeSnapshot {
            node_id: 2,
            placement: Point { x: 11.0, y: 7.0 },
            size: Size {
                width: 40.0,
                height: 20.0,
            },
            draw_commands: vec![child_command],
            ..Default::default()
        };

        BuildNodeSnapshot {
            node_id: 1,
            size: Size {
                width: 80.0,
                height: 50.0,
            },
            content_offset: offset,
            motion_context_animated,
            translated_content_context: true,
            children: vec![child],
            ..Default::default()
        }
    }

    let base = build_layer_node_for_test(
        parent_with_offset(Point { x: 0.0, y: -18.0 }, true),
        1.0,
        false,
    );
    let moved = build_layer_node_for_test(
        parent_with_offset(Point { x: 0.0, y: -32.0 }, true),
        1.0,
        false,
    );
    let rested = build_layer_node_for_test(
        parent_with_offset(Point { x: 0.0, y: -18.0 }, false),
        1.0,
        false,
    );

    let RenderNode::Layer(base_child) = &base.children[0] else {
        panic!("expected child layer");
    };
    let RenderNode::Layer(moved_child) = &moved.children[0] else {
        panic!("expected child layer");
    };

    assert_ne!(
        base_child.transform_to_parent.map_point(Point::default()),
        moved_child.transform_to_parent.map_point(Point::default()),
        "scroll offset still has to move child content visually"
    );
    assert_eq!(
        base_child.target_content_hash(),
        moved_child.target_content_hash(),
        "child source content identity stays stable when only the parent scroll offset changes"
    );
    assert_ne!(
        base.target_content_hash(),
        moved.target_content_hash(),
        "a full-surface cache of the scroll viewport must include the scroll offset"
    );
    assert_ne!(
        base.target_content_hash(),
        rested.target_content_hash(),
        "full-surface cache keys must include active scroll motion policy"
    );
}

#[test]
fn rounded_clip_to_bounds_records_shape_clip_without_runtime_shader() {
    let layer = graphics_layer_with_shaped_clip(
        GraphicsLayer::default(),
        true,
        Some(RoundedCornerShape::new(4.0, 8.0, 12.0, 16.0)),
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        },
    );

    assert!(layer.clip);
    assert!(layer.render_effect.is_none());
    let LayerShape::Rounded(shape) = layer.shape else {
        panic!("rounded clip must be recorded as layer shape");
    };
    assert_eq!(shape, RoundedCornerShape::new(4.0, 8.0, 12.0, 16.0));
    assert!(isolation_reasons(&layer).shape_clip);
}

#[test]
fn rounded_clip_to_bounds_keeps_existing_effect_inside_mask() {
    let existing = RenderEffect::blur(3.0);
    let layer = graphics_layer_with_shaped_clip(
        GraphicsLayer {
            render_effect: Some(existing.clone()),
            ..GraphicsLayer::default()
        },
        true,
        Some(RoundedCornerShape::uniform(10.0)),
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        },
    );

    let Some(RenderEffect::Chain { first, second }) = layer.render_effect else {
        panic!("existing effect should chain into rounded clip mask");
    };
    assert_eq!(*first, existing);
    assert!(
        matches!(*second, RenderEffect::Shader { .. }),
        "rounded mask must be the outer effect"
    );
}

#[test]
fn rounded_corners_clip_to_bounds_builds_graph_shape_clip_from_modifier_chain() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        cranpose_ui::Box(
            Modifier::empty()
                .width(100.0)
                .height(40.0)
                .rounded_corner_shape(RoundedCornerShape::new(4.0, 8.0, 12.0, 16.0))
                .clip_to_bounds(),
            cranpose_ui::BoxSpec::default(),
            || {
                Text("rounded child", Modifier::empty(), TextStyle::default());
            },
        );
    });

    let root = composition.root().expect("rounded clip root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(
            root,
            Size {
                width: 160.0,
                height: 100.0,
            },
        )
        .expect("rounded clip layout");
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("rounded clip graph");
    applier.clear_runtime_handle();

    let rounded_layer = find_layer_by_node_id(&graph.root, root).expect("rounded layer");
    assert!(rounded_layer.graphics_layer.clip);
    assert!(matches!(
        rounded_layer.graphics_layer.shape,
        LayerShape::Rounded(_)
    ));
    assert!(rounded_layer.graphics_layer.render_effect.is_none());
    assert!(rounded_layer.isolation.shape_clip);
    assert!(
        !graph_has_runtime_shader_effect(&graph.root),
        "simple rounded_corners().clip_to_bounds() must not become a runtime shader effect"
    );
}

fn wrapped_card_composition(
    label: Rc<RefCell<Option<cranpose_core::MutableState<String>>>>,
    card_id: Rc<RefCell<Option<NodeId>>>,
) -> cranpose_ui::TestComposition {
    cranpose_ui::run_test_composition(move || {
        let text = cranpose_core::rememberMutableStateOf(|| "before".to_string());
        *label.borrow_mut() = Some(text);
        let card_id = card_id.clone();
        cranpose_ui::Box(
            Modifier::empty().size_points(240.0, 120.0),
            cranpose_ui::BoxSpec::default(),
            move || {
                let shape = cranpose_ui::LayerShape::Rounded(
                    cranpose_ui::RoundedCornerShape::uniform(12.0),
                );
                let id = cranpose_ui::Box(
                    Modifier::empty()
                        .offset(20.0, 30.0)
                        .size_points(100.0, 40.0)
                        .drop_shadow(shape, |scope| scope.radius = 6.0)
                        .graphics_layer(move || GraphicsLayer {
                            shape,
                            clip: true,
                            translation_x: 5.0,
                            ..Default::default()
                        })
                        .background(cranpose_ui::Color(0.2, 0.4, 0.8, 1.0)),
                    cranpose_ui::BoxSpec::default(),
                    move || {
                        Text(text, Modifier::empty(), TextStyle::default());
                    },
                );
                *card_id.borrow_mut() = Some(id);
            },
        );
    })
}

fn wrapper_and_card(root: &LayerNode, card_id: NodeId) -> (&LayerNode, &LayerNode) {
    let wrapper = root
        .children
        .iter()
        .find_map(|child| match child {
            RenderNode::Layer(layer) if layer.wraps == Some(card_id) => Some(layer.as_ref()),
            _ => None,
        })
        .expect("the card's outer shadow wraps its clipped layer");
    let card = find_layer_by_node_id(wrapper, card_id).expect("card layer inside the wrapper");
    (wrapper, card)
}

#[test]
fn a_draw_before_the_graphics_layer_wraps_the_clipped_layer_in_the_parents_space() {
    let label = Rc::new(RefCell::new(None));
    let card_id = Rc::new(RefCell::new(None));
    let mut composition = wrapped_card_composition(label, card_id.clone());
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(
            root,
            Size {
                width: 240.0,
                height: 120.0,
            },
        )
        .expect("layout");
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("graph");
    applier.clear_runtime_handle();
    let card_id = card_id.borrow().expect("card id");
    let (wrapper, card) = wrapper_and_card(&graph.root, card_id);

    assert_eq!(wrapper.node_id, None);
    assert!(!wrapper.graphics_layer.clip);
    assert_eq!(
        wrapper.transform_to_parent.map_point(Point::default()),
        Point { x: 20.0, y: 30.0 },
        "the wrapper carries the layout placement alone"
    );
    assert_eq!(
        card.transform_to_parent.map_point(Point::default()),
        Point { x: 5.0, y: 0.0 },
        "the clipped layer keeps only its own graphics-layer transform"
    );
    assert!(card.graphics_layer.clip);
    let [shadow, RenderNode::Layer(_)] = wrapper.children.as_slice() else {
        panic!(
            "wrapper children must be the outer shadow then the card, got {} children",
            wrapper.children.len()
        );
    };
    let shadow_is_outer = match shadow {
        RenderNode::DrawRun(run) => run.summary.has_shadow && !run.summary.has_non_shadow,
        RenderNode::Primitive(entry) => matches!(
            &entry.node,
            PrimitiveNode::Draw(draw) if matches!(draw.primitive, DrawPrimitive::Shadow(_))
        ),
        RenderNode::Layer(_) => false,
    };
    assert!(shadow_is_outer, "the outer draw is the shadow alone");
    assert!(
        card.children.iter().any(|child| match child {
            RenderNode::DrawRun(run) => run.summary.has_non_shadow,
            RenderNode::Primitive(entry) => matches!(
                &entry.node,
                PrimitiveNode::Draw(draw)
                    if !matches!(draw.primitive, DrawPrimitive::Shadow(_))
            ),
            RenderNode::Layer(_) => false,
        }),
        "the background chained after the layer stays inside it"
    );
}

#[test]
fn a_dirty_wrapped_node_is_rebuilt_as_one_wrapper() {
    let label = Rc::new(RefCell::new(None));
    let card_id = Rc::new(RefCell::new(None));
    let mut composition = wrapped_card_composition(label.clone(), card_id.clone());
    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 120.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, viewport).expect("layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("graph");
    applier.clear_runtime_handle();
    drop(applier);
    let card_id = card_id.borrow().expect("card id");

    let text = label.borrow().as_ref().copied().expect("label state");
    text.set_value("after".to_string());
    composition
        .process_invalid_scopes()
        .expect("text recomposition");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, viewport).expect("layout");
    assert!(update_graph_from_applier(
        &mut applier,
        &mut graph,
        &[card_id],
        1.0
    ));
    applier.clear_runtime_handle();

    let (wrapper, card) = wrapper_and_card(&graph.root, card_id);
    assert_eq!(
        wrapper.transform_to_parent.map_point(Point::default()),
        Point { x: 20.0, y: 30.0 }
    );
    assert!(
        !card.children.iter().any(|child| matches!(
            child,
            RenderNode::Layer(layer) if layer.wraps.is_some()
        )),
        "rebuilding the wrapped node must replace its wrapper, not nest a second one"
    );
    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);
    assert_eq!(labels, vec!["after".to_string()]);
}

#[test]
fn update_graph_from_applier_replaces_dirty_child_layer() {
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
        Rc::new(RefCell::new(None));
    let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = state_holder.clone();
    let child_id_holder_for_comp = child_id_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let label = cranpose_core::rememberMutableStateOf(|| "before".to_string());
        *state_holder_for_comp.borrow_mut() = Some(label);
        let child_id_holder_for_content = child_id_holder_for_comp.clone();
        cranpose_ui::Box(
            Modifier::empty().size_points(240.0, 80.0),
            cranpose_ui::BoxSpec::default(),
            move || {
                let child_id = Text(label, Modifier::empty(), TextStyle::default());
                *child_id_holder_for_content.borrow_mut() = Some(child_id);
                Text("stable", Modifier::empty(), TextStyle::default());
            },
        );
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 80.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
    let child_id = child_id_holder
        .borrow()
        .expect("text child id should be captured");
    let initial_transform = find_layer_by_node_id(&graph.root, child_id)
        .expect("text child layer")
        .transform_to_parent;
    applier.clear_runtime_handle();
    drop(applier);

    let label = state_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("label state should be captured");
    label.set_value("after".to_string());
    composition
        .process_invalid_scopes()
        .expect("text recomposition");

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("updated layout");
    let child_id = child_id_holder
        .borrow()
        .expect("text child id should remain captured");

    assert!(
        update_graph_from_applier(&mut applier, &mut graph, &[child_id], 1.0),
        "dirty child should be replaceable from retained applier state"
    );
    applier.clear_runtime_handle();

    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);
    assert!(
        labels.iter().any(|label| label == "after"),
        "updated graph should contain refreshed child text, got {labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label == "before"),
        "updated graph should not retain stale child text, got {labels:?}"
    );
    assert!(
        labels.iter().any(|label| label == "stable"),
        "sibling content should remain present, got {labels:?}"
    );
    assert_eq!(
        find_layer_by_node_id(&graph.root, child_id)
            .expect("updated text child layer")
            .transform_to_parent,
        initial_transform,
        "draw-only child replacement must preserve the retained parent placement transform"
    );
}

fn assert_same_cache_hash_state(dirty_road: &LayerNode, full_road: &LayerNode, path: &str) {
    assert_eq!(
        dirty_road.node_id, full_road.node_id,
        "tree shape must match at {path}"
    );
    assert_eq!(
        dirty_road.cache_hashes_valid, full_road.cache_hashes_valid,
        "hash validity at {path} (node {:?})",
        dirty_road.node_id
    );
    if full_road.cache_hashes_valid {
        assert_eq!(
            dirty_road.cache_hashes, full_road.cache_hashes,
            "stored hashes at {path} (node {:?})",
            dirty_road.node_id
        );
    }
    assert_eq!(
        dirty_road.target_content_hash(),
        full_road.target_content_hash(),
        "target content hash at {path} (node {:?})",
        dirty_road.node_id
    );
    assert_eq!(
        dirty_road.children.len(),
        full_road.children.len(),
        "child count at {path}"
    );
    for (index, (dirty_child, full_child)) in dirty_road
        .children
        .iter()
        .zip(full_road.children.iter())
        .enumerate()
    {
        if let (RenderNode::Layer(dirty_child), RenderNode::Layer(full_child)) =
            (dirty_child, full_child)
        {
            assert_same_cache_hash_state(dirty_child, full_child, &format!("{path}/{index}"));
        }
    }
}

fn assert_dirty_hash_road_matches_full_walk(graph: &RenderGraph) {
    let mut full_road = graph.root.clone();
    full_road.recompute_raster_cache_hashes();
    assert_same_cache_hash_state(&graph.root, &full_road, "root");
}

#[test]
fn dirty_update_leaves_the_hashes_a_full_walk_leaves() {
    let label_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
        Rc::new(RefCell::new(None));
    let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let label_holder_for_comp = label_holder.clone();
    let child_id_holder_for_comp = child_id_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let label = cranpose_core::rememberMutableStateOf(|| "before".to_string());
        *label_holder_for_comp.borrow_mut() = Some(label);
        let child_id_holder_for_content = child_id_holder_for_comp.clone();
        Column(
            Modifier::empty().size_points(240.0, 200.0),
            ColumnSpec::default(),
            move || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(240.0, 80.0)
                        .graphics_layer(|| GraphicsLayer {
                            alpha: 0.5,
                            ..GraphicsLayer::default()
                        }),
                    cranpose_ui::BoxSpec::default(),
                    {
                        let child_id_holder_for_box = child_id_holder_for_content.clone();
                        move || {
                            let child_id = Text(label, Modifier::empty(), TextStyle::default());
                            *child_id_holder_for_box.borrow_mut() = Some(child_id);
                        }
                    },
                );
                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(240.0, 80.0)
                        .graphics_layer(|| GraphicsLayer {
                            alpha: 0.75,
                            ..GraphicsLayer::default()
                        }),
                    cranpose_ui::BoxSpec::default(),
                    || {
                        Text("stable", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 200.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
    graph.root.recompute_raster_cache_hashes();
    applier.clear_runtime_handle();
    drop(applier);
    assert_dirty_hash_road_matches_full_walk(&graph);

    let label = label_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("label state should be captured");
    label.set_value("after".to_string());
    composition
        .process_invalid_scopes()
        .expect("text recomposition");

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("updated layout");
    let child_id = child_id_holder
        .borrow()
        .expect("text child id should be captured");
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &[child_id], 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "dirty child update should apply in place, got {:?}",
        report.update
    );
    assert_dirty_hash_road_matches_full_walk(&graph);
}

#[test]
fn dirty_update_with_a_new_row_leaves_the_hashes_a_full_walk_leaves() {
    let rows_holder: Rc<RefCell<Option<cranpose_core::MutableState<usize>>>> =
        Rc::new(RefCell::new(None));
    let column_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let rows_holder_for_comp = rows_holder.clone();
    let column_id_holder_for_comp = column_id_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let rows = cranpose_core::rememberMutableStateOf(|| 2usize);
        *rows_holder_for_comp.borrow_mut() = Some(rows);
        let column_id_holder_for_content = column_id_holder_for_comp.clone();
        cranpose_ui::Box(
            Modifier::empty()
                .size_points(240.0, 240.0)
                .graphics_layer(|| GraphicsLayer {
                    alpha: 0.5,
                    ..GraphicsLayer::default()
                }),
            cranpose_ui::BoxSpec::default(),
            move || {
                let column_id = Column(
                    Modifier::empty().size_points(240.0, 240.0),
                    ColumnSpec::default(),
                    move || {
                        for index in 0..rows.get() {
                            Text(
                                format!("row {index}"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        }
                    },
                );
                *column_id_holder_for_content.borrow_mut() = Some(column_id);
            },
        );
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 240.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
    graph.root.recompute_raster_cache_hashes();
    applier.clear_runtime_handle();
    drop(applier);

    let rows = rows_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("row count state should be captured");
    rows.set_value(3);
    composition
        .process_invalid_scopes()
        .expect("row recomposition");

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("updated layout");
    let column_id = column_id_holder.borrow().expect("column id");
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &[column_id], 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "structural update should apply in place, got {:?}",
        report.update
    );
    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);
    assert!(
        labels.iter().any(|label| label == "row 2"),
        "the new row must be in the patched graph, got {labels:?}"
    );
    assert_dirty_hash_road_matches_full_walk(&graph);
}

#[test]
fn scene_build_publishes_live_translated_window_rect_without_layout_tree() {
    use std::cell::Cell;

    use cranpose_ui::{Box, BoxSpec, MeasureLayoutOptions, measure_layout_with_options};

    let spacer_before = 120.0_f32;
    let sink: Rc<Cell<Rect>> = Rc::new(Cell::new(Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    }));
    let sink_for_comp = sink.clone();
    let translation = Rc::new(Cell::new(Point { x: 7.0, y: 11.0 }));
    let translation_for_comp = translation.clone();
    let mut composition = cranpose_ui::run_test_composition(move || {
        let sink = sink_for_comp.clone();
        let translation = translation_for_comp.clone();
        Column(
            Modifier::empty()
                .size_points(200.0, 400.0)
                .graphics_layer(move || GraphicsLayer {
                    translation_x: translation.get().x,
                    translation_y: translation.get().y,
                    ..Default::default()
                }),
            ColumnSpec::default(),
            move || {
                Spacer(Size {
                    width: 200.0,
                    height: spacer_before,
                });
                Box(
                    Modifier::empty()
                        .size_points(200.0, 50.0)
                        .graphics_layer(|| GraphicsLayer {
                            translation_x: 5.0,
                            translation_y: -9.0,
                            ..Default::default()
                        })
                        .report_window_rect(sink.clone()),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 200.0,
        height: 400.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    measure_layout_with_options(
        &mut applier,
        root,
        viewport,
        MeasureLayoutOptions {
            collect_semantics: false,
            build_layout_tree: false,
        },
    )
    .expect("layout");
    assert_eq!(
        sink.get().height,
        0.0,
        "sink must start empty (place disabled)"
    );

    for offset in [Point { x: 7.0, y: 11.0 }, Point { x: -3.0, y: 5.0 }] {
        translation.set(offset);
        let _graph = build_graph_from_applier(&mut applier, root, 1.0).expect("scene graph");
        assert_eq!(
            sink.get(),
            Rect {
                x: offset.x + 5.0,
                y: spacer_before + offset.y - 9.0,
                width: 200.0,
                height: 50.0,
            },
            "window rect must follow both live layer translations without relayout"
        );
    }
    applier.clear_runtime_handle();
}

#[test]
fn update_graph_from_applier_reports_failed_dirty_child_rebuild() {
    let mut graph = RenderGraph {
        root: build_layer_node_for_test(snapshot_with_translation(0.0), 1.0, false),
    };
    let mut applier = MemoryApplier::new();

    let report = update_graph_from_applier_report(&mut applier, &mut graph, &[2], 1.0);

    assert_eq!(
        report,
        GraphUpdateReport {
            update: GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable),
            hit_graph_dirty: true,
        },
        "dirty child graph updates must not report success when the replacement cannot be rebuilt"
    );
}

#[test]
fn scrolled_list_under_a_composited_layer_keeps_the_hashes_a_full_walk_leaves() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = scroll_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        cranpose_ui::Box(
            Modifier::empty()
                .size_points(240.0, 320.0)
                .graphics_layer(|| GraphicsLayer {
                    alpha: 0.6,
                    ..GraphicsLayer::default()
                }),
            cranpose_ui::BoxSpec::default(),
            move || scrolling_rows_column(scroll_state, composited_row),
        );
    });

    let (root, mut graph) = initial_scrolled_graph(&mut composition);
    let viewport = scroll_viewport();

    let scroll_state = scroll_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("scroll state should be captured");
    assert!(
        scroll_state.dispatch_raw_delta(96.0) > 0.0,
        "test scroll must be consumed"
    );
    let dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    assert!(
        !dirty_nodes.is_empty(),
        "a scroll must schedule a scoped scene update"
    );

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "scroll update should apply in place, got {:?}",
        report.update
    );
    assert_dirty_hash_road_matches_full_walk(&graph);
}

#[test]
fn update_graph_from_applier_refreshes_scroll_content_offset() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = scroll_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        Column(
            Modifier::empty()
                .size_points(240.0, 120.0)
                .vertical_scroll(scroll_state, false),
            ColumnSpec::default(),
            || {
                Text("scroll top", Modifier::empty(), TextStyle::default());
                Spacer(Size {
                    width: 0.0,
                    height: 160.0,
                });
                Text("scroll target", Modifier::empty(), TextStyle::default());
            },
        );
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 120.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial scroll layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
    graph.root.recompute_raster_cache_hashes();
    let initial_target_top =
        find_text_top(&graph.root, "scroll target").expect("initial target text");
    applier.clear_runtime_handle();
    drop(applier);

    let scroll_state = scroll_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("scroll state should be captured");
    let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
    assert!(consumed_scroll > 0.0, "test scroll must be consumed");
    let dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    assert!(
        !dirty_nodes.is_empty(),
        "scroll state invalidation must schedule scoped layout graph update"
    );

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "scroll graph update should apply in place, got {:?}",
        report.update
    );
    let updated_target_top =
        find_text_top(&graph.root, "scroll target").expect("updated target text");
    assert!(
        updated_target_top < initial_target_top - consumed_scroll * 0.75,
        "partial graph update must refresh scroll content offset: initial_y={initial_target_top} updated_y={updated_target_top} dirty_nodes={dirty_nodes:?}"
    );
    assert_dirty_hash_road_matches_full_walk(&graph);
}

#[test]
fn an_overmarked_ancestor_chain_still_translates_instead_of_relowering() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = scroll_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        cranpose_ui::Box(
            Modifier::empty().size_points(240.0, 320.0),
            cranpose_ui::BoxSpec::default(),
            move || scrolling_rows_column(scroll_state, tinted_row),
        );
    });

    let (root, mut graph) = initial_scrolled_graph(&mut composition);
    let viewport = scroll_viewport();
    let initial_row_top = find_text_top(&graph.root, "row 3").expect("initial row text");

    let scroll_state = scroll_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("scroll state should be captured");
    let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
    assert!(consumed_scroll > 0.0, "test scroll must be consumed");
    let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    dirty_nodes.push(graph.root.node_id.expect("root id"));
    let mut cursor = &graph.root;
    while let Some(RenderNode::Layer(child)) = cursor
        .children
        .iter()
        .find(|child| matches!(child, RenderNode::Layer(_)))
    {
        let chain = child.node_id.expect("chain node id");
        if dirty_nodes.contains(&chain) {
            break;
        }
        dirty_nodes.push(chain);
        cursor = child;
    }

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    reset_lowered_layer_count();
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "chain update should apply in place, got {:?}",
        report.update
    );
    let lowered = lowered_layer_count();
    assert_eq!(
        lowered, 0,
        "an over-marked ancestor chain over a pure scroll must still \
         translate; {lowered} layers were rebuilt (dirty={dirty_nodes:?})"
    );
    let updated_row_top = find_text_top(&graph.root, "row 3").expect("updated row text");
    assert!(
        updated_row_top < initial_row_top - consumed_scroll * 0.75,
        "the translation must land through the chain: initial_y={initial_row_top} updated_y={updated_row_top}"
    );
    assert_dirty_hash_road_matches_full_walk(&graph);
}

#[test]
fn a_scrolled_container_translates_clean_children_instead_of_relowering() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = scroll_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        scrolling_rows_column(scroll_state, tinted_row);
    });

    let (root, mut graph) = initial_scrolled_graph(&mut composition);
    let viewport = scroll_viewport();
    let initial_row_tops: Vec<_> = (0..12)
        .map(|index| find_text_top(&graph.root, &format!("row {index}")).expect("initial row text"))
        .collect();

    let scroll_state = scroll_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("scroll state should be captured");
    let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
    assert!(consumed_scroll > 0.0, "test scroll must be consumed");
    let dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    assert!(
        !dirty_nodes.is_empty(),
        "a scroll must schedule a scoped scene update"
    );

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    reset_lowered_layer_count();
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "scroll update should apply in place, got {:?}",
        report.update
    );
    let lowered = lowered_layer_count();
    assert_eq!(
        lowered, 0,
        "a pure scroll of clean children must translate the retained \
         subtrees, not re-lower them; {lowered} layers were rebuilt"
    );
    for (index, initial_row_top) in initial_row_tops.into_iter().enumerate() {
        let updated_row_top = find_text_top(&graph.root, &format!("row {index}"))
            .expect("every retained row survives scrolling");
        assert!(
            (updated_row_top - (initial_row_top - consumed_scroll)).abs() < 0.01,
            "row {index} did not follow the scroll: initial_y={initial_row_top} updated_y={updated_row_top}"
        );
    }
    assert_dirty_hash_road_matches_full_walk(&graph);
}

#[test]
fn a_scrolled_container_refreshes_dirty_shadowed_rows_without_relowering_children() {
    for wrapped_root in [false, true] {
        assert_shadowed_scroll_reuses_children(wrapped_root);
    }
}

fn shadowed_scroll_rows(
    generation: usize,
    shadow_radius: &Rc<std::cell::Cell<f32>>,
    first_text: &Rc<std::cell::Cell<Option<NodeId>>>,
    clicks: &Rc<RefCell<Vec<(usize, usize)>>>,
) {
    for index in 0..12usize {
        let shape =
            cranpose_ui::LayerShape::Rounded(cranpose_ui::RoundedCornerShape::uniform(12.0));
        let radius = shadow_radius.clone();
        let recorded_clicks = clicks.clone();
        let first_text = first_text.clone();
        cranpose_ui::Box(
            Modifier::empty()
                .size_points(240.0, 60.0)
                .clickable(move |_| recorded_clicks.borrow_mut().push((index, generation)))
                .drop_shadow(shape, move |scope| scope.radius = radius.get())
                .graphics_layer(move || GraphicsLayer {
                    shape,
                    clip: true,
                    ..Default::default()
                }),
            cranpose_ui::BoxSpec::default(),
            move || {
                let label = if index == 0 && generation > 0 {
                    "changed row".to_owned()
                } else {
                    format!("row {index}")
                };
                let id = Text(label, Modifier::empty(), TextStyle::default());
                if index == 0 {
                    first_text.set(Some(id));
                }
            },
        );
    }
}

fn assert_shadowed_scroll_reuses_children(wrapped_root: bool) {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = scroll_holder.clone();
    let shadow_radius = Rc::new(std::cell::Cell::new(6.0));
    let shadow_radius_for_comp = shadow_radius.clone();
    let generation_holder = Rc::new(RefCell::new(None));
    let generation_for_comp = generation_holder.clone();
    let first_text = Rc::new(std::cell::Cell::new(None));
    let first_text_for_comp = first_text.clone();
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let clicks_for_comp = clicks.clone();
    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        let generation = cranpose_core::rememberMutableStateOf(|| 0usize);
        *generation_for_comp.borrow_mut() = Some(generation);
        let mut modifier = Modifier::empty().size_points(240.0, 320.0);
        if wrapped_root {
            modifier = modifier
                .drop_shadow(cranpose_ui::LayerShape::Rectangle, |scope| {
                    scope.radius = 4.0;
                })
                .graphics_layer(GraphicsLayer::default);
        }
        Column(
            modifier.vertical_scroll(scroll_state, false),
            ColumnSpec::default(),
            {
                let shadow_radius = shadow_radius_for_comp.clone();
                let first_text = first_text_for_comp.clone();
                let clicks = clicks_for_comp.clone();
                move || {
                    shadowed_scroll_rows(generation.value(), &shadow_radius, &first_text, &clicks);
                }
            },
        );
    });

    let (root, mut graph) = initial_scrolled_graph(&mut composition);
    let viewport = scroll_viewport();
    let initial_row_top = find_text_top(&graph.root, "row 3").expect("row text");

    let scroll_state = scroll_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("scroll state should be captured");
    let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
    shadow_radius.set(11.0);
    let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    let row_ids: Vec<_> = find_layer_by_node_id(&graph.root, root)
        .expect("scroll layer")
        .children
        .iter()
        .filter_map(|child| match child {
            RenderNode::Layer(layer) => layer.wraps,
            _ => None,
        })
        .collect();
    dirty_nodes.extend(row_ids.iter().copied());
    dirty_nodes.push(root);
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    reset_lowered_layer_count();
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);

    assert!(report.applied(), "got {:?}", report.update);
    assert_eq!(
        lowered_layer_count(),
        0,
        "rows wrapped in their outer shadow must translate like plain rows"
    );
    let updated_row_top = find_text_top(&graph.root, "row 3").expect("row text");
    assert!(updated_row_top < initial_row_top - consumed_scroll * 0.75);
    let wrappers = find_layer_by_node_id(&graph.root, root)
        .expect("scroll layer")
        .children
        .iter()
        .filter(|child| matches!(child, RenderNode::Layer(layer) if layer.wraps.is_some()))
        .count();
    assert_eq!(wrappers, 12, "every row keeps exactly one wrapper");
    assert_dirty_hash_road_matches_full_walk(&graph);
    let fresh = build_graph_from_applier(&mut applier, root, 1.0).expect("fresh graph");
    applier.clear_runtime_handle();
    assert_eq!(
        crate::graph_hash::layer_raster_cache_hashes(&graph.root),
        crate::graph_hash::layer_raster_cache_hashes(&fresh.root),
        "changed outer shadows and retained child positions must match a fresh build"
    );

    drop(applier);
    generation_holder
        .borrow()
        .expect("generation state")
        .set_value(1);
    composition
        .process_invalid_scopes()
        .expect("changed row composition");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("changed row layout");
    let mut dirty = row_ids.clone();
    dirty.push(first_text.get().expect("first text node"));
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty, 1.0);
    assert!(report.applied());
    assert!(find_text_top(&graph.root, "changed row").is_some());
    assert!(find_text_top(&graph.root, "row 0").is_none());
    for row in row_ids {
        let layer = find_layer_by_node_id(&graph.root, row).expect("clickable row");
        let hit = layer.hit_test.as_ref().expect("row hit target");
        assert_eq!(hit.pointer_inputs.len(), 1);
        for kind in [
            cranpose_foundation::PointerEventKind::Down,
            cranpose_foundation::PointerEventKind::Up,
        ] {
            let position = Point { x: 10.0, y: 10.0 };
            hit.pointer_inputs[0](
                cranpose_foundation::PointerEvent::new(kind, position, position).with_buttons(
                    cranpose_foundation::PointerButtons::new()
                        .with(cranpose_foundation::PointerButton::Primary),
                ),
            );
        }
    }
    assert_eq!(
        *clicks.borrow(),
        (0..12).map(|index| (index, 1)).collect::<Vec<_>>()
    );
    let fresh = build_graph_from_applier(&mut applier, root, 1.0).expect("fresh changed graph");
    applier.clear_runtime_handle();
    assert_eq!(
        collect_text_tops(&graph.root),
        collect_text_tops(&fresh.root)
    );
    assert_dirty_hash_road_matches_full_walk(&graph);
}

#[test]
fn a_sliding_lazy_window_lowers_only_the_entering_rows() {
    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = state_holder.clone();
    let mut composition = cranpose_ui::run_test_composition(move || {
        let list_state = rememberLazyListState();
        *state_holder_for_comp.borrow_mut() = Some(list_state);
        LazyColumn(
            Modifier::empty().size_points(240.0, 320.0),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(60, |index| {
                    cranpose_ui::Box(
                        Modifier::empty()
                            .size_points(240.0, 60.0)
                            .background(Color(0.9, 0.9, 0.92, 1.0)),
                        cranpose_ui::BoxSpec::default(),
                        move || {
                            Text(
                                format!("row {index}"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                });
            },
        );
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 320.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial lazy layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
    graph.root.recompute_raster_cache_hashes();
    let _ = applier.take_structural_change_parents_attached_to(root);
    let initial_row_top = find_text_top(&graph.root, "row 4").expect("initial row text");
    applier.clear_runtime_handle();
    drop(applier);

    let list_state = (*state_holder.borrow()).expect("list state should be captured");
    let consumed = list_state.dispatch_scroll_delta(-96.0);
    assert!(consumed != 0.0, "the lazy scroll must consume the delta");
    let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    dirty_nodes.extend(cranpose_ui::pending_measure_repass_nodes_snapshot());

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled lazy layout");
    dirty_nodes.extend(applier.take_structural_change_parents_attached_to(root));
    dirty_nodes.sort_unstable();
    dirty_nodes.dedup();
    assert!(
        !dirty_nodes.is_empty(),
        "a lazy scroll must mark the list dirty"
    );

    reset_lowered_layer_count();
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "the boundary frame must apply in place, got {:?}",
        report.update
    );
    let lowered = lowered_layer_count();
    assert!(
        lowered > 0,
        "rows crossed the window boundary; the entering subtrees must lower"
    );
    assert!(
        lowered <= 8,
        "only the entering rows may lower on a boundary frame; \
         {lowered} layers were rebuilt (dirty={dirty_nodes:?})"
    );
    let updated_row_top = find_text_top(&graph.root, "row 4").expect("updated row text");
    assert!(
        updated_row_top < initial_row_top - 60.0,
        "the retained rows must move with the scroll: \
         initial_y={initial_row_top} updated_y={updated_row_top}"
    );
    assert_dirty_hash_road_matches_full_walk(&graph);
}

fn collect_text_tops(layer: &LayerNode) -> std::collections::BTreeMap<String, i64> {
    fn walk(
        layer: &LayerNode,
        transform: ProjectiveTransform,
        out: &mut std::collections::BTreeMap<String, i64>,
    ) {
        for child in &layer.children {
            match child {
                RenderNode::Primitive(primitive) => {
                    if let PrimitiveNode::Text(text) = &primitive.node {
                        let quad = transform.map_rect(text.rect);
                        let top = quad
                            .iter()
                            .map(|point| point[1])
                            .fold(f32::INFINITY, f32::min);
                        if top.is_finite() {
                            out.insert(text.text.text.clone(), (top * 10.0).round() as i64);
                        }
                    }
                }
                RenderNode::Layer(child_layer) => {
                    let child_transform = child_layer.transform_to_parent.then(transform);
                    walk(child_layer, child_transform, out);
                }
                RenderNode::DrawRun(_) => {}
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(layer, ProjectiveTransform::identity(), &mut out);
    out
}

#[test]
fn a_lazy_jump_of_any_distance_patches_to_what_a_fresh_build_shows() {
    for delta in [-30.0f32, -60.0, -96.0, -180.0, -300.0] {
        let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let state_holder_for_comp = state_holder.clone();
        let mut composition = cranpose_ui::run_test_composition(move || {
            let list_state = rememberLazyListState();
            *state_holder_for_comp.borrow_mut() = Some(list_state);
            LazyColumn(
                Modifier::empty()
                    .size_points(240.0, 320.0)
                    .graphics_layer(|| GraphicsLayer {
                        translation_x: 7.0,
                        translation_y: 11.0,
                        ..Default::default()
                    }),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(60, |index| {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .size_points(240.0, 60.0)
                                .background(Color(0.9, 0.9, 0.92, 1.0)),
                            cranpose_ui::BoxSpec::default(),
                            move || {
                                Text(
                                    format!("row {index}"),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    });
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 320.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial lazy layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        let _ = applier.take_structural_change_parents_attached_to(root);
        applier.clear_runtime_handle();
        drop(applier);

        let list_state = (*state_holder.borrow()).expect("list state should be captured");
        let consumed = list_state.dispatch_scroll_delta(delta);
        assert!(consumed != 0.0, "delta {delta}: the scroll must consume");
        let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
        dirty_nodes.extend(cranpose_ui::pending_measure_repass_nodes_snapshot());

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled lazy layout");
        dirty_nodes.extend(applier.take_structural_change_parents_attached_to(root));
        dirty_nodes.sort_unstable();
        dirty_nodes.dedup();
        reset_lowered_layer_count();
        let report = update_graph_from_applier_report(&mut applier, &mut graph, &dirty_nodes, 1.0);
        assert!(report.applied(), "delta {delta}: boundary frame must apply");
        assert_eq!(
            find_layer_by_node_id(&graph.root, root)
                .expect("list layer")
                .scene_children_layer_translation,
            Point { x: 7.0, y: 11.0 },
            "patched child origins must retain the container's layer translation"
        );
        if delta == -30.0 {
            assert_eq!(
                lowered_layer_count(),
                0,
                "a buffer-absorbed scroll must not lower any layer"
            );
        }

        let fresh =
            build_graph_from_applier(&mut applier, root, 1.0).expect("fresh comparison graph");
        applier.clear_runtime_handle();

        let patched_texts = collect_text_tops(&graph.root);
        let fresh_texts = collect_text_tops(&fresh.root);
        assert_eq!(
            patched_texts, fresh_texts,
            "delta {delta}: the patched scene must show exactly what a \
             fresh build shows (dirty={dirty_nodes:?})"
        );
    }
}

#[test]
fn update_graph_from_applier_keeps_parent_content_offset_for_dirty_scroll_child() {
    let label_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
        Rc::new(RefCell::new(None));
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let label_holder_for_comp = label_holder.clone();
    let scroll_holder_for_comp = scroll_holder.clone();
    let child_id_holder_for_comp = child_id_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let label = cranpose_core::rememberMutableStateOf(|| "scrolled child before".to_string());
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *label_holder_for_comp.borrow_mut() = Some(label);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        let child_id_holder_for_content = child_id_holder_for_comp.clone();
        Column(
            Modifier::empty()
                .size_points(260.0, 90.0)
                .vertical_scroll(scroll_state, false),
            ColumnSpec::default(),
            move || {
                Spacer(Size {
                    width: 0.0,
                    height: 24.0,
                });
                let child_id = Text(label, Modifier::empty(), TextStyle::default());
                *child_id_holder_for_content.borrow_mut() = Some(child_id);
                Spacer(Size {
                    width: 0.0,
                    height: 220.0,
                });
            },
        );
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 260.0,
        height: 90.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial layout");
    applier.clear_runtime_handle();
    drop(applier);

    let scroll_state = scroll_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("scroll state should be captured");
    assert!(scroll_state.dispatch_raw_delta(36.0) > 0.0);

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("scrolled graph");
    let child_id = child_id_holder
        .borrow()
        .expect("text child id should be captured");
    let scrolled_transform = find_layer_by_node_id(&graph.root, child_id)
        .expect("scrolled child layer")
        .transform_to_parent;
    applier.clear_runtime_handle();
    drop(applier);

    let label = label_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("label state should be captured");
    label.set_value("scrolled child after".to_string());
    composition
        .process_invalid_scopes()
        .expect("text recomposition");

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("updated scrolled layout");
    let child_id = child_id_holder
        .borrow()
        .expect("text child id should remain captured");
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &[child_id], 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "dirty child graph update should apply, got {:?}",
        report.update
    );
    let updated = find_layer_by_node_id(&graph.root, child_id).expect("updated child layer");
    assert_eq!(
        updated.transform_to_parent, scrolled_transform,
        "dirty child replacement inside a scrolled parent must keep the parent's content-offset transform"
    );
    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);
    assert!(
        labels.iter().any(|label| label == "scrolled child after"),
        "updated graph should contain refreshed text, got {labels:?}"
    );
}

#[test]
fn dirty_scrolled_overlay_graphics_layer_stays_aligned_with_underlay() {
    let alpha_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let underlay_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let overlay_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let alpha_holder_for_comp = alpha_holder.clone();
    let scroll_holder_for_comp = scroll_holder.clone();
    let underlay_id_holder_for_comp = underlay_id_holder.clone();
    let overlay_id_holder_for_comp = overlay_id_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let alpha = cranpose_core::rememberMutableStateOf(|| 1.0f32);
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *alpha_holder_for_comp.borrow_mut() = Some(alpha);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        let underlay_id_holder_for_content = underlay_id_holder_for_comp.clone();
        let overlay_id_holder_for_content = overlay_id_holder_for_comp.clone();
        Column(
            Modifier::empty()
                .size_points(260.0, 120.0)
                .vertical_scroll(scroll_state, false),
            ColumnSpec::default(),
            move || {
                Spacer(Size {
                    width: 0.0,
                    height: 180.0,
                });
                cranpose_ui::Box(
                    Modifier::empty().size_points(188.0, 88.0),
                    cranpose_ui::BoxSpec::default(),
                    {
                        let underlay_id_holder_for_box = underlay_id_holder_for_content.clone();
                        let overlay_id_holder_for_box = overlay_id_holder_for_content.clone();
                        move || {
                            let underlay_id = cranpose_ui::Box(
                                Modifier::empty().size_points(188.0, 88.0),
                                cranpose_ui::BoxSpec::default(),
                                || {
                                    Text(
                                        "UNDERLAY CONTENT",
                                        Modifier::empty().absolute_offset(12.0, 8.0),
                                        TextStyle::default(),
                                    );
                                },
                            );
                            *underlay_id_holder_for_box.borrow_mut() = Some(underlay_id);
                            let overlay_id = cranpose_ui::Box(
                                Modifier::empty().size_points(188.0, 88.0).graphics_layer(
                                    move || GraphicsLayer {
                                        alpha: alpha.get(),
                                        ..GraphicsLayer::default()
                                    },
                                ),
                                cranpose_ui::BoxSpec::default(),
                                || {
                                    Text(
                                        "TOP LAYER",
                                        Modifier::empty().absolute_offset(74.0, 39.6),
                                        TextStyle::default(),
                                    );
                                },
                            );
                            *overlay_id_holder_for_box.borrow_mut() = Some(overlay_id);
                        }
                    },
                );
                Spacer(Size {
                    width: 0.0,
                    height: 280.0,
                });
            },
        );
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 260.0,
        height: 120.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial layout");
    applier.clear_runtime_handle();
    drop(applier);

    let scroll_state = scroll_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("scroll state should be captured");
    assert!(scroll_state.dispatch_raw_delta(96.0) > 0.0);

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("scrolled graph");
    applier.clear_runtime_handle();
    drop(applier);

    let underlay_id = underlay_id_holder
        .borrow()
        .expect("underlay id should be captured");
    let overlay_id = overlay_id_holder
        .borrow()
        .expect("overlay id should be captured");
    let scrolled_underlay_origin =
        find_layer_origin(&graph.root, underlay_id).expect("underlay origin");
    let scrolled_overlay_origin =
        find_layer_origin(&graph.root, overlay_id).expect("overlay origin");
    assert_eq!(scrolled_underlay_origin, scrolled_overlay_origin);

    let alpha = alpha_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("alpha state should be captured");
    alpha.set_value(0.35);

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &[overlay_id], 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "dirty overlay graph update should apply, got {:?}",
        report.update
    );
    let updated_underlay_origin =
        find_layer_origin(&graph.root, underlay_id).expect("updated underlay origin");
    let updated_overlay_origin =
        find_layer_origin(&graph.root, overlay_id).expect("updated overlay origin");
    assert_eq!(
        updated_underlay_origin, scrolled_underlay_origin,
        "stable underlay must keep its scrolled origin"
    );
    assert_eq!(
        updated_overlay_origin, updated_underlay_origin,
        "dirty overlay graphics layer must stay aligned with its stable underlay"
    );
}

#[test]
fn update_graph_from_applier_refreshes_dirty_graphics_layer_transform() {
    let offset_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let node_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let offset_holder_for_comp = offset_holder.clone();
    let node_id_holder_for_comp = node_id_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let offset = cranpose_core::rememberMutableStateOf(|| 0.0f32);
        *offset_holder_for_comp.borrow_mut() = Some(offset);
        let node_id = cranpose_ui::Box(
            Modifier::empty()
                .size_points(40.0, 20.0)
                .graphics_layer(move || GraphicsLayer {
                    translation_x: offset.get(),
                    ..GraphicsLayer::default()
                }),
            cranpose_ui::BoxSpec::default(),
            || {},
        );
        *node_id_holder_for_comp.borrow_mut() = Some(node_id);
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 120.0,
        height: 80.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
    let node_id = node_id_holder
        .borrow()
        .expect("graphics layer node id should be captured");
    let initial_origin = find_layer_by_node_id(&graph.root, node_id)
        .expect("initial graphics layer")
        .transform_to_parent
        .map_point(Point::default());
    applier.clear_runtime_handle();
    drop(applier);

    let offset = offset_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("offset state should be captured");
    offset.set_value(32.0);

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &[node_id], 1.0);
    assert!(
        report.applied(),
        "dirty graphics layer should be replaceable from retained applier state, got {:?}",
        report.update
    );
    assert!(
        !report.hit_graph_dirty,
        "a moved visual-only layer should not force hit graph refresh"
    );
    applier.clear_runtime_handle();

    let updated_origin = find_layer_by_node_id(&graph.root, node_id)
        .expect("updated graphics layer")
        .transform_to_parent
        .map_point(Point::default());
    assert!(
        (updated_origin.x - (initial_origin.x + 32.0)).abs() < 0.1,
        "scoped graph update must refresh graphics-layer translation: initial={initial_origin:?} updated={updated_origin:?}"
    );
}

#[test]
fn update_graph_from_applier_reports_hit_dirty_for_moved_clickable_layer() {
    let offset_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let node_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let offset_holder_for_comp = offset_holder.clone();
    let node_id_holder_for_comp = node_id_holder.clone();

    let mut composition = cranpose_ui::run_test_composition(move || {
        let offset = cranpose_core::rememberMutableStateOf(|| 0.0f32);
        *offset_holder_for_comp.borrow_mut() = Some(offset);
        let node_id = cranpose_ui::Box(
            Modifier::empty()
                .size_points(40.0, 20.0)
                .graphics_layer(move || GraphicsLayer {
                    translation_x: offset.get(),
                    ..GraphicsLayer::default()
                })
                .clickable(|_| {}),
            cranpose_ui::BoxSpec::default(),
            || {},
        );
        *node_id_holder_for_comp.borrow_mut() = Some(node_id);
    });

    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 120.0,
        height: 80.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("initial layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
    let node_id = node_id_holder
        .borrow()
        .expect("graphics layer node id should be captured");
    applier.clear_runtime_handle();
    drop(applier);

    let offset = offset_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("offset state should be captured");
    offset.set_value(32.0);

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let report = update_graph_from_applier_report(&mut applier, &mut graph, &[node_id], 1.0);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "dirty clickable graphics layer should be replaceable from retained applier state, got {:?}",
        report.update
    );
    assert!(
        report.hit_graph_dirty,
        "moved clickable layers must refresh hit geometry"
    );
}

#[test]
fn appending_empty_draw_commands_preserves_existing_nodes_and_command_identity() {
    let empty = Rc::new(|_: &mut DrawScopeDefault| {});
    let commands = [
        DrawCommand::Behind(empty.clone()),
        DrawCommand::WithContent(empty.clone()),
        DrawCommand::Overlay(empty),
    ];
    let mut nodes = vec![RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        Vec::new(),
    ))];
    for (placement, phase) in [
        (DrawPlacement::Behind, PrimitivePhase::BeforeChildren),
        (DrawPlacement::Overlay, PrimitivePhase::AfterChildren),
    ] {
        append_draw_nodes(
            &mut nodes,
            42,
            &commands,
            3,
            placement,
            Size::default(),
            phase,
        );
    }
    let actual: Vec<_> = nodes
        .iter()
        .map(|node| {
            let RenderNode::DrawRun(run) = node else {
                panic!("expected a draw run");
            };
            assert!(run.is_empty());
            (
                run.phase,
                run.command
                    .map(|id| (id.node_id, id.command_index, id.placement)),
            )
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            (PrimitivePhase::BeforeChildren, None),
            (
                PrimitivePhase::BeforeChildren,
                Some((42, 3, DrawPlacement::Behind))
            ),
            (
                PrimitivePhase::BeforeChildren,
                Some((42, 4, DrawPlacement::Behind))
            ),
            (
                PrimitivePhase::AfterChildren,
                Some((42, 4, DrawPlacement::Overlay))
            ),
            (
                PrimitivePhase::AfterChildren,
                Some((42, 5, DrawPlacement::Overlay))
            ),
        ]
    );
}

#[test]
fn overlay_draw_commands_are_tagged_after_children() {
    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 4.0, y: 5.0 },
        size: Size {
            width: 20.0,
            height: 10.0,
        },
        ..Default::default()
    };
    let behind = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
        scope.push_recorded(vec![cranpose_ui_graphics::DrawPrimitive::Rect {
            rect: Rect {
                x: 1.0,
                y: 2.0,
                width: 8.0,
                height: 6.0,
            },
            brush: Brush::solid(Color::WHITE),
            stroke: None,
        }]);
    }));
    let overlay = DrawCommand::Overlay(Rc::new(|scope: &mut DrawScopeDefault| {
        scope.push_recorded(vec![cranpose_ui_graphics::DrawPrimitive::Rect {
            rect: Rect {
                x: 3.0,
                y: 1.0,
                width: 5.0,
                height: 4.0,
            },
            brush: Brush::solid(Color::BLACK),
            stroke: None,
        }]);
    }));

    let parent = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 80.0,
            height: 50.0,
        },
        draw_commands: vec![behind, overlay],
        children: vec![child],
        ..Default::default()
    };

    let graph = build_layer_node_for_test(parent, 1.0, false);
    let RenderNode::DrawRun(behind) = &graph.children[0] else {
        panic!("expected before-children draw run");
    };
    let RenderNode::Layer(_) = &graph.children[1] else {
        panic!("expected child layer");
    };
    let RenderNode::DrawRun(overlay) = &graph.children[2] else {
        panic!("expected after-children draw run");
    };

    assert_eq!(behind.phase, PrimitivePhase::BeforeChildren);
    assert_eq!(overlay.phase, PrimitivePhase::AfterChildren);
}

#[test]
fn command_recordings_reuse_buffers_across_rebuilds() {
    let snapshot = || BuildNodeSnapshot {
        node_id: 7001,
        size: Size {
            width: 40.0,
            height: 20.0,
        },
        draw_commands: vec![DrawCommand::Behind(Rc::new(
            |scope: &mut DrawScopeDefault| {
                scope.draw_rect_at(
                    Rect {
                        x: 1.0,
                        y: 2.0,
                        width: 8.0,
                        height: 6.0,
                    },
                    Brush::solid(Color::WHITE),
                );
            },
        ))],
        ..Default::default()
    };
    fn run_of(layer: &LayerNode) -> &DrawRunNode {
        let RenderNode::DrawRun(run) = &layer.children[0] else {
            panic!("expected draw run");
        };
        run
    }

    let graph_a = build_layer_node_for_test(snapshot(), 1.0, false);
    let ptr_a = run_of(&graph_a).recording.shapes().bodies().as_ptr();

    let graph_b = build_layer_node_for_test(snapshot(), 1.0, false);
    let ptr_b = run_of(&graph_b).recording.shapes().bodies().as_ptr();
    assert_ne!(
        ptr_a, ptr_b,
        "a buffer a live graph shares must never be recorded into"
    );
    assert_eq!(
        run_of(&graph_a).recording,
        run_of(&graph_b).recording,
        "re-recording must reproduce the recording"
    );

    drop(graph_a);
    let graph_c = build_layer_node_for_test(snapshot(), 1.0, false);
    assert_eq!(
        run_of(&graph_c).recording.shapes().bodies().as_ptr(),
        ptr_a,
        "the released buffer must be reused for the next recording"
    );

    let held = std::rc::Rc::clone(&run_of(&graph_c).recording);
    drop(graph_c);
    let graph_d = build_layer_node_for_test(snapshot(), 1.0, false);
    let ptr_d = run_of(&graph_d).recording.shapes().bodies().as_ptr();
    assert_ne!(ptr_d, held.shapes().bodies().as_ptr());
    assert_ne!(ptr_d, run_of(&graph_b).recording.shapes().bodies().as_ptr());
}

#[test]
fn stored_content_hash_changes_when_child_transform_changes() {
    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 4.0, y: 5.0 },
        size: Size {
            width: 20.0,
            height: 10.0,
        },
        ..Default::default()
    };
    let mut moved_child = child.clone();
    moved_child.placement.x += 7.0;

    let parent = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 80.0,
            height: 50.0,
        },
        children: vec![child],
        ..Default::default()
    };
    let moved_parent = BuildNodeSnapshot {
        children: vec![moved_child],
        ..parent.clone()
    };

    let static_graph = build_layer_node_for_test(parent, 1.0, false);
    let moved_graph = build_layer_node_for_test(moved_parent, 1.0, false);

    assert_ne!(
        static_graph.target_content_hash(),
        moved_graph.target_content_hash(),
        "moving a child within the parent must invalidate the parent subtree hash"
    );
}

#[test]
fn stored_effect_hash_tracks_local_effect_only() {
    let base = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 80.0,
            height: 50.0,
        },
        ..Default::default()
    };
    let mut effected = base.clone();
    effected.graphics_layer = Some(GraphicsLayer {
        render_effect: Some(cranpose_ui_graphics::RenderEffect::blur(6.0)),
        ..GraphicsLayer::default()
    });

    let base_graph = build_layer_node_for_test(base, 1.0, false);
    let effected_graph = build_layer_node_for_test(effected, 1.0, false);

    assert_eq!(
        base_graph.target_content_hash(),
        effected_graph.target_content_hash(),
        "post-processing effect parameters belong to the effect hash, not the content hash"
    );
    assert_ne!(base_graph.effect_hash(), effected_graph.effect_hash());
}

#[test]
fn text_node_preserves_rtl_alignment_clip_and_baseline_shift() {
    let mut text_style = TextStyle::default();
    text_style.paragraph_style.text_align = TextAlign::Start;
    text_style.paragraph_style.text_direction = TextDirection::Rtl;
    text_style.span_style.baseline_shift = Some(BaselineShift::SUPERSCRIPT);

    let snapshot = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 180.0,
            height: 48.0,
        },
        measured_max_width: Some(180.0),
        annotated_text: Some(AnnotatedString::from("rtl")),
        text_style: Some(text_style),
        text_layout_options: Some(cranpose_ui::TextLayoutOptions {
            overflow: cranpose_ui::TextOverflow::Clip,
            ..Default::default()
        }),
        ..Default::default()
    };

    let graph = build_layer_node_for_test(snapshot, 1.0, false);
    let RenderNode::Primitive(text_primitive) = &graph.children[0] else {
        panic!("expected text primitive");
    };
    let PrimitiveNode::Text(text) = &text_primitive.node else {
        panic!("expected text primitive");
    };
    let clip = text
        .clip
        .expect("clipped overflow should produce a clip rect");

    assert!(
        text.rect.x > 0.0,
        "RTL start alignment should shift the text rect within the available width"
    );
    assert!(
        clip.y < text.rect.y,
        "baseline shift must expand the clip upward so superscript glyphs are preserved"
    );
    assert!(
        clip.intersect(text.rect).is_some(),
        "the clip rect must intersect the shifted text draw rect"
    );
}

#[test]
fn clipped_text_node_raster_bounds_use_measured_text_width_not_full_box() {
    let snapshot = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 320.0,
            height: 48.0,
        },
        measured_max_width: Some(320.0),
        annotated_text: Some(AnnotatedString::from("short")),
        text_style: Some(TextStyle::default()),
        text_layout_options: Some(cranpose_ui::TextLayoutOptions {
            overflow: cranpose_ui::TextOverflow::Clip,
            ..Default::default()
        }),
        ..Default::default()
    };

    let graph = build_layer_node_for_test(snapshot, 1.0, false);
    let RenderNode::Primitive(text_primitive) = &graph.children[0] else {
        panic!("expected text primitive");
    };
    let PrimitiveNode::Text(text) = &text_primitive.node else {
        panic!("expected text primitive");
    };
    let clip = text.clip.expect("clipped text should keep a clip rect");

    assert!(
        text.rect.width < 320.0,
        "text raster bounds should track measured glyph width instead of full content width"
    );
    assert_eq!(
        clip.width, 322.0,
        "text clip should still preserve the full content box plus clip padding"
    );
}

#[test]
fn text_field_pan_shifts_glyphs_and_clips_to_field_bounds() {
    let pan_offset = 25.0_f32;
    let field_width = 80.0_f32;
    let resolved_viewports = Rc::new(std::cell::RefCell::new(Vec::new()));
    let viewports = resolved_viewports.clone();
    let make_snapshot = |text_pan: Option<cranpose_ui::TextPanResolver>| BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: field_width,
            height: 24.0,
        },
        measured_max_width: Some(field_width),
        annotated_text: Some(AnnotatedString::from(
            "a very long single line of text that cannot fit",
        )),
        text_style: Some(TextStyle::default()),
        text_layout_options: Some(cranpose_ui::TextLayoutOptions::default()),
        text_pan,
        ..Default::default()
    };

    let text_node = |snapshot: BuildNodeSnapshot| {
        let graph = build_layer_node_for_test(snapshot, 1.0, false);
        let RenderNode::Primitive(text_primitive) = &graph.children[0] else {
            panic!("expected text primitive");
        };
        let PrimitiveNode::Text(text) = &text_primitive.node else {
            panic!("expected text primitive");
        };
        (**text).clone()
    };

    let unpanned = text_node(make_snapshot(None));
    let panned = text_node(make_snapshot(Some(Rc::new(move |viewport| {
        viewports.borrow_mut().push(viewport);
        pan_offset
    }))));

    assert_eq!(
        resolved_viewports.borrow().as_slice(),
        &[field_width],
        "the pan resolver must receive the content viewport width"
    );
    assert_eq!(
        panned.rect.x, -pan_offset,
        "text glyphs must shift left by the pan offset"
    );
    assert!(
        panned.rect.width > field_width,
        "panned single-line text must be laid out unconstrained, got {}",
        panned.rect.width
    );
    assert!(
        panned.rect.width >= unpanned.rect.width,
        "unconstrained layout must not be narrower than wrapped layout"
    );
    assert!(
        panned.rect.height <= unpanned.rect.height,
        "single-line layout must not wrap onto extra lines"
    );
    let clip = panned
        .clip
        .expect("panned text field must clip to field bounds");
    assert!(
        clip.x + clip.width <= field_width + TEXT_CLIP_PAD + f32::EPSILON,
        "clip must not extend past the field bounds, got {clip:?}"
    );
}

#[test]
fn translated_content_context_preserves_descendant_text_motion_when_unspecified() {
    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 11.0, y: 7.0 },
        size: Size {
            width: 120.0,
            height: 32.0,
        },
        measured_max_width: Some(120.0),
        annotated_text: Some(AnnotatedString::from("scrolling")),
        text_style: Some(TextStyle::default()),
        ..Default::default()
    };
    let parent = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 160.0,
            height: 64.0,
        },
        content_offset: Point { x: 0.0, y: -18.5 },
        translated_content_context: true,
        children: vec![child],
        ..Default::default()
    };

    let graph = build_layer_node_for_test(parent, 1.0, false);
    let RenderNode::Layer(child_layer) = &graph.children[0] else {
        panic!("expected child layer");
    };
    let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
        panic!("expected text primitive");
    };
    let PrimitiveNode::Text(text) = &text_primitive.node else {
        panic!("expected text primitive");
    };

    assert_eq!(text.text_style.paragraph_style.text_motion, None);
    assert!(!child_layer.motion_context_animated);
}

#[test]
fn content_offset_without_translated_context_keeps_descendant_text_unspecified() {
    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 11.0, y: 7.0 },
        size: Size {
            width: 120.0,
            height: 32.0,
        },
        measured_max_width: Some(120.0),
        annotated_text: Some(AnnotatedString::from("scrolling")),
        text_style: Some(TextStyle::default()),
        ..Default::default()
    };
    let parent = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 160.0,
            height: 64.0,
        },
        content_offset: Point { x: 0.0, y: -18.0 },
        children: vec![child],
        ..Default::default()
    };

    let graph = build_layer_node_for_test(parent, 1.0, false);
    let RenderNode::Layer(child_layer) = &graph.children[0] else {
        panic!("expected child layer");
    };
    let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
        panic!("expected text primitive");
    };
    let PrimitiveNode::Text(text) = &text_primitive.node else {
        panic!("expected text primitive");
    };

    assert_eq!(
        text.text_style.paragraph_style.text_motion, None,
        "content_offset alone must not force text onto the translated-content motion path"
    );
    assert!(!child_layer.motion_context_animated);
}

#[test]
fn translated_content_context_preserves_effectful_text_motion_when_unspecified() {
    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 11.0, y: 7.0 },
        size: Size {
            width: 120.0,
            height: 32.0,
        },
        measured_max_width: Some(120.0),
        annotated_text: Some(AnnotatedString::from("shadow")),
        text_style: Some(TextStyle::from_span_style(SpanStyle {
            shadow: Some(cranpose_ui::text::Shadow {
                color: Color::BLACK,
                offset: Point::new(1.0, 2.0),
                blur_radius: 3.0,
            }),
            ..SpanStyle::default()
        })),
        ..Default::default()
    };
    let parent = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 160.0,
            height: 64.0,
        },
        content_offset: Point { x: 0.0, y: -18.5 },
        translated_content_context: true,
        children: vec![child],
        ..Default::default()
    };

    let graph = build_layer_node_for_test(parent, 1.0, false);
    let RenderNode::Layer(child_layer) = &graph.children[0] else {
        panic!("expected child layer");
    };
    let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
        panic!("expected text primitive");
    };
    let PrimitiveNode::Text(text) = &text_primitive.node else {
        panic!("expected text primitive");
    };

    assert_eq!(text.text_style.paragraph_style.text_motion, None);
}

#[test]
fn animated_motion_marker_preserves_descendant_text_motion_when_unspecified() {
    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 11.0, y: 7.0 },
        size: Size {
            width: 120.0,
            height: 32.0,
        },
        measured_max_width: Some(120.0),
        annotated_text: Some(AnnotatedString::from("lazy")),
        text_style: Some(TextStyle::default()),
        ..Default::default()
    };
    let parent = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 160.0,
            height: 64.0,
        },
        motion_context_animated: true,
        children: vec![child],
        ..Default::default()
    };

    let graph = build_layer_node_for_test(parent, 1.0, false);
    let RenderNode::Layer(child_layer) = &graph.children[0] else {
        panic!("expected child layer");
    };
    let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
        panic!("expected text primitive");
    };
    let PrimitiveNode::Text(text) = &text_primitive.node else {
        panic!("expected text primitive");
    };

    assert_eq!(text.text_style.paragraph_style.text_motion, None);
    assert!(graph.motion_context_animated);
    assert!(child_layer.motion_context_animated);
}

#[test]
fn lazy_column_item_text_keeps_unspecified_motion_at_origin() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        let list_state = rememberLazyListState();
        LazyColumn(
            Modifier::empty(),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.item_keyed(Some(0), None, || {
                    Text("LazyMotion", Modifier::empty(), TextStyle::default());
                });
            },
        );
    });

    let root = composition.root().expect("lazy column root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let _ = applier
        .compute_layout(
            root,
            Size {
                width: 240.0,
                height: 240.0,
            },
        )
        .expect("lazy column layout");
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
    applier.clear_runtime_handle();

    assert_eq!(find_text_motion(&graph.root, "LazyMotion"), Some(None));
}

#[test]
fn scrolled_lazy_column_item_text_keeps_unspecified_motion_at_rest() {
    use std::{cell::RefCell, rc::Rc};

    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = state_holder.clone();
    let mut composition = cranpose_ui::run_test_composition(move || {
        let list_state = rememberLazyListState();
        *state_holder_for_comp.borrow_mut() = Some(list_state);
        LazyColumn(
            Modifier::empty().height(120.0),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(8, |index| {
                    Text(
                        format!("LazyMotion {index}"),
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
                });
            },
        );
    });

    let list_state = (*state_holder.borrow()).expect("lazy list state should be captured");
    list_state.scroll_to_item(3, 0.0);

    let root = composition.root().expect("lazy column root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let active_children = lay_out_lazy_column(&mut applier, root);
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
    let child_debug: Vec<String> = active_children
        .iter()
        .map(|&child_id| {
            if let Ok(summary) = applier.with_node::<LayoutNode, _>(child_id, |node| {
                format!(
                    "layout#{child_id} placed={} text={:?} children={:?}",
                    node.layout_state().is_placed(),
                    node.modifier_slices_snapshot()
                        .text_content()
                        .map(str::to_string),
                    node.children.clone()
                )
            }) {
                summary
            } else if let Ok(summary) =
                applier.with_node::<SubcomposeLayoutNode, _>(child_id, |node| {
                    format!(
                        "subcompose#{child_id} placed={} active_children={:?}",
                        node.layout_state().is_placed(),
                        node.active_children()
                    )
                })
            {
                summary
            } else {
                format!("missing#{child_id}")
            }
        })
        .collect();
    applier.clear_runtime_handle();

    let first_index = list_state.first_visible_item_index();
    assert!(
        first_index > 0,
        "lazy list should move away from origin before graph building, observed first_index={first_index}"
    );
    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);
    assert_eq!(
        find_text_motion(&graph.root, &format!("LazyMotion {first_index}")),
        Some(None),
        "graph labels after scroll: {labels:?}, active_children={active_children:?}, child_debug={child_debug:?}"
    );
}

#[test]
fn scrolled_lazy_column_render_graph_keeps_beyond_bound_text_rows() {
    use std::{cell::RefCell, rc::Rc};

    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = state_holder.clone();
    let mut composition = cranpose_ui::run_test_composition(move || {
        let list_state = rememberLazyListState();
        *state_holder_for_comp.borrow_mut() = Some(list_state);
        let mut spec = LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0));
        spec.beyond_bounds_item_count = 0;
        LazyColumn(Modifier::empty().height(96.0), list_state, spec, |scope| {
            scope.items(12, |index| {
                Text(
                    format!("WarmRow {index}"),
                    Modifier::empty().height(32.0),
                    TextStyle::default(),
                );
            });
        });
    });

    let list_state = (*state_holder.borrow()).expect("lazy list state should be captured");
    list_state.scroll_to_item(4, 0.0);

    let root = composition.root().expect("lazy column root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let active_children = lay_out_lazy_column(&mut applier, root);
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
    applier.clear_runtime_handle();

    let visible_indices: Vec<_> = list_state
        .layout_info()
        .visible_items_info
        .iter()
        .map(|item| item.index)
        .collect();
    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);

    assert_eq!(
        visible_indices,
        vec![4, 5, 6],
        "test setup expects exactly three viewport-visible rows"
    );
    assert!(
        labels.iter().any(|label| label == "WarmRow 7"),
        "render graph must retain at least one after-bound text row for glyph prewarm; labels={labels:?}, active_children={active_children:?}"
    );
}

#[test]
fn scrolled_lazy_column_uses_visible_item_offset_as_snap_anchor_offset() {
    use std::{cell::RefCell, rc::Rc};

    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = state_holder.clone();
    let mut composition = cranpose_ui::run_test_composition(move || {
        let list_state = rememberLazyListState();
        *state_holder_for_comp.borrow_mut() = Some(list_state);
        LazyColumn(
            Modifier::empty().height(120.0),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(8, |index| {
                    Text(
                        format!("LazySnap {index}"),
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
                });
            },
        );
    });

    let list_state = (*state_holder.borrow()).expect("lazy list state should be captured");
    list_state.scroll_to_item(2, 7.5);

    let root = composition.root().expect("lazy column root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let _ = applier
        .compute_layout(
            root,
            Size {
                width: 240.0,
                height: 240.0,
            },
        )
        .expect("lazy column layout");
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("lazy column graph");
    applier.clear_runtime_handle();

    let layout_info = list_state.layout_info();
    let first_visible_offset = layout_info
        .visible_items_info
        .first()
        .expect("lazy layout should expose visible item info")
        .offset;
    let snap_offset = find_translated_content_offset(&graph.root)
        .expect("lazy list graph should include translated content context");

    assert!(
        (snap_offset.y - first_visible_offset).abs() <= 0.001,
        "lazy snap offset must follow the visible content origin; snap_offset={snap_offset:?} first_visible_offset={first_visible_offset}"
    );
}

#[test]
fn explicit_static_text_motion_is_preserved_under_scrolling_context() {
    let child = BuildNodeSnapshot {
        node_id: 2,
        placement: Point { x: 11.0, y: 7.0 },
        size: Size {
            width: 120.0,
            height: 32.0,
        },
        measured_max_width: Some(120.0),
        annotated_text: Some(AnnotatedString::from("static")),
        text_style: Some(TextStyle::from_paragraph_style(
            cranpose_ui::text::ParagraphStyle {
                text_motion: Some(TextMotion::Static),
                ..Default::default()
            },
        )),
        ..Default::default()
    };
    let parent = BuildNodeSnapshot {
        node_id: 1,
        size: Size {
            width: 160.0,
            height: 64.0,
        },
        content_offset: Point { x: 0.0, y: -18.5 },
        translated_content_context: true,
        children: vec![child],
        ..Default::default()
    };

    let graph = build_layer_node_for_test(parent, 1.0, false);
    let RenderNode::Layer(child_layer) = &graph.children[0] else {
        panic!("expected child layer");
    };
    let RenderNode::Primitive(text_primitive) = &child_layer.children[0] else {
        panic!("expected text primitive");
    };
    let PrimitiveNode::Text(text) = &text_primitive.node else {
        panic!("expected text primitive");
    };

    assert_eq!(
        text.text_style.paragraph_style.text_motion,
        Some(TextMotion::Static),
        "explicit text motion must win over inherited scrolling motion context"
    );
}

#[test]
fn wrapped_paragraph_paints_the_height_it_measured() {
    const BODY: &str = "fed back картица scored fp32 износ once paper fed Vision dropped \
         fed widest the strip mask prompt mask threshold Vision on датум instance mask \
         износ Apple";
    const FOLLOWING: &str = "FOLLOWING SIBLING";

    let app_context = cranpose_ui::AppContext::new();
    app_context.enter(|| {
        cranpose_ui::text::set_text_measurer(
            crate::software_text_raster::SoftwareTextMeasurer::from_fonts_or_default(&[], 8192),
        );
        let mut composition = cranpose_ui::run_test_composition(move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Text(BODY.to_string(), Modifier::empty(), TextStyle::default());
                    Text(
                        FOLLOWING.to_string(),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                },
            );
        });

        let root = composition.root().expect("composition root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout = applier
            .compute_layout(
                root,
                Size {
                    width: 245.0,
                    height: 900.0,
                },
            )
            .expect("layout");

        fn find_box<'a>(node: &'a LayoutBox, value: &str) -> Option<&'a LayoutBox> {
            if node
                .node_data
                .modifier_slices()
                .text_content()
                .is_some_and(|text| text == value)
            {
                return Some(node);
            }
            node.children
                .iter()
                .find_map(|child| find_box(child, value))
        }
        let body_box = find_box(layout.root(), BODY).expect("measured paragraph box");
        let following_box = find_box(layout.root(), FOLLOWING).expect("measured sibling box");
        let measured_height = body_box.rect.height;
        let following_top = following_box.rect.y;
        assert!(
            measured_height > 60.0,
            "test setup expects a genuinely multi-line paragraph, got {measured_height}"
        );
        assert!(
            body_box.rect.width < 245.0,
            "test setup expects the node to be placed at its own measured width, \
             not the full constraint, got {}",
            body_box.rect.width
        );

        let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("render graph");
        applier.clear_runtime_handle();

        fn squashed(value: &str) -> String {
            value.chars().filter(|c| !c.is_whitespace()).collect()
        }
        fn find_text<'a>(layer: &'a LayerNode, value: &str) -> Option<&'a TextPrimitiveNode> {
            for child in &layer.children {
                match child {
                    RenderNode::Primitive(primitive) => {
                        if let PrimitiveNode::Text(text) = &primitive.node
                            && squashed(&text.text.text) == squashed(value)
                        {
                            return Some(text);
                        }
                    }
                    RenderNode::Layer(child_layer) => {
                        if let Some(found) = find_text(child_layer, value) {
                            return Some(found);
                        }
                    }
                    RenderNode::DrawRun(_) => {}
                }
            }
            None
        }
        let painted = find_text(&graph.root, BODY).expect("painted paragraph");

        assert!(
            (painted.rect.height - measured_height).abs() < 0.5,
            "paragraph painted {:.2} tall into a box layout measured at {:.2} \
             (painted rect {:?})",
            painted.rect.height,
            measured_height,
            painted.rect
        );
        assert!(
            painted.rect.y + painted.rect.height <= following_top + 0.5,
            "painted paragraph bottom {:.2} runs past the following sibling placed at \
             {:.2}",
            painted.rect.y + painted.rect.height,
            following_top
        );
    });
}

struct TestWindow(Size);

impl cranpose_ui::WindowRootDescriptor for TestWindow {
    fn layout_size(&self) -> Size {
        self.0
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn with_window_root_scene(check: impl FnOnce(&mut MemoryApplier, NodeId, NodeId)) {
    let window_node = Rc::new(std::cell::Cell::new(None));
    let window: Rc<dyn cranpose_ui::WindowRootDescriptor> = Rc::new(TestWindow(Size {
        width: 320.0,
        height: 240.0,
    }));
    let mut composition = cranpose_ui::run_test_composition({
        let window_node = Rc::clone(&window_node);
        let window = Rc::clone(&window);
        move || {
            let window = Rc::clone(&window);
            let window_node = Rc::clone(&window_node);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Text(
                    "outside".to_string(),
                    Modifier::empty().height(20.0),
                    TextStyle::default(),
                );
                let id = cranpose_ui::Box(
                    Modifier::empty().window_root(Rc::clone(&window)),
                    cranpose_ui::BoxSpec::default(),
                    || {
                        Text(
                            "inside".to_string(),
                            Modifier::empty().height(20.0),
                            TextStyle::default(),
                        );
                    },
                );
                window_node.set(Some(id));
            });
        }
    });
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(
            root,
            Size {
                width: 240.0,
                height: 300.0,
            },
        )
        .expect("layout");
    let window_node = window_node.get().expect("window node");
    check(&mut applier, root, window_node);
    applier.clear_runtime_handle();
}

fn window_scene_starts_at_the_origin(graph: &RenderGraph, why: &str) {
    assert_eq!(
        graph.root.transform_to_parent,
        layer_transform_to_parent(
            graph.root.local_bounds,
            Point::default(),
            &GraphicsLayer::default()
        ),
        "{why}"
    );
}

#[test]
fn window_root_subtree_leaves_the_parent_scene_and_starts_its_own() {
    with_window_root_scene(|applier, root, window_node| {
        let primary = build_graph_from_applier(applier, root, 1.0).expect("primary graph");
        let mut labels = Vec::new();
        collect_text_labels(&primary.root, &mut labels);
        assert_eq!(labels, vec!["outside".to_string()]);

        let graph = build_graph_from_applier(applier, window_node, 1.0).expect("window graph");
        let mut labels = Vec::new();
        collect_text_labels(&graph.root, &mut labels);
        assert_eq!(labels, vec!["inside".to_string()]);
        assert_eq!(layer_identity(&graph.root), Some(window_node));
        assert_eq!(
            graph.root.local_bounds,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 240.0
            },
            "the window's scene is the window's size"
        );
        window_scene_starts_at_the_origin(
            &graph,
            "the window's scene starts at the origin, not where the column placed the box",
        );
    });
}

#[test]
fn a_patched_window_scene_stays_at_the_window_s_own_origin() {
    with_window_root_scene(|applier, _root, window_node| {
        let mut graph = build_graph_from_applier(applier, window_node, 1.0).expect("window graph");
        assert!(
            update_graph_from_applier(applier, &mut graph, &[window_node], 1.0),
            "the window's own node is patched in place rather than rebuilt"
        );
        window_scene_starts_at_the_origin(
            &graph,
            "a window scene patched in place still starts at the window's origin, not at the \
             placement its node has in the window it was declared in",
        );
    });
}

#[test]
fn a_patched_parent_leaves_a_window_root_child_out_of_its_scene() {
    with_window_root_scene(|applier, root, _window_node| {
        let mut graph = build_graph_from_applier(applier, root, 1.0).expect("primary graph");
        assert!(
            update_graph_from_applier(applier, &mut graph, &[root], 1.0),
            "a parent whose child is in a window of its own is patched in place; taking the \
             child for one of its own leaves the scoped update no choice but to rebuild"
        );
        let mut labels = Vec::new();
        collect_text_labels(&graph.root, &mut labels);
        assert_eq!(
            labels,
            vec!["outside".to_string()],
            "the window's content belongs to the window's scene, not its parent's"
        );
    });
}

#[test]
fn a_child_that_leaves_for_a_window_leaves_its_parent_s_scene_with_it() {
    let torn: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let window: Rc<dyn cranpose_ui::WindowRootDescriptor> = Rc::new(TestWindow(Size {
        width: 320.0,
        height: 240.0,
    }));
    let mut composition = cranpose_ui::run_test_composition({
        let torn = Rc::clone(&torn);
        let window = Rc::clone(&window);
        move || {
            let torn = Rc::clone(&torn);
            let window = Rc::clone(&window);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let state = cranpose_core::rememberMutableStateOf(|| false);
                *torn.borrow_mut() = Some(state);
                Text(
                    "outside".to_string(),
                    Modifier::empty().height(20.0),
                    TextStyle::default(),
                );
                let modifier = if state.get() {
                    Modifier::empty().window_root(Rc::clone(&window))
                } else {
                    Modifier::empty()
                };
                cranpose_ui::Box(modifier, cranpose_ui::BoxSpec::default(), || {
                    Text(
                        "inside".to_string(),
                        Modifier::empty().height(20.0),
                        TextStyle::default(),
                    );
                });
            });
        }
    });
    let root = composition.root().expect("root");
    let viewport = Size {
        width: 240.0,
        height: 300.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, viewport).expect("layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("graph");
    applier.clear_runtime_handle();
    drop(applier);
    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);
    assert_eq!(
        labels,
        vec!["outside".to_string(), "inside".to_string()],
        "both are in the one window to start with"
    );

    let state = torn.borrow().expect("the torn state");
    state.set_value(true);
    composition
        .process_invalid_scopes()
        .expect("the child takes a window of its own");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, viewport).expect("layout");
    let patched = update_graph_from_applier(&mut applier, &mut graph, &[root], 1.0);
    applier.clear_runtime_handle();
    drop(applier);

    assert!(
        patched,
        "a parent whose child left for a window of its own is patched in place"
    );
    let mut labels = Vec::new();
    collect_text_labels(&graph.root, &mut labels);
    assert_eq!(
        labels,
        vec!["outside".to_string()],
        "the child that left draws in its own window now, not in the one it left"
    );
}

fn lay_out_lazy_column(applier: &mut MemoryApplier, root: NodeId) -> Vec<NodeId> {
    let _ = applier
        .compute_layout(
            root,
            Size {
                width: 240.0,
                height: 240.0,
            },
        )
        .expect("lazy column layout");
    applier
        .with_node::<SubcomposeLayoutNode, _>(root, |node| node.active_children())
        .expect("lazy column should be subcompose")
}

fn scrolling_rows_column(scroll_state: ScrollState, row_modifier: fn() -> Modifier) {
    Column(
        Modifier::empty()
            .size_points(240.0, 320.0)
            .vertical_scroll(scroll_state, false),
        ColumnSpec::default(),
        move || {
            for index in 0..12usize {
                cranpose_ui::Box(row_modifier(), cranpose_ui::BoxSpec::default(), move || {
                    Text(
                        format!("row {index}"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                });
            }
        },
    );
}

fn composited_row() -> Modifier {
    Modifier::empty()
        .size_points(240.0, 60.0)
        .graphics_layer(|| GraphicsLayer {
            alpha: 0.8,
            ..GraphicsLayer::default()
        })
}

fn tinted_row() -> Modifier {
    Modifier::empty()
        .size_points(240.0, 60.0)
        .background(Color(0.9, 0.9, 0.92, 1.0))
}

fn scroll_viewport() -> Size {
    Size {
        width: 240.0,
        height: 320.0,
    }
}

fn initial_scrolled_graph(composition: &mut cranpose_ui::TestComposition) -> (NodeId, RenderGraph) {
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, scroll_viewport())
        .expect("initial scroll layout");
    let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
    graph.root.recompute_raster_cache_hashes();
    applier.clear_runtime_handle();
    (root, graph)
}
