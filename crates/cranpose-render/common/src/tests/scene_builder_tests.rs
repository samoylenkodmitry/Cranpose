use std::{cell::RefCell, rc::Rc};

use cranpose_foundation::{
    PointerEvent, PointerEventKind,
    lazy::{LazyListScope, LazyListState, rememberLazyListState},
};
use cranpose_ui::{
    Color, Column, ColumnSpec, DrawCommand, LayoutBox, LayoutEngine, LayoutNode, LayoutTree,
    LazyColumn, LazyColumnSpec, LinearArrangement, Modifier, Point, Rect, RoundedCornerShape,
    ScrollState, Size, Spacer, Text, TextLayoutOptions, TextOptions, TextOverflow, TextStyle,
    TextWithOptions,
    text::{AnnotatedString, BaselineShift, SpanStyle, TextAlign, TextDirection, TextMotion},
};
use cranpose_ui_graphics::{
    Brush, DrawPlacement, DrawPrimitive, DrawScopeDefault, GraphicsLayer, LayerShape, RenderEffect,
};

use super::{node_layer::root_record, text_node::TEXT_CLIP_PAD, *};
use crate::{
    graph::{DrawRunNode, PrimitivePhase, TextPrimitiveNode},
    scene_store::{ItemRef, RecordChanges},
};

// Each of `ids` with the update that records it again.
fn content(ids: &[NodeId]) -> Vec<(NodeId, NodeUpdate)> {
    ids.iter().map(|&id| (id, NodeUpdate::Content)).collect()
}
fn laid_out_scene_column(child_count: usize) -> cranpose_ui::TestComposition {
    let mut composition = cranpose_ui::run_test_composition(move || {
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for index in 0..child_count {
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
                height: child_count as f32 * 20.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    drop(applier);
    composition
}

#[test]
fn scene_snapshot_preserves_children_beyond_inline_capacity_in_order() {
    let mut composition = laid_out_scene_column(12);
    let root = composition.root().expect("root");
    let mut applier = composition.applier_mut();
    let stored_children = applier
        .with_node::<LayoutNode, _>(root, |node| node.children().as_ptr())
        .expect("root layout node");
    read_node_data(&applier, root, |data| {
        assert_eq!(data.children.as_ptr(), stored_children);
    })
    .expect("root scene data");
    let graph = built(&applier, root).expect("graph");
    let mut labels = Vec::new();
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
    assert_eq!(
        labels,
        (0..12)
            .map(|index| format!("child-{index}"))
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "manual release timing probe"]
fn scene_rebuild_timing() {
    use std::hint::black_box;

    use web_time::Instant;

    let iterations = std::env::var("SCENE_REBUILD_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1000);
    for children in [8, 64, 256] {
        let mut composition = laid_out_scene_column(children);
        let root = composition.root().expect("root");
        let mut applier = composition.applier_mut();
        let applier = &mut *applier;
        let mut graph = Built::default();
        for _ in 0..100 {
            graph.rebuild_from_applier(applier, root);
        }
        let start = Instant::now();
        for _ in 0..iterations {
            graph.rebuild_from_applier(applier, root);
            black_box(&graph.store);
        }
        let ns_per_rebuild = start.elapsed().as_nanos() as f64 / iterations as f64;
        println!(
            "SCENE_REBUILD children={children} iterations={iterations} ns_per_rebuild={ns_per_rebuild:.3}"
        );
    }
}

#[test]
fn scene_read_keeps_lazy_children_stable_while_the_handle_changes() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        LazyColumn(
            Modifier::empty().size_points(240.0, 240.0),
            rememberLazyListState(),
            LazyColumnSpec::default(),
            |scope| {
                scope.items(12, |_| {
                    Spacer(Modifier::empty().height(20.0));
                });
            },
        );
    });
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let expected = lay_out_lazy_column(&mut applier, root);
    let node: &dyn Any = applier.get_ref(root).expect("root node");
    let handle = node
        .downcast_ref::<SubcomposeLayoutNode>()
        .expect("lazy root")
        .handle();
    read_node_data(&applier, root, |data| {
        assert_eq!(data.children, expected);
        handle.set_active_children(std::iter::empty());
        assert_eq!(data.children, expected);
    })
    .expect("scene data");
    read_node_data(&applier, root, |data| assert!(data.children.is_empty()))
        .expect("updated scene data");
    applier.clear_runtime_handle();
}

// A scene the tests build and update through the builder's own entry
// points, which report how an update went.
#[derive(Default)]
struct Built {
    store: SceneStore,
    walk: WalkMemory,
}

impl Built {
    fn update_from_applier(
        &mut self,
        applier: &MemoryApplier,
        _root: NodeId,
        updates: SceneUpdates<'_>,
    ) -> GraphUpdateReport {
        update_from_applier(applier, &mut self.store, &mut self.walk, updates)
    }

    fn rebuild_from_applier(&mut self, applier: &MemoryApplier, root: NodeId) {
        assert!(
            rebuild_from_applier(applier, root, &mut self.store, &mut self.walk),
            "the root lowers"
        );
    }

    fn root_layer(&self) -> Option<LayerRef<'_>> {
        self.store.root_layer()
    }
}

// The scene of the layout tree below `root`, built from scratch.
fn built(applier: &MemoryApplier, root: NodeId) -> Option<Built> {
    let mut built = Built::default();
    rebuild_from_applier(applier, root, &mut built.store, &mut built.walk).then_some(built)
}

// What each record of `graph` that draws for a node was at its last
// changes, by node.
fn record_changes(graph: &Built) -> HashMap<NodeId, RecordChanges> {
    fn visit(layer: LayerRef<'_>, out: &mut HashMap<NodeId, RecordChanges>) {
        if let Some(node) = layer.node_id() {
            out.insert(node, layer.changes());
        }
        for child in layer.children() {
            visit(child, out);
        }
    }
    let mut out = HashMap::default();
    visit(root_of(&graph.store), &mut out);
    out
}

// Updates `graph` for the dirty nodes `dirty` and checks that no record of
// a clean node drew anew: a scroll moves clean rows, it writes none again.
fn update_keeping_clean_records(
    graph: &mut Built,
    applier: &MemoryApplier,
    root: NodeId,
    dirty: &[NodeId],
) -> GraphUpdateReport {
    let before = record_changes(graph);
    let report = graph.update_from_applier(applier, root, &content(dirty));
    let after = record_changes(graph);
    let mut redrawn: Vec<NodeId> = before
        .iter()
        .filter(|(node, was)| {
            !dirty.contains(node) && after.get(node).is_some_and(|now| !was.same_own(*now))
        })
        .map(|(node, _)| *node)
        .collect();
    redrawn.sort_unstable();
    assert!(
        redrawn.is_empty(),
        "records of clean nodes drew anew: {redrawn:?} (dirty={dirty:?})"
    );
    report
}

fn find_text_motion(layer: LayerRef<'_>, label: &str) -> Option<Option<TextMotion>> {
    layer.items().find_map(|item| match item {
        ItemRef::Text(text) if text.render_text.text() == label => {
            Some(text.text_style.paragraph_style.text_motion)
        }
        ItemRef::Layer(child) | ItemRef::Leaf(child) => find_text_motion(child, label),
        ItemRef::Text(_) | ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => None,
    })
}

fn collect_text_labels(layer: LayerRef<'_>, labels: &mut Vec<String>) {
    for item in layer.items() {
        match item {
            ItemRef::Text(text) => labels.push(text.render_text.text().to_owned()),
            ItemRef::Layer(child) | ItemRef::Leaf(child) => collect_text_labels(child, labels),
            ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => {}
        }
    }
}

fn find_text_top(layer: LayerRef<'_>, label: &str) -> Option<f32> {
    fn search(layer: LayerRef<'_>, label: &str, transform: ProjectiveTransform) -> Option<f32> {
        layer.items().find_map(|item| match item {
            ItemRef::Text(text) if text.render_text.text() == label => {
                let quad = transform.map_rect(text.rect);
                let top = quad
                    .iter()
                    .map(|point| point[1])
                    .fold(f32::INFINITY, f32::min);
                top.is_finite().then_some(top)
            }
            ItemRef::Layer(child) | ItemRef::Leaf(child) => {
                search(child, label, child.transform_to_parent().then(transform))
            }
            ItemRef::Text(_) | ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => None,
        })
    }

    search(layer, label, ProjectiveTransform::identity())
}

// The child record among `layer`'s items at `index`, counting every item.
fn child_at(layer: LayerRef<'_>, index: usize) -> LayerRef<'_> {
    match layer.items().nth(index) {
        Some(ItemRef::Layer(child) | ItemRef::Leaf(child)) => child,
        _ => panic!("expected a child record at item {index}"),
    }
}

fn find_layer_by_node_id(layer: LayerRef<'_>, node_id: NodeId) -> Option<LayerRef<'_>> {
    if layer.kind() != RecordKind::Leaf && layer.node_id() == Some(node_id) {
        return Some(layer);
    }
    layer
        .children()
        .filter(|child| child.kind() != RecordKind::Leaf)
        .find_map(|child| find_layer_by_node_id(child, node_id))
}

fn find_inline_by_node_id(layer: LayerRef<'_>, node_id: NodeId) -> Option<LayerRef<'_>> {
    layer.items().find_map(|item| match item {
        ItemRef::Layer(child) => find_inline_by_node_id(child, node_id),
        ItemRef::Leaf(leaf) => (leaf.node_id() == Some(node_id)).then_some(leaf),
        ItemRef::Text(_) | ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => None,
    })
}

// Where the node `node_id` is placed in its parent: its layer's transform,
// or the translation that places it when it draws in its parent's layer.
fn find_node_transform(layer: LayerRef<'_>, node_id: NodeId) -> Option<ProjectiveTransform> {
    find_layer_by_node_id(layer, node_id)
        .or_else(|| find_inline_by_node_id(layer, node_id))
        .map(LayerRef::transform_to_parent)
}

fn find_layer_origin(layer: LayerRef<'_>, node_id: NodeId) -> Option<Point> {
    fn search(
        layer: LayerRef<'_>,
        node_id: NodeId,
        transform: ProjectiveTransform,
    ) -> Option<Point> {
        if layer.node_id() == Some(node_id) {
            return Some(transform.map_point(Point::default()));
        }
        layer
            .children()
            .find_map(|child| search(child, node_id, child.transform_to_parent().then(transform)))
    }

    search(layer, node_id, ProjectiveTransform::identity())
}

fn find_translated_content_offset(layer: LayerRef<'_>) -> Option<Point> {
    if layer.translated_content_context() {
        return Some(layer.translated_content_offset());
    }
    layer
        .children()
        .filter(|child| child.kind() != RecordKind::Leaf)
        .find_map(find_translated_content_offset)
}

fn graph_has_runtime_shader_effect(layer: LayerRef<'_>) -> bool {
    layer
        .effect()
        .is_some_and(RenderEffect::contains_runtime_shader)
        || layer.children().any(graph_has_runtime_shader_effect)
}

// The root record of `store`.
fn root_of(store: &SceneStore) -> LayerRef<'_> {
    store.root_layer().expect("the store holds a scene")
}

// The first run among the items of `layer`.
fn first_run(layer: LayerRef<'_>) -> &DrawRunNode {
    layer
        .items()
        .find_map(|item| match item {
            ItemRef::Run(run) => Some(run),
            _ => None,
        })
        .expect("expected draw run")
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
    let graph = built(&applier, root).expect("rounded clip graph");
    applier.clear_runtime_handle();

    let graph_root = graph.root_layer().expect("a scene");
    let rounded_layer = find_layer_by_node_id(graph_root, root).expect("rounded layer");
    assert!(rounded_layer.graphics_layer().clip);
    assert!(matches!(
        rounded_layer.graphics_layer().shape,
        LayerShape::Rounded(_)
    ));
    assert!(rounded_layer.effect().is_none());
    assert!(rounded_layer.isolation().shape_clip);
    assert!(
        !graph_has_runtime_shader_effect(graph.root_layer().expect("a scene")),
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
        let card_id = Rc::clone(&card_id);
        cranpose_ui::Box(
            Modifier::empty().size_points(240.0, 120.0),
            cranpose_ui::BoxSpec::default(),
            move || {
                let shape = LayerShape::Rounded(RoundedCornerShape::uniform(12.0));
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
                        .background(Color(0.2, 0.4, 0.8, 1.0)),
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

fn wrapper_and_card(root: LayerRef<'_>, card_id: NodeId) -> (LayerRef<'_>, LayerRef<'_>) {
    let wrapper = root
        .children()
        .find(|layer| layer.wraps() == Some(card_id))
        .expect("the card's outer shadow wraps its clipped layer");
    let card = find_layer_by_node_id(wrapper, card_id).expect("card layer inside the wrapper");
    (wrapper, card)
}

#[test]
fn a_draw_before_the_graphics_layer_wraps_the_clipped_layer_in_the_parents_space() {
    let label = Rc::new(RefCell::new(None));
    let card_id = Rc::new(RefCell::new(None));
    let mut composition = wrapped_card_composition(label, Rc::clone(&card_id));
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
    let graph = built(&applier, root).expect("graph");
    applier.clear_runtime_handle();
    let card_id = card_id.borrow().expect("card id");
    let graph_root = graph.root_layer().expect("a scene");
    let (wrapper, card) = wrapper_and_card(graph_root, card_id);

    assert_eq!(wrapper.node_id(), None);
    assert!(!wrapper.graphics_layer().clip);
    assert_eq!(
        wrapper.transform_to_parent().map_point(Point::default()),
        Point { x: 20.0, y: 30.0 },
        "the wrapper carries the layout placement alone"
    );
    assert_eq!(
        card.transform_to_parent().map_point(Point::default()),
        Point { x: 5.0, y: 0.0 },
        "the clipped layer keeps only its own graphics-layer transform"
    );
    assert!(card.graphics_layer().clip);
    let items: Vec<ItemRef<'_>> = wrapper.items().collect();
    let [shadow, ItemRef::Layer(_)] = items.as_slice() else {
        panic!(
            "wrapper children must be the outer shadow then the card, got {} children",
            items.len()
        );
    };
    let shadow_is_outer = match shadow {
        ItemRef::Run(run) => run.summary.has_shadow && !run.summary.has_non_shadow,
        ItemRef::Draw(draw) => matches!(draw.primitive, DrawPrimitive::Shadow(_)),
        ItemRef::Layer(_) | ItemRef::Leaf(_) | ItemRef::Text(_) | ItemRef::Shape(_) => false,
    };
    assert!(shadow_is_outer, "the outer draw is the shadow alone");
    assert!(
        card.items().any(|item| match item {
            ItemRef::Run(run) => run.summary.has_non_shadow,
            ItemRef::Shape(shape) => shape.shape_count() > 0,
            ItemRef::Draw(draw) => !matches!(draw.primitive, DrawPrimitive::Shadow(_)),
            ItemRef::Layer(_) | ItemRef::Leaf(_) | ItemRef::Text(_) => false,
        }),
        "the background chained after the layer stays inside it"
    );
}

#[test]
fn a_dirty_wrapped_node_is_rebuilt_as_one_wrapper() {
    let label = Rc::new(RefCell::new(None));
    let card_id = Rc::new(RefCell::new(None));
    let mut composition = wrapped_card_composition(Rc::clone(&label), Rc::clone(&card_id));
    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 120.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, viewport).expect("layout");
    let mut graph = built(&applier, root).expect("graph");
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
    assert!(
        graph
            .update_from_applier(&applier, root, &[(card_id, NodeUpdate::Content)])
            .applied()
    );
    applier.clear_runtime_handle();

    let graph_root = graph.root_layer().expect("a scene");
    let (wrapper, card) = wrapper_and_card(graph_root, card_id);
    assert_eq!(
        wrapper.transform_to_parent().map_point(Point::default()),
        Point { x: 20.0, y: 30.0 }
    );
    assert!(
        !card.children().any(|layer| layer.wraps().is_some()),
        "rebuilding the wrapped node must replace its wrapper, not nest a second one"
    );
    let mut labels = Vec::new();
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
    assert_eq!(labels, vec!["after".to_string()]);
}

#[test]
fn update_graph_from_applier_replaces_dirty_child_layer() {
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
        Rc::new(RefCell::new(None));
    let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = Rc::clone(&state_holder);
    let child_id_holder_for_comp = Rc::clone(&child_id_holder);

    let mut composition = cranpose_ui::run_test_composition(move || {
        let label = cranpose_core::rememberMutableStateOf(|| "before".to_string());
        *state_holder_for_comp.borrow_mut() = Some(label);
        let child_id_holder_for_content = Rc::clone(&child_id_holder_for_comp);
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
    let mut graph = built(&applier, root).expect("initial graph");
    let child_id = child_id_holder
        .borrow()
        .expect("text child id should be captured");
    let initial_transform =
        find_node_transform(graph.root_layer().expect("a scene"), child_id).expect("text child");
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
        graph
            .update_from_applier(&applier, root, &[(child_id, NodeUpdate::Content)])
            .applied(),
        "dirty child should be replaceable from retained applier state"
    );
    applier.clear_runtime_handle();

    let mut labels = Vec::new();
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
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
        find_node_transform(graph.root_layer().expect("a scene"), child_id)
            .expect("updated text child"),
        initial_transform,
        "draw-only child replacement must preserve the retained parent placement transform"
    );
}

// The target content hash of each record below `layer`, with its path and
// node, in tree order.
fn cached_hashes(layer: LayerRef<'_>, path: &str, out: &mut Vec<(String, Option<NodeId>, u64)>) {
    out.push((
        path.to_owned(),
        layer.node_id(),
        layer.target_content_hash(),
    ));
    for (index, child) in layer.children().enumerate() {
        cached_hashes(child, &format!("{path}/{index}"), out);
    }
}

// The hashes an update left in `graph` equal those of a scene built again
// from the same layout tree.
fn assert_dirty_hash_road_matches_full_walk(graph: &Built, fresh: &Built) {
    let mut kept = Vec::new();
    cached_hashes(root_of(&graph.store), "root", &mut kept);
    let mut worked_out = Vec::new();
    cached_hashes(root_of(&fresh.store), "root", &mut worked_out);
    assert_eq!(kept, worked_out, "target content hashes by record path");
}

#[test]
fn dirty_update_leaves_the_hashes_a_full_walk_leaves() {
    let label_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
        Rc::new(RefCell::new(None));
    let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let label_holder_for_comp = Rc::clone(&label_holder);
    let child_id_holder_for_comp = Rc::clone(&child_id_holder);

    let mut composition = cranpose_ui::run_test_composition(move || {
        let label = cranpose_core::rememberMutableStateOf(|| "before".to_string());
        *label_holder_for_comp.borrow_mut() = Some(label);
        let child_id_holder_for_content = Rc::clone(&child_id_holder_for_comp);
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
                        let child_id_holder_for_box = Rc::clone(&child_id_holder_for_content);
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
    let mut graph = built(&applier, root).expect("initial graph");
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
    applier.clear_runtime_handle();
    drop(applier);

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
    let report = graph.update_from_applier(&applier, root, &[(child_id, NodeUpdate::Content)]);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "dirty child update should apply in place, got {:?}",
        report.update
    );
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
}

#[test]
fn dirty_update_with_a_new_row_leaves_the_hashes_a_full_walk_leaves() {
    let rows_holder: Rc<RefCell<Option<cranpose_core::MutableState<usize>>>> =
        Rc::new(RefCell::new(None));
    let column_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let rows_holder_for_comp = Rc::clone(&rows_holder);
    let column_id_holder_for_comp = Rc::clone(&column_id_holder);

    let mut composition = cranpose_ui::run_test_composition(move || {
        let rows = cranpose_core::rememberMutableStateOf(|| 2usize);
        *rows_holder_for_comp.borrow_mut() = Some(rows);
        let column_id_holder_for_content = Rc::clone(&column_id_holder_for_comp);
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
    let mut graph = built(&applier, root).expect("initial graph");
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
    let report = graph.update_from_applier(&applier, root, &[(column_id, NodeUpdate::Content)]);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "structural update should apply in place, got {:?}",
        report.update
    );
    let mut labels = Vec::new();
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
    assert!(
        labels.iter().any(|label| label == "row 2"),
        "the new row must be in the patched graph, got {labels:?}"
    );
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
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
    let sink_for_comp = Rc::clone(&sink);
    let translation = Rc::new(Cell::new(Point { x: 7.0, y: 11.0 }));
    let translation_for_comp = Rc::clone(&translation);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let sink = Rc::clone(&sink_for_comp);
        let translation = Rc::clone(&translation_for_comp);
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
                Spacer(Modifier::empty().size_points(200.0, spacer_before));
                Box(
                    Modifier::empty()
                        .size_points(200.0, 50.0)
                        .graphics_layer(|| GraphicsLayer {
                            translation_x: 5.0,
                            translation_y: -9.0,
                            ..Default::default()
                        })
                        .report_window_rect(Rc::clone(&sink)),
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
        let _graph = built(&applier, root).expect("scene graph");
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
fn scrolled_list_under_a_composited_layer_keeps_the_hashes_a_full_walk_leaves() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = Rc::clone(&scroll_holder);

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
    let report = graph.update_from_applier(&applier, root, &content(&dirty_nodes));
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "scroll update should apply in place, got {:?}",
        report.update
    );
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
}

#[test]
fn update_graph_from_applier_refreshes_scroll_content_offset() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = Rc::clone(&scroll_holder);

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
                Spacer(Modifier::empty().size_points(0.0, 160.0));
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
    let mut graph = built(&applier, root).expect("initial graph");
    let initial_target_top = find_text_top(graph.root_layer().expect("a scene"), "scroll target")
        .expect("initial target text");
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
    let report = graph.update_from_applier(&applier, root, &content(&dirty_nodes));
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "scroll graph update should apply in place, got {:?}",
        report.update
    );
    let updated_target_top = find_text_top(graph.root_layer().expect("a scene"), "scroll target")
        .expect("updated target text");
    assert!(
        updated_target_top < initial_target_top - consumed_scroll * 0.75,
        "partial graph update must refresh scroll content offset: initial_y={initial_target_top} updated_y={updated_target_top} dirty_nodes={dirty_nodes:?}"
    );
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
}

#[test]
fn an_overmarked_ancestor_chain_over_a_scroll_moves_the_rows() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = Rc::clone(&scroll_holder);

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
    let initial_row_top =
        find_text_top(graph.root_layer().expect("a scene"), "row 3").expect("initial row text");

    let scroll_state = scroll_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("scroll state should be captured");
    let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
    assert!(consumed_scroll > 0.0, "test scroll must be consumed");
    let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    let mut cursor = graph.root_layer().expect("a scene");
    dirty_nodes.push(cursor.node_id().expect("root id"));
    while let Some(child) = cursor
        .children()
        .find(|child| child.kind() != RecordKind::Leaf)
    {
        let chain = child.node_id().expect("chain node id");
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
    let report = update_keeping_clean_records(&mut graph, &applier, root, &dirty_nodes);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "chain update should apply in place, got {:?}",
        report.update
    );
    let updated_row_top =
        find_text_top(graph.root_layer().expect("a scene"), "row 3").expect("updated row text");
    assert!(
        updated_row_top < initial_row_top - consumed_scroll * 0.75,
        "the translation must land through the chain: initial_y={initial_row_top} updated_y={updated_row_top}"
    );
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
}

#[test]
fn a_scrolled_container_moves_each_clean_row_by_the_scroll() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let scroll_holder_for_comp = Rc::clone(&scroll_holder);

    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        scrolling_rows_column(scroll_state, tinted_row);
    });

    let (root, mut graph) = initial_scrolled_graph(&mut composition);
    let viewport = scroll_viewport();
    let initial_row_tops: Vec<_> = (0..12)
        .map(|index| {
            find_text_top(
                graph.root_layer().expect("a scene"),
                &format!("row {index}"),
            )
            .expect("initial row text")
        })
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
    let report = update_keeping_clean_records(&mut graph, &applier, root, &dirty_nodes);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "scroll update should apply in place, got {:?}",
        report.update
    );
    for (index, initial_row_top) in initial_row_tops.into_iter().enumerate() {
        let updated_row_top = find_text_top(
            graph.root_layer().expect("a scene"),
            &format!("row {index}"),
        )
        .expect("every retained row survives scrolling");
        assert!(
            (updated_row_top - (initial_row_top - consumed_scroll)).abs() < 0.01,
            "row {index} did not follow the scroll: initial_y={initial_row_top} updated_y={updated_row_top}"
        );
    }
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
}

#[test]
fn a_scrolled_container_moves_shadowed_rows_with_one_wrapper_each() {
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
        let shape = LayerShape::Rounded(RoundedCornerShape::uniform(12.0));
        let radius = Rc::clone(shadow_radius);
        let recorded_clicks = Rc::clone(clicks);
        let first_text = Rc::clone(first_text);
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
    let scroll_holder_for_comp = Rc::clone(&scroll_holder);
    let shadow_radius = Rc::new(std::cell::Cell::new(6.0));
    let shadow_radius_for_comp = Rc::clone(&shadow_radius);
    let generation_holder = Rc::new(RefCell::new(None));
    let generation_for_comp = Rc::clone(&generation_holder);
    let first_text = Rc::new(std::cell::Cell::new(None));
    let first_text_for_comp = Rc::clone(&first_text);
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let clicks_for_comp = Rc::clone(&clicks);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        let generation = cranpose_core::rememberMutableStateOf(|| 0usize);
        *generation_for_comp.borrow_mut() = Some(generation);
        let mut modifier = Modifier::empty().size_points(240.0, 320.0);
        if wrapped_root {
            modifier = modifier
                .drop_shadow(LayerShape::Rectangle, |scope| {
                    scope.radius = 4.0;
                })
                .graphics_layer(GraphicsLayer::default);
        }
        Column(
            modifier.vertical_scroll(scroll_state, false),
            ColumnSpec::default(),
            {
                let shadow_radius = Rc::clone(&shadow_radius_for_comp);
                let first_text = Rc::clone(&first_text_for_comp);
                let clicks = Rc::clone(&clicks_for_comp);
                move || {
                    shadowed_scroll_rows(generation.value(), &shadow_radius, &first_text, &clicks);
                }
            },
        );
    });

    let (root, mut graph) = initial_scrolled_graph(&mut composition);
    let viewport = scroll_viewport();
    let initial_row_top =
        find_text_top(graph.root_layer().expect("a scene"), "row 3").expect("row text");

    let scroll_state = (*scroll_holder.borrow()).expect("scroll state should be captured");
    let consumed_scroll = scroll_state.dispatch_raw_delta(96.0);
    shadow_radius.set(11.0);
    let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    let row_ids: Vec<_> = find_layer_by_node_id(graph.root_layer().expect("a scene"), root)
        .expect("scroll layer")
        .children()
        .filter_map(LayerRef::wraps)
        .collect();
    dirty_nodes.extend(row_ids.iter().copied());
    dirty_nodes.push(root);
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    let report = update_keeping_clean_records(&mut graph, &applier, root, &dirty_nodes);

    assert!(report.applied(), "got {:?}", report.update);
    let updated_row_top =
        find_text_top(graph.root_layer().expect("a scene"), "row 3").expect("row text");
    assert!(updated_row_top < initial_row_top - consumed_scroll * 0.75);
    let wrappers = find_layer_by_node_id(graph.root_layer().expect("a scene"), root)
        .expect("scroll layer")
        .children()
        .filter(|layer| layer.wraps().is_some())
        .count();
    assert_eq!(wrappers, 12, "every row keeps exactly one wrapper");
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
    let fresh = built(&applier, root).expect("fresh graph");
    applier.clear_runtime_handle();
    let hashes = |layer: LayerRef<'_>| (layer.target_content_hash(), layer.effect_hash());
    assert_eq!(
        hashes(graph.root_layer().expect("a scene")),
        hashes(fresh.root_layer().expect("a scene")),
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
    let report = graph.update_from_applier(&applier, root, &content(&dirty));
    assert!(report.applied());
    let graph_root = graph.root_layer().expect("a scene");
    assert!(find_text_top(graph_root, "changed row").is_some());
    assert!(find_text_top(graph_root, "row 0").is_none());
    for row in row_ids {
        let layer = find_layer_by_node_id(graph_root, row).expect("clickable row");
        let hit = layer.hit_test().expect("row hit target");
        assert_eq!(hit.handlers.pointer_inputs().len(), 1);
        for kind in [PointerEventKind::Down, PointerEventKind::Up] {
            let position = Point { x: 10.0, y: 10.0 };
            hit.handlers.pointer_inputs()[0](
                PointerEvent::new(kind, position, position).with_buttons(
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
    let fresh = built(&applier, root).expect("fresh changed graph");
    applier.clear_runtime_handle();
    assert_eq!(
        collect_text_tops(graph.root_layer().expect("a scene")),
        collect_text_tops(fresh.root_layer().expect("a scene"))
    );
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
}

fn take_redraw_nodes(applier: &mut MemoryApplier, node: NodeId, out: &mut Vec<NodeId>) {
    let mut children = smallvec::SmallVec::<[NodeId; 8]>::new();
    let Ok(entry) = cranpose_core::Applier::get_mut(applier, node) else {
        return;
    };
    entry.collect_children_into(&mut children);
    let redraw = applier
        .with_node::<LayoutNode, _>(node, |layout| {
            let redraw = layout.needs_redraw();
            layout.clear_needs_redraw();
            redraw
        })
        .unwrap_or(false);
    if redraw {
        out.push(node);
    }
    for child in children {
        take_redraw_nodes(applier, child, out);
    }
}

#[test]
fn a_sliding_lazy_window_shows_what_a_fresh_build_shows() {
    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = Rc::clone(&state_holder);
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
    let mut graph = built(&applier, root).expect("initial graph");
    let _ = applier.take_structural_change_parents_attached_to(root);
    let initial_row_top =
        find_text_top(graph.root_layer().expect("a scene"), "row 4").expect("initial row text");
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
    take_redraw_nodes(&mut applier, root, &mut dirty_nodes);
    dirty_nodes.sort_unstable();
    dirty_nodes.dedup();
    assert!(
        !dirty_nodes.is_empty(),
        "a lazy scroll must mark the list dirty"
    );

    let report = update_keeping_clean_records(&mut graph, &applier, root, &dirty_nodes);
    let fresh = built(&applier, root).expect("fresh comparison graph");
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "the boundary frame must apply in place, got {:?}",
        report.update
    );
    assert_eq!(
        collect_text_tops(graph.root_layer().expect("a scene")),
        collect_text_tops(fresh.root_layer().expect("a scene")),
        "rows crossed the window boundary: the patched scene must show what a fresh build shows"
    );
    let updated_row_top =
        find_text_top(graph.root_layer().expect("a scene"), "row 4").expect("updated row text");
    assert!(
        updated_row_top < initial_row_top - 60.0,
        "the retained rows must move with the scroll: \
         initial_y={initial_row_top} updated_y={updated_row_top}"
    );
    assert_dirty_hash_road_matches_full_walk(&graph, &built(&applier, root).expect("fresh graph"));
}

fn collect_layer_origins(layer: LayerRef<'_>) -> Vec<(Option<NodeId>, Option<NodeId>, Point)> {
    let mut origins = vec![(layer.node_id(), layer.wraps(), layer.origin_in_parent())];
    for child in layer
        .children()
        .filter(|child| child.kind() != RecordKind::Leaf)
    {
        origins.extend(collect_layer_origins(child));
    }
    origins
}

fn collect_text_tops(layer: LayerRef<'_>) -> std::collections::BTreeMap<String, i64> {
    fn walk(
        layer: LayerRef<'_>,
        transform: ProjectiveTransform,
        out: &mut std::collections::BTreeMap<String, i64>,
    ) {
        for item in layer.items() {
            match item {
                ItemRef::Text(text) => {
                    let quad = transform.map_rect(text.rect);
                    let top = quad
                        .iter()
                        .map(|point| point[1])
                        .fold(f32::INFINITY, f32::min);
                    if top.is_finite() {
                        out.insert(
                            text.render_text.text().to_owned(),
                            (top * 10.0).round() as i64,
                        );
                    }
                }
                ItemRef::Layer(child) | ItemRef::Leaf(child) => {
                    walk(child, child.transform_to_parent().then(transform), out);
                }
                ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => {}
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
        let state_holder_for_comp = Rc::clone(&state_holder);
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
        let mut graph = built(&applier, root).expect("initial graph");
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
        take_redraw_nodes(&mut applier, root, &mut dirty_nodes);
        dirty_nodes.sort_unstable();
        dirty_nodes.dedup();
        let report = update_keeping_clean_records(&mut graph, &applier, root, &dirty_nodes);
        assert!(report.applied(), "delta {delta}: boundary frame must apply");

        let fresh = built(&applier, root).expect("fresh comparison graph");
        applier.clear_runtime_handle();

        let patched_texts = collect_text_tops(graph.root_layer().expect("a scene"));
        let fresh_texts = collect_text_tops(fresh.root_layer().expect("a scene"));
        assert_eq!(
            patched_texts, fresh_texts,
            "delta {delta}: the patched scene must show exactly what a \
             fresh build shows (dirty={dirty_nodes:?})"
        );
        assert_eq!(
            collect_layer_origins(graph.root_layer().expect("a scene")),
            collect_layer_origins(fresh.root_layer().expect("a scene")),
            "delta {delta}: patched layers must sit where a fresh build places them"
        );
    }
}

#[test]
fn update_graph_from_applier_keeps_parent_content_offset_for_dirty_scroll_child() {
    let label_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
        Rc::new(RefCell::new(None));
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let child_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let label_holder_for_comp = Rc::clone(&label_holder);
    let scroll_holder_for_comp = Rc::clone(&scroll_holder);
    let child_id_holder_for_comp = Rc::clone(&child_id_holder);

    let mut composition = cranpose_ui::run_test_composition(move || {
        let label = cranpose_core::rememberMutableStateOf(|| "scrolled child before".to_string());
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *label_holder_for_comp.borrow_mut() = Some(label);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        let child_id_holder_for_content = Rc::clone(&child_id_holder_for_comp);
        Column(
            Modifier::empty()
                .size_points(260.0, 90.0)
                .vertical_scroll(scroll_state, false),
            ColumnSpec::default(),
            move || {
                Spacer(Modifier::empty().size_points(0.0, 24.0));
                let child_id = Text(label, Modifier::empty(), TextStyle::default());
                *child_id_holder_for_content.borrow_mut() = Some(child_id);
                Spacer(Modifier::empty().size_points(0.0, 220.0));
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
    let mut graph = built(&applier, root).expect("scrolled graph");
    let child_id = child_id_holder
        .borrow()
        .expect("text child id should be captured");
    let scrolled_transform = find_node_transform(graph.root_layer().expect("a scene"), child_id)
        .expect("scrolled child");
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
    let report = graph.update_from_applier(&applier, root, &[(child_id, NodeUpdate::Content)]);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "dirty child graph update should apply, got {:?}",
        report.update
    );
    let updated =
        find_node_transform(graph.root_layer().expect("a scene"), child_id).expect("updated child");
    assert_eq!(
        updated, scrolled_transform,
        "dirty child replacement inside a scrolled parent must keep the parent's content-offset transform"
    );
    let mut labels = Vec::new();
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
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
    let alpha_holder_for_comp = Rc::clone(&alpha_holder);
    let scroll_holder_for_comp = Rc::clone(&scroll_holder);
    let underlay_id_holder_for_comp = Rc::clone(&underlay_id_holder);
    let overlay_id_holder_for_comp = Rc::clone(&overlay_id_holder);

    let mut composition = cranpose_ui::run_test_composition(move || {
        let alpha = cranpose_core::rememberMutableStateOf(|| 1.0f32);
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *alpha_holder_for_comp.borrow_mut() = Some(alpha);
        *scroll_holder_for_comp.borrow_mut() = Some(scroll_state);
        let underlay_id_holder_for_content = Rc::clone(&underlay_id_holder_for_comp);
        let overlay_id_holder_for_content = Rc::clone(&overlay_id_holder_for_comp);
        Column(
            Modifier::empty()
                .size_points(260.0, 120.0)
                .vertical_scroll(scroll_state, false),
            ColumnSpec::default(),
            move || {
                Spacer(Modifier::empty().size_points(0.0, 180.0));
                cranpose_ui::Box(
                    Modifier::empty().size_points(188.0, 88.0),
                    cranpose_ui::BoxSpec::default(),
                    {
                        let underlay_id_holder_for_box = Rc::clone(&underlay_id_holder_for_content);
                        let overlay_id_holder_for_box = Rc::clone(&overlay_id_holder_for_content);
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
                Spacer(Modifier::empty().size_points(0.0, 280.0));
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
    let mut graph = built(&applier, root).expect("scrolled graph");
    applier.clear_runtime_handle();
    drop(applier);

    let underlay_id = underlay_id_holder
        .borrow()
        .expect("underlay id should be captured");
    let overlay_id = overlay_id_holder
        .borrow()
        .expect("overlay id should be captured");
    let scrolled_underlay_origin =
        find_layer_origin(graph.root_layer().expect("a scene"), underlay_id)
            .expect("underlay origin");
    let scrolled_overlay_origin =
        find_layer_origin(graph.root_layer().expect("a scene"), overlay_id)
            .expect("overlay origin");
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
    let report = graph.update_from_applier(&applier, root, &[(overlay_id, NodeUpdate::Content)]);
    applier.clear_runtime_handle();

    assert!(
        report.applied(),
        "dirty overlay graph update should apply, got {:?}",
        report.update
    );
    let updated_underlay_origin =
        find_layer_origin(graph.root_layer().expect("a scene"), underlay_id)
            .expect("updated underlay origin");
    let updated_overlay_origin =
        find_layer_origin(graph.root_layer().expect("a scene"), overlay_id)
            .expect("updated overlay origin");
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
    let offset_holder_for_comp = Rc::clone(&offset_holder);
    let node_id_holder_for_comp = Rc::clone(&node_id_holder);

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
    let mut graph = built(&applier, root).expect("initial graph");
    let node_id = node_id_holder
        .borrow()
        .expect("graphics layer node id should be captured");
    let initial_origin = find_layer_by_node_id(graph.root_layer().expect("a scene"), node_id)
        .expect("initial graphics layer")
        .transform_to_parent()
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
    let report = graph.update_from_applier(&applier, root, &[(node_id, NodeUpdate::Layer)]);
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

    let updated_origin = find_layer_by_node_id(graph.root_layer().expect("a scene"), node_id)
        .expect("updated graphics layer")
        .transform_to_parent()
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
    let offset_holder_for_comp = Rc::clone(&offset_holder);
    let node_id_holder_for_comp = Rc::clone(&node_id_holder);

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
    let mut graph = built(&applier, root).expect("initial graph");
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
    let report = graph.update_from_applier(&applier, root, &[(node_id, NodeUpdate::Layer)]);
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
    let empty: cranpose_ui::DrawCommandFn = Rc::new(|_: &mut DrawScopeDefault| {});
    let commands = [
        DrawCommand::Behind(Rc::clone(&empty)),
        DrawCommand::WithContent(Rc::clone(&empty)),
        DrawCommand::Overlay(empty),
    ];
    let mut store = SceneStore::new();
    store.begin_update();
    let root = root_record(&mut store, NodeId(42));
    let mut items =
        vec![store.add_run(DrawRunNode::new(PrimitivePhase::BeforeChildren, Vec::new()))];
    for (placement, phase) in [
        (DrawPlacement::Behind, PrimitivePhase::BeforeChildren),
        (DrawPlacement::Overlay, PrimitivePhase::AfterChildren),
    ] {
        let node = NodeDraws {
            parent: root,
            commands: NodeCommands {
                node_id: NodeId(42),
                commands: &commands,
                first_index: 3,
            },
            target: DrawTarget {
                size: Size::default(),
                motion_context_animated: false,
                paint: None,
            },
        };
        append_draw_nodes(
            &mut store,
            &node,
            (&mut items, &mut OwnDraws::none()),
            DrawPass::Record(placement),
            phase,
        );
    }
    let actual: Vec<_> = items
        .iter()
        .map(|&item| {
            let run = store.run(item);
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
                Some((NodeId(42), 3, DrawPlacement::Behind))
            ),
            (
                PrimitivePhase::BeforeChildren,
                Some((NodeId(42), 4, DrawPlacement::Behind))
            ),
            (
                PrimitivePhase::AfterChildren,
                Some((NodeId(42), 4, DrawPlacement::Overlay))
            ),
            (
                PrimitivePhase::AfterChildren,
                Some((NodeId(42), 5, DrawPlacement::Overlay))
            ),
        ]
    );
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
    let graph = built(&applier, root).expect("lazy column graph");
    applier.clear_runtime_handle();

    assert_eq!(
        find_text_motion(graph.root_layer().expect("a scene"), "LazyMotion"),
        Some(None)
    );
}

#[test]
fn scrolled_lazy_column_item_text_keeps_unspecified_motion_at_rest() {
    use std::{cell::RefCell, rc::Rc};

    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = Rc::clone(&state_holder);
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
    let graph = built(&applier, root).expect("lazy column graph");
    let child_debug: Vec<String> = active_children
        .iter()
        .map(|&child_id| {
            if let Ok(summary) = applier.with_node::<LayoutNode, _>(child_id, |node| {
                format!(
                    "layout#{child_id} placed={} text={:?} children={:?}",
                    node.layout_state().is_placed(),
                    node.modifier_slices_snapshot()
                        .text_content()
                        .map(|text| text.to_string()),
                    node.children().to_vec()
                )
            }) {
                summary
            } else if let Ok(summary) =
                applier.with_node::<SubcomposeLayoutNode, _>(child_id, |node| {
                    format!(
                        "subcompose#{child_id} placed={} active_children={:?}",
                        node.layout_state().is_placed(),
                        node.with_active_children(<[NodeId]>::to_vec)
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
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
    assert_eq!(
        find_text_motion(
            graph.root_layer().expect("a scene"),
            &format!("LazyMotion {first_index}")
        ),
        Some(None),
        "graph labels after scroll: {labels:?}, active_children={active_children:?}, child_debug={child_debug:?}"
    );
}

#[test]
fn scrolled_lazy_column_render_graph_holds_only_viewport_rows() {
    use std::{cell::RefCell, rc::Rc};

    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = Rc::clone(&state_holder);
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
    let graph = built(&applier, root).expect("lazy column graph");
    applier.clear_runtime_handle();

    let visible_indices: Vec<_> = list_state
        .layout_info()
        .visible_items_info
        .iter()
        .map(|item| item.index)
        .collect();
    let mut labels = Vec::new();
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);

    assert_eq!(
        visible_indices,
        vec![4, 5, 6],
        "test setup expects exactly three viewport-visible rows"
    );
    assert_eq!(
        labels,
        ["WarmRow 4", "WarmRow 5", "WarmRow 6"],
        "rows kept composed beyond the viewport stay out of the render graph; active_children={active_children:?}"
    );
}

#[test]
fn scrolled_lazy_column_uses_visible_item_offset_as_snap_anchor_offset() {
    use std::{cell::RefCell, rc::Rc};

    let state_holder: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let state_holder_for_comp = Rc::clone(&state_holder);
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
    let graph = built(&applier, root).expect("lazy column graph");
    applier.clear_runtime_handle();

    let layout_info = list_state.layout_info();
    let first_visible_offset = layout_info
        .visible_items_info
        .first()
        .expect("lazy layout should expose visible item info")
        .offset;
    let snap_offset = find_translated_content_offset(graph.root_layer().expect("a scene"))
        .expect("lazy list graph should include translated content context");

    assert!(
        (snap_offset.y - first_visible_offset).abs() <= 0.001,
        "lazy snap offset must follow the visible content origin; snap_offset={snap_offset:?} first_visible_offset={first_visible_offset}"
    );
}

const WRAPPING_BODY: &str = "fed back картица scored fp32 износ once paper fed Vision dropped \
     fed widest the strip mask prompt mask threshold Vision on датум instance mask износ Apple";

fn find_text_box<'a>(node: &'a LayoutBox, value: &str) -> Option<&'a LayoutBox> {
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
        .find_map(|child| find_text_box(child, value))
}

fn find_text_primitive(
    layer: LayerRef<'_>,
    matches: &impl Fn(&TextPrimitiveNode) -> bool,
) -> Option<TextPrimitiveNode> {
    layer.items().find_map(|item| match item {
        ItemRef::Text(text) if matches(text) => Some(text.clone()),
        ItemRef::Layer(child) | ItemRef::Leaf(child) => find_text_primitive(child, matches),
        ItemRef::Text(_) | ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => None,
    })
}

#[test]
fn wrapped_paragraph_paints_the_height_it_measured() {
    const BODY: &str = WRAPPING_BODY;
    const FOLLOWING: &str = "FOLLOWING SIBLING";

    let app_context = cranpose_ui::AppContext::new();
    app_context.enter(|| {
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
        cranpose_ui::text::set_text_fonts(
            crate::text_fonts::TextFonts::from_fonts_or_default(&[]).resolver(),
        );

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

        let body_box = find_text_box(layout.root(), BODY).expect("measured paragraph box");
        let following_box = find_text_box(layout.root(), FOLLOWING).expect("measured sibling box");
        let measured_height = body_box.rect.height;
        let following_top = following_box.rect.y;
        assert!(
            measured_height > 60.0,
            "test setup expects a genuinely multi-line paragraph, got {measured_height}"
        );

        let graph = built(&applier, root).expect("render graph");
        applier.clear_runtime_handle();

        fn squashed(value: &str) -> String {
            value.chars().filter(|c| !c.is_whitespace()).collect()
        }
        let painted = find_text_primitive(graph.root_layer().expect("a scene"), &|text| {
            squashed(text.render_text.text()) == squashed(BODY)
        })
        .expect("painted paragraph");

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

// The text of each line of `layout` of `text`, an ellipsized line ending
// in `…`.
fn layout_lines(layout: &cranpose_text::TextLayout, text: &str) -> Vec<String> {
    (0..layout.line_count())
        .map(|line| {
            let shown = text
                .get(layout.line_start(line)..layout.line_visible_end(line))
                .unwrap_or_default();
            if layout.is_line_ellipsized(line) {
                format!("{shown}\u{2026}")
            } else {
                shown.to_owned()
            }
        })
        .collect()
}

struct PaintedParagraph {
    measured: Vec<String>,
    rewrapped_at_available_width: Vec<String>,
    painted_from_applier: Vec<String>,
}

impl PaintedParagraph {
    fn assert_paints_measured_lines(&self, context: &str) {
        assert_eq!(
            self.painted_from_applier, self.measured,
            "{context}: the scene must paint the lines layout measured"
        );
    }
}

fn paint_paragraph(modifier: Modifier, options: TextOptions, text_width: f32) -> PaintedParagraph {
    paint_body(options.into(), text_width, move || {
        TextWithOptions(
            WRAPPING_BODY.to_string(),
            modifier.clone(),
            TextStyle::default(),
            options,
        );
    })
}

fn paint_text_field(modifier: Modifier, wrap_width: f32) -> PaintedParagraph {
    paint_body(TextLayoutOptions::default(), wrap_width, move || {
        let state = cranpose_core::remember(|| cranpose_ui::TextFieldState::new(WRAPPING_BODY))
            .with(|state| *state);
        cranpose_ui::BasicTextField(state, modifier.clone(), TextStyle::default());
    })
}

fn paint_body(
    options: TextLayoutOptions,
    text_width: f32,
    body: impl Fn() + 'static,
) -> PaintedParagraph {
    const AVAILABLE_WIDTH: f32 = 245.0;

    let body: Rc<dyn Fn()> = Rc::new(body);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let body = Rc::clone(&body);
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || body(),
        );
    });
    cranpose_ui::text::set_text_fonts(
        crate::text_fonts::TextFonts::from_fonts_or_default(&[]).resolver(),
    );

    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: AVAILABLE_WIDTH,
                height: 900.0,
            },
        )
        .expect("layout");
    let body_box = find_text_box(layout.root(), WRAPPING_BODY).expect("measured paragraph box");
    let body_id = body_box.node_id;
    let painted_text = |root: LayerRef<'_>| {
        let painted =
            find_text_primitive(root, &|text| text.node_id == body_id).expect("painted paragraph");
        layout_lines(&painted.layout, painted.render_text.text())
    };
    let graph = built(&applier, root).expect("render graph");
    applier.clear_runtime_handle();
    let prepare_at = |width: f32| {
        let prepared = cranpose_ui::text::prepare_text_layout(
            &AnnotatedString::from(WRAPPING_BODY),
            &TextStyle::default(),
            options,
            Some(width),
        );
        layout_lines(&prepared.layout, WRAPPING_BODY)
    };

    PaintedParagraph {
        measured: prepare_at(text_width),
        rewrapped_at_available_width: prepare_at(AVAILABLE_WIDTH),
        painted_from_applier: painted_text(graph.root_layer().expect("a scene")),
    }
}

#[test]
fn an_ellipsis_paragraph_under_its_line_limit_paints_the_lines_it_measured() {
    let options = TextOptions {
        overflow: TextOverflow::Ellipsis,
        max_lines: Some(8),
        ..TextOptions::default()
    };
    let paragraph = paint_paragraph(Modifier::empty(), options, 245.0);

    assert!(
        paragraph.measured.len() > 1
            && !paragraph
                .measured
                .iter()
                .any(|line| line.ends_with('\u{2026}')),
        "test setup expects the paragraph to wrap and stay under its line limit: {:?}",
        paragraph.measured
    );
    paragraph.assert_paints_measured_lines("ellipsis, max_lines 8");
}

#[test]
fn width_limited_paragraph_paints_the_lines_it_measured() {
    for options in [
        TextOptions::default(),
        TextOptions {
            overflow: TextOverflow::Ellipsis,
            max_lines: Some(4),
            ..TextOptions::default()
        },
    ] {
        let paragraph = paint_paragraph(Modifier::empty().width(150.0), options, 150.0);

        paragraph.assert_paints_measured_lines(&format!("width(150), {options:?}"));
    }
}

#[test]
fn width_limited_text_field_paints_the_lines_it_measured() {
    let field = paint_text_field(Modifier::empty().width(150.0), 150.0);

    assert_ne!(
        field.rewrapped_at_available_width, field.measured,
        "test setup expects the column's width to wrap the field differently"
    );
    field.assert_paints_measured_lines("text field, width(150)");
}

#[test]
fn an_unconstrained_text_field_paints_the_lines_it_measured() {
    let field = paint_text_field(Modifier::empty(), 245.0);

    assert!(
        field.measured.len() > 1,
        "test setup expects the field to wrap: {:?}",
        field.measured
    );
    field.assert_paints_measured_lines("text field, unconstrained");
}

struct TestWindow(Size);

impl cranpose_ui::WindowRootDescriptor for TestWindow {
    fn layout_size(&self) -> Size {
        self.0
    }

    fn as_any(&self) -> &dyn Any {
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

fn window_scene_starts_at_the_origin(graph: &Built, why: &str) {
    let root = root_of(&graph.store);
    assert_eq!(
        root.transform_to_parent(),
        layer_transform_to_parent(
            root.local_bounds(),
            Point::default(),
            &GraphicsLayer::default()
        ),
        "{why}"
    );
}

#[test]
fn window_root_subtree_leaves_the_parent_scene_and_starts_its_own() {
    with_window_root_scene(|applier, root, window_node| {
        let primary = built(applier, root).expect("primary graph");
        let mut labels = Vec::new();
        collect_text_labels(primary.root_layer().expect("a scene"), &mut labels);
        assert_eq!(labels, vec!["outside".to_string()]);

        let graph = built(applier, window_node).expect("window graph");
        let graph_root = graph.root_layer().expect("a scene");
        let mut labels = Vec::new();
        collect_text_labels(graph_root, &mut labels);
        assert_eq!(labels, vec!["inside".to_string()]);
        assert_eq!(
            graph_root.node_id().or_else(|| graph_root.wraps()),
            Some(window_node)
        );
        assert_eq!(
            graph_root.local_bounds(),
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
        let mut graph = built(applier, window_node).expect("window graph");
        assert!(
            graph
                .update_from_applier(applier, window_node, &[(window_node, NodeUpdate::Content)])
                .applied(),
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
        let mut graph = built(applier, root).expect("primary graph");
        assert!(
            graph
                .update_from_applier(applier, root, &[(root, NodeUpdate::Content)])
                .applied(),
            "a parent whose child is in a window of its own is patched in place; taking the \
             child for one of its own leaves the scoped update no choice but to rebuild"
        );
        let mut labels = Vec::new();
        collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
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
    let mut graph = built(&applier, root).expect("graph");
    applier.clear_runtime_handle();
    drop(applier);
    let mut labels = Vec::new();
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
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
    let patched = graph
        .update_from_applier(&applier, root, &[(root, NodeUpdate::Content)])
        .applied();
    applier.clear_runtime_handle();
    drop(applier);

    assert!(
        patched,
        "a parent whose child left for a window of its own is patched in place"
    );
    let mut labels = Vec::new();
    collect_text_labels(graph.root_layer().expect("a scene"), &mut labels);
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
    let node: &dyn Any = applier.get_ref(root).expect("root node");
    let node = node
        .downcast_ref::<SubcomposeLayoutNode>()
        .expect("lazy column should be subcompose");
    node.with_active_children(|children| {
        read_node_data(applier, root, |data| {
            assert_eq!(data.children.as_ptr(), children.as_ptr());
        })
        .expect("lazy column scene data");
        children.to_vec()
    })
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

fn initial_scrolled_graph(composition: &mut cranpose_ui::TestComposition) -> (NodeId, Built) {
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, scroll_viewport())
        .expect("initial scroll layout");
    let graph = built(&applier, root).expect("initial graph");
    applier.clear_runtime_handle();
    (root, graph)
}

fn field_window_origin(applier: &mut MemoryApplier, field: NodeId) -> Point {
    applier
        .with_node::<LayoutNode, _>(field, |node| {
            node.modifier_slices_snapshot()
                .text_window()
                .expect("a text field publishes its window origin")
                .get()
                .local_to_window
                .map_point(Point::default())
        })
        .expect("the field is a layout node")
}

#[test]
fn a_field_rebuilt_under_a_scrolled_translated_column_publishes_its_window_origin() {
    let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let field_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let scroll_for_comp = Rc::clone(&scroll_holder);
    let field_for_comp = Rc::clone(&field_holder);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
        *scroll_for_comp.borrow_mut() = Some(scroll_state);
        let field_for_content = Rc::clone(&field_for_comp);
        Column(
            Modifier::empty()
                .size_points(260.0, 90.0)
                .graphics_layer(|| GraphicsLayer {
                    translation_x: 7.0,
                    translation_y: 11.0,
                    ..Default::default()
                })
                .vertical_scroll(scroll_state, false),
            ColumnSpec::default(),
            move || {
                Spacer(Modifier::empty().size_points(0.0, 24.0));
                let field = cranpose_ui::BasicTextField(
                    cranpose_foundation::text::TextFieldState::new("field"),
                    Modifier::empty(),
                    TextStyle::default(),
                );
                *field_for_content.borrow_mut() = Some(field);
                Spacer(Modifier::empty().size_points(0.0, 220.0));
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
    applier.compute_layout(root, viewport).expect("layout");
    let mut graph = built(&applier, root).expect("graph");
    applier.clear_runtime_handle();
    drop(applier);

    let scroll_state = scroll_holder.borrow().expect("scroll state");
    assert!(scroll_state.dispatch_raw_delta(36.0) > 0.0);
    let field = field_holder.borrow().expect("field id");
    let mut dirty_nodes = cranpose_ui::pending_layout_repass_nodes_snapshot();
    dirty_nodes.extend(cranpose_ui::pending_measure_repass_nodes_snapshot());
    dirty_nodes.push(field);

    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("scrolled layout");
    let stale = Point {
        x: -500.0,
        y: -500.0,
    };
    applier
        .with_node::<LayoutNode, _>(field, |node| {
            if let Some(sink) = node.modifier_slices_snapshot().text_window() {
                sink.set(cranpose_ui_graphics::WindowCoordinates {
                    size: Size::default(),
                    local_to_window: ProjectiveTransform::translation(stale.x, stale.y),
                });
            }
        })
        .expect("the field is a layout node");
    let report = graph.update_from_applier(&applier, root, &content(&dirty_nodes));
    assert!(report.applied(), "the scroll patches the graph");
    let patched = field_window_origin(&mut applier, field);
    assert_eq!(
        patched,
        Point {
            x: 7.0,
            y: 11.0 + 24.0 - 36.0
        },
        "the rebuilt field adds up its ancestors' origins, scroll and translation"
    );
    built(&applier, root).expect("fresh graph");
    assert_eq!(field_window_origin(&mut applier, field), patched);
    applier.clear_runtime_handle();
}

fn collect_text_nodes(layer: LayerRef<'_>, texts: &mut Vec<TextPrimitiveNode>) {
    for item in layer.items() {
        match item {
            ItemRef::Text(text) => texts.push(text.clone()),
            ItemRef::Layer(child) | ItemRef::Leaf(child) => collect_text_nodes(child, texts),
            ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => {}
        }
    }
}

#[test]
fn rebuilt_text_nodes_share_their_layouts_render_text_and_style() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        Text("shared once", Modifier::empty(), TextStyle::default());
    });
    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 200.0,
        height: 50.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, viewport).expect("layout");
    let first = built(&applier, root).expect("graph");
    let second = built(&applier, root).expect("rebuilt graph");
    applier.clear_runtime_handle();

    let (mut before, mut after) = (Vec::new(), Vec::new());
    collect_text_nodes(first.root_layer().expect("a scene"), &mut before);
    collect_text_nodes(second.root_layer().expect("a scene"), &mut after);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].render_text.text(), "shared once");
    assert!(
        std::sync::Arc::ptr_eq(&before[0].render_text, &after[0].render_text),
        "a rebuild hands over the prepared layout's copy, not a new one"
    );
    assert!(
        std::sync::Arc::ptr_eq(&before[0].text_style, &after[0].text_style),
        "a rebuild shares the prepared layout's style, not a copy of it"
    );
}

#[test]
fn built_layers_know_whether_their_content_stays_inside() {
    let inside_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let outside_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
    let inside_for_comp = Rc::clone(&inside_holder);
    let outside_for_comp = Rc::clone(&outside_holder);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let inside_for_content = Rc::clone(&inside_for_comp);
        let outside_for_content = Rc::clone(&outside_for_comp);
        Column(
            Modifier::empty().size_points(200.0, 100.0),
            ColumnSpec::default(),
            move || {
                let inside = Rc::clone(&inside_for_content);
                let column = Column(
                    Modifier::empty()
                        .size_points(120.0, 40.0)
                        .background(Color(0.9, 0.9, 0.9, 1.0)),
                    ColumnSpec::default(),
                    move || {
                        Text("inside", Modifier::empty(), TextStyle::default());
                    },
                );
                *inside.borrow_mut() = Some(column);
                let outside = Rc::clone(&outside_for_content);
                let spilling = Column(
                    Modifier::empty().size_points(50.0, 20.0),
                    ColumnSpec::default(),
                    move || {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .offset(40.0, 0.0)
                                .size_points(30.0, 20.0)
                                .background(Color(0.2, 0.2, 0.2, 1.0)),
                            cranpose_ui::BoxSpec::default(),
                            || {},
                        );
                    },
                );
                *outside.borrow_mut() = Some(spilling);
            },
        );
    });
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(
            root,
            Size {
                width: 200.0,
                height: 100.0,
            },
        )
        .expect("layout");
    let graph = built(&applier, root).expect("graph");
    applier.clear_runtime_handle();

    let inside = inside_holder.borrow().expect("inside column");
    let spilling = outside_holder.borrow().expect("spilling column");
    assert!(
        find_layer_by_node_id(graph.root_layer().expect("a scene"), inside)
            .expect("inside layer")
            .draws_within_bounds(),
        "a background and a text inside the column stay inside it"
    );
    assert!(
        !find_layer_by_node_id(graph.root_layer().expect("a scene"), spilling)
            .expect("spilling layer")
            .draws_within_bounds(),
        "a child offset past the column's edge draws past it"
    );
}

#[test]
fn a_dirty_child_that_starts_publishing_window_origins_marks_its_ancestors() {
    let field_holder: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> =
        Rc::new(RefCell::new(None));
    let ids: Rc<RefCell<(Option<NodeId>, Option<NodeId>)>> = Rc::new(RefCell::new((None, None)));
    let field_for_comp = Rc::clone(&field_holder);
    let ids_for_comp = Rc::clone(&ids);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let show_field = cranpose_core::rememberMutableStateOf(|| false);
        *field_for_comp.borrow_mut() = Some(show_field);
        let ids_for_outer = Rc::clone(&ids_for_comp);
        let outer = cranpose_ui::Box(
            Modifier::empty().size_points(240.0, 80.0),
            cranpose_ui::BoxSpec::default(),
            move || {
                let inner = cranpose_ui::Box(
                    Modifier::empty().size_points(200.0, 40.0),
                    cranpose_ui::BoxSpec::default(),
                    move || {
                        if show_field.get() {
                            cranpose_ui::BasicTextField(
                                cranpose_foundation::text::TextFieldState::new("field"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        } else {
                            Text("label", Modifier::empty(), TextStyle::default());
                        }
                    },
                );
                ids_for_outer.borrow_mut().1 = Some(inner);
            },
        );
        ids_for_comp.borrow_mut().0 = Some(outer);
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
    let mut graph = built(&applier, root).expect("initial graph");
    applier.clear_runtime_handle();
    drop(applier);
    let (outer, inner) = *ids.borrow();
    let (outer, inner) = (outer.expect("outer id"), inner.expect("inner id"));
    assert!(
        !find_layer_by_node_id(graph.root_layer().expect("a scene"), outer)
            .expect("outer layer")
            .has_origin_sinks()
    );

    field_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("field switch")
        .set_value(true);
    composition
        .process_invalid_scopes()
        .expect("field recomposition");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(root, viewport)
        .expect("updated layout");
    assert!(
        graph
            .update_from_applier(&applier, root, &[(inner, NodeUpdate::Content)])
            .applied()
    );
    let fresh = built(&applier, root).expect("fresh graph");
    applier.clear_runtime_handle();

    assert!(
        find_layer_by_node_id(fresh.root_layer().expect("a scene"), outer)
            .expect("fresh outer layer")
            .has_origin_sinks(),
        "a full build marks the field's ancestors"
    );
    assert!(
        find_layer_by_node_id(graph.root_layer().expect("a scene"), outer)
            .expect("patched outer layer")
            .has_origin_sinks(),
        "the patched graph marks them too, or a later scroll skips republishing the field's origin"
    );
}

type Holder<T> = Rc<RefCell<Option<T>>>;

fn nested_label_composition(
    label: Holder<cranpose_core::MutableState<String>>,
    padding: Holder<cranpose_core::MutableState<f32>>,
    ids: Holder<(NodeId, NodeId)>,
) -> cranpose_ui::TestComposition {
    cranpose_ui::run_test_composition(move || {
        let text = cranpose_core::rememberMutableStateOf(|| "a".to_string());
        let pad = cranpose_core::rememberMutableStateOf(|| 0.0f32);
        *label.borrow_mut() = Some(text);
        *padding.borrow_mut() = Some(pad);
        let ids = Rc::clone(&ids);
        Column(
            Modifier::empty().size_points(240.0, 200.0),
            ColumnSpec::default(),
            move || {
                // Above the padded box, so the box's growth moves no sibling.
                Text("stable", Modifier::empty(), TextStyle::default());
                let leaf = Rc::new(RefCell::new(None));
                let leaf_seen = Rc::clone(&leaf);
                let padded = cranpose_ui::Box(
                    Modifier::empty().padding(pad.value()),
                    cranpose_ui::BoxSpec::default(),
                    move || {
                        let leaf_seen = Rc::clone(&leaf_seen);
                        cranpose_ui::Box(
                            Modifier::empty().padding(3.0),
                            cranpose_ui::BoxSpec::default(),
                            move || {
                                let leaf_seen = Rc::clone(&leaf_seen);
                                cranpose_ui::Box(
                                    Modifier::empty(),
                                    cranpose_ui::BoxSpec::default(),
                                    move || {
                                        *leaf_seen.borrow_mut() = Some(Text(
                                            text,
                                            Modifier::empty(),
                                            TextStyle::default(),
                                        ));
                                    },
                                );
                            },
                        );
                    },
                );
                if let Some(leaf) = *leaf.borrow() {
                    *ids.borrow_mut() = Some((padded, leaf));
                }
            },
        );
    })
}

#[test]
fn a_node_changed_alone_frame_after_frame_updates_as_a_walk_from_the_root_does() {
    let label: Holder<cranpose_core::MutableState<String>> = Rc::default();
    let padding: Holder<cranpose_core::MutableState<f32>> = Rc::default();
    let ids: Holder<(NodeId, NodeId)> = Rc::default();
    let mut composition =
        nested_label_composition(Rc::clone(&label), Rc::clone(&padding), Rc::clone(&ids));
    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 200.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle.clone());
    applier.compute_layout(root, viewport).expect("layout");
    let mut graph = built(&applier, root).expect("graph");
    applier.clear_runtime_handle();
    drop(applier);
    let (padded, leaf) = ids.borrow().expect("the ids");
    let label = label.borrow().as_ref().copied().expect("the label state");
    let padding = padding
        .borrow()
        .as_ref()
        .copied()
        .expect("the padding state");

    // The text alone, twice; the padded box, which moves the text; the text
    // alone, twice again.
    let frames: [(&str, f32, NodeId, bool); 5] = [
        ("b", 0.0, leaf, false),
        ("c", 0.0, leaf, true),
        ("c", 12.0, padded, false),
        ("d", 12.0, leaf, false),
        ("e", 12.0, leaf, true),
    ];
    for (text, pad, dirty, along_path) in frames {
        label.set_value(text.to_string());
        padding.set_value(pad);
        composition.process_invalid_scopes().expect("recomposition");
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle.clone());
        applier.compute_layout(root, viewport).expect("layout");
        let held = graph
            .walk
            .retained_path
            .as_ref()
            .is_some_and(|path| path.node == dirty);
        assert_eq!(
            held, along_path,
            "frame {text}/{pad}: the update goes along the retained path only \
             when the last one changed this node alone"
        );
        assert!(
            graph
                .update_from_applier(&applier, root, &[(dirty, NodeUpdate::Content)])
                .applied()
        );
        let fresh = built(&applier, root).expect("fresh graph");
        applier.clear_runtime_handle();
        drop(applier);

        let (mut updated_labels, mut fresh_labels) = (Vec::new(), Vec::new());
        collect_text_labels(graph.root_layer().expect("a scene"), &mut updated_labels);
        collect_text_labels(fresh.root_layer().expect("a scene"), &mut fresh_labels);
        assert_eq!(updated_labels, fresh_labels, "frame {text}/{pad}");
        assert_eq!(
            find_layer_origin(graph.root_layer().expect("a scene"), leaf),
            find_layer_origin(fresh.root_layer().expect("a scene"), leaf),
            "frame {text}/{pad}: the text sits where a fresh scene puts it"
        );
        assert_dirty_hash_road_matches_full_walk(&graph, &fresh);
    }
}

#[test]
fn the_same_dirty_nodes_frame_after_frame_update_as_a_fresh_scene_does() {
    let label: Holder<cranpose_core::MutableState<String>> = Rc::default();
    let padding: Holder<cranpose_core::MutableState<f32>> = Rc::default();
    let ids: Holder<(NodeId, NodeId)> = Rc::default();
    let mut composition =
        nested_label_composition(Rc::clone(&label), Rc::clone(&padding), Rc::clone(&ids));
    let root = composition.root().expect("composition root");
    let viewport = Size {
        width: 240.0,
        height: 200.0,
    };
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle.clone());
    applier.compute_layout(root, viewport).expect("layout");
    let mut graph = built(&applier, root).expect("graph");
    applier.clear_runtime_handle();
    drop(applier);
    let (padded, leaf) = ids.borrow().expect("the ids");
    let label = label.borrow().as_ref().copied().expect("the label state");
    let padding = padding
        .borrow()
        .as_ref()
        .copied()
        .expect("the padding state");

    // The padded box and the text it moves change together, the next update
    // with the same two keeps the marks of the one before; the text alone,
    // then the box alone, as many nodes as the update before but another,
    // make the walk mark again.
    let both: &[(NodeId, NodeUpdate)] =
        &[(padded, NodeUpdate::Content), (leaf, NodeUpdate::Content)];
    let text_alone: &[(NodeId, NodeUpdate)] = &[(leaf, NodeUpdate::Content)];
    let box_alone: &[(NodeId, NodeUpdate)] = &[(padded, NodeUpdate::Content)];
    let frames = [
        ("b", 4.0, both),
        ("c", 8.0, both),
        ("d", 8.0, text_alone),
        ("d", 12.0, box_alone),
        ("e", 16.0, both),
        ("f", 20.0, both),
    ];
    for (text, pad, dirty) in frames {
        label.set_value(text.to_string());
        padding.set_value(pad);
        composition.process_invalid_scopes().expect("recomposition");
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle.clone());
        applier.compute_layout(root, viewport).expect("layout");
        assert!(graph.update_from_applier(&applier, root, dirty).applied());
        let fresh = built(&applier, root).expect("fresh graph");
        applier.clear_runtime_handle();
        drop(applier);

        let (mut updated_labels, mut fresh_labels) = (Vec::new(), Vec::new());
        collect_text_labels(graph.root_layer().expect("a scene"), &mut updated_labels);
        collect_text_labels(fresh.root_layer().expect("a scene"), &mut fresh_labels);
        assert_eq!(updated_labels, fresh_labels, "frame {text}/{pad}");
        assert_eq!(
            find_layer_origin(graph.root_layer().expect("a scene"), leaf),
            find_layer_origin(fresh.root_layer().expect("a scene"), leaf),
            "frame {text}/{pad}: the text sits where a fresh scene puts it"
        );
        assert_dirty_hash_road_matches_full_walk(&graph, &fresh);
    }
}

const WINDOW: Size = Size {
    width: 400.0,
    height: 400.0,
};

// A composition laid out in `WINDOW`, with the layout tree and the scene
// built from it.
struct Composed {
    composition: cranpose_ui::TestComposition,
    layout: LayoutTree,
    scene: Built,
}

impl Composed {
    fn new(content: impl FnMut() + 'static) -> Self {
        let mut composition = cranpose_ui::run_test_composition(content);
        let (layout, scene) = Self::lay_out(&mut composition);
        Self {
            composition,
            layout,
            scene,
        }
    }

    // Lays `composition` out and builds its scene from scratch.
    fn lay_out(composition: &mut cranpose_ui::TestComposition) -> (LayoutTree, Built) {
        let root = composition.root().expect("a composition root");
        composition.with_runtime_applier(|applier| {
            let layout = applier.compute_layout(root, WINDOW).expect("layout");
            (layout, built(applier, root).expect("a scene"))
        })
    }

    fn root(&self) -> LayerRef<'_> {
        self.scene.root_layer().expect("a scene")
    }

    // The slices of the first laid-out node that `is_target` picks.
    fn slices(&self, is_target: impl Fn(&ModifierNodeSlices) -> bool) -> Rc<ModifierNodeSlices> {
        fn find(
            node: &LayoutBox,
            is_target: &dyn Fn(&ModifierNodeSlices) -> bool,
        ) -> Option<Rc<ModifierNodeSlices>> {
            if is_target(&node.node_data.modifier_slices) {
                return Some(Rc::clone(&node.node_data.modifier_slices));
            }
            node.children
                .iter()
                .find_map(|child| find(child, is_target))
        }

        find(self.layout.root(), &is_target).expect("a laid-out node the test targets")
    }

    // Turns the wheel over the scroll container to scroll its content up by
    // `distance`, which starts its scroll motion, and builds the scene again.
    fn wheel(&mut self, distance: f32) {
        let scroll = self.slices(ModifierNodeSlices::translated_content_context);
        let handler = scroll
            .pointer_inputs()
            .first()
            .cloned()
            .expect("the scroll container takes the wheel");
        let event = PointerEvent::new(PointerEventKind::Scroll, Point::default(), Point::default())
            .with_scroll_delta(Point::new(0.0, -distance));
        self.composition.with_app_context(|| handler(event.clone()));
        assert!(event.is_consumed(), "the wheel scrolls the container");
        (self.layout, self.scene) = Self::lay_out(&mut self.composition);
    }
}

fn draws_a_rect(modifier: Modifier) -> Modifier {
    modifier.draw_behind(|scope| {
        scope.draw_rect_at(
            Rect {
                x: 3.0,
                y: 4.0,
                width: 20.0,
                height: 8.0,
            },
            Brush::solid(Color::WHITE),
        );
    })
}

// A box of 80 by 50 holding `content`, its own look set by `modifier`.
fn parent_box(modifier: Modifier, content: impl Fn() + Clone + 'static) -> Composed {
    Composed::new(move || {
        cranpose_ui::Box(
            modifier.clone().size_points(80.0, 50.0),
            cranpose_ui::BoxSpec::default(),
            content.clone(),
        );
    })
}

// A child of 40 by 20 at (x, y) that draws a rect.
fn child_box(x: f32, y: f32) {
    cranpose_ui::Box(
        draws_a_rect(Modifier::empty().offset(x, y).size_points(40.0, 20.0)),
        cranpose_ui::BoxSpec::default(),
        || {},
    );
}

// A parent moved by its graphics layer by `translation_x`, over a child at
// (11, 7) that draws a rect.
fn translated_parent(translation_x: f32) -> Composed {
    parent_box(
        Modifier::empty().graphics_layer(move || GraphicsLayer {
            translation_x,
            ..GraphicsLayer::default()
        }),
        || child_box(11.0, 7.0),
    )
}

#[test]
fn parent_translation_changes_layer_transform_but_not_child_local_geometry() {
    let (still, moved) = (translated_parent(0.0), translated_parent(23.5));
    let (still, moved) = (still.root(), moved.root());
    let still_draw = first_run(child_at(still, 0)).primitives().next();
    let moved_draw = first_run(child_at(moved, 0)).primitives().next();

    assert_ne!(
        still.transform_to_parent(),
        moved.transform_to_parent(),
        "parent transform should encode translation"
    );
    assert!(still_draw.is_some(), "the child records its rect");
    assert_eq!(
        still_draw, moved_draw,
        "child local primitive geometry must stay stable under parent translation"
    );
}

#[test]
fn stored_content_hash_ignores_parent_translation() {
    assert_eq!(
        translated_parent(0.0).root().target_content_hash(),
        translated_parent(23.5).root().target_content_hash(),
        "parent rigid motion must not invalidate the subtree content hash"
    );
}

#[test]
fn parent_content_offset_is_encoded_in_child_transform() {
    let child = Rc::new(std::cell::Cell::new(None));
    let composed = parent_box(Modifier::empty().padding_each(13.0, 9.0, 0.0, 0.0), {
        let child = Rc::clone(&child);
        move || {
            child.set(Some(cranpose_ui::Box(
                draws_a_rect(Modifier::empty().offset(11.0, 7.0).size_points(40.0, 20.0)),
                cranpose_ui::BoxSpec::default(),
                || {},
            )));
        }
    });

    let child = child.get().expect("the child composed");
    let top_left = find_node_transform(composed.root(), child)
        .expect("the child is in the scene")
        .map_point(Point::default());
    assert_eq!(top_left, Point { x: 24.0, y: 16.0 });
}

// A column of 160 by 64 scrolled by `offset`, holding `content` above a tall
// spacer.
fn scroll_column(offset: f32, content: impl Fn() + Clone + 'static) -> Composed {
    Composed::new(move || {
        let state = cranpose_core::remember(|| ScrollState::new(offset)).with(|state| *state);
        Column(
            Modifier::empty()
                .size_points(160.0, 64.0)
                .vertical_scroll(state, false),
            ColumnSpec::default(),
            content.clone(),
        );
    })
}

// A text labelled `label` in `style`, 120 by 32 at (11, 7).
fn placed_text(label: &'static str, style: TextStyle) -> impl Fn() + Clone + 'static {
    move || {
        Text(
            label,
            Modifier::empty().offset(11.0, 7.0).size_points(120.0, 32.0),
            style.clone(),
        );
        Spacer(Modifier::empty().height(400.0));
    }
}

#[test]
fn translated_content_offset_changes_visual_position_and_full_surface_hash() {
    let row = || {
        child_box(11.0, 7.0);
        Spacer(Modifier::empty().height(400.0));
    };
    let mut base = scroll_column(0.0, row);
    base.wheel(18.0);
    let mut moved = scroll_column(0.0, row);
    moved.wheel(32.0);
    let rested = scroll_column(18.0, row);

    let (base, moved, rested) = (base.root(), moved.root(), rested.root());
    assert_eq!(
        find_translated_content_offset(base),
        find_translated_content_offset(rested),
        "the wheel scrolls as far as the rested column"
    );
    assert_ne!(
        child_at(base, 0).transform_to_parent(),
        child_at(moved, 0).transform_to_parent(),
        "scroll offset still has to move child content visually"
    );
    assert_eq!(
        base.motion_source_content_hash(),
        moved.motion_source_content_hash(),
        "the source content identity stays stable when only the scroll offset changes"
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
fn update_graph_from_applier_reports_failed_dirty_child_rebuild() {
    let mut composed = translated_parent(0.0);
    let root = composed.composition.root().expect("a composition root");
    let child = child_at(composed.root(), 0)
        .node_id()
        .expect("the child is a node");

    let report = composed.scene.update_from_applier(
        &MemoryApplier::new(),
        root,
        &[(child, NodeUpdate::Content)],
    );

    assert_eq!(
        report,
        GraphUpdateReport {
            update: GraphUpdate::NeedsRebuild(GraphRebuildReason::DirtyLayerUnavailable),
            hit_graph_dirty: true,
            layers_moved: true,
        },
        "dirty child graph updates must not report success when the replacement cannot be rebuilt"
    );
}

#[test]
fn overlay_draw_commands_are_tagged_after_children() {
    let composed = parent_box(
        Modifier::empty().draw_with_content(|scope| {
            scope.draw_rect_at(
                Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 8.0,
                    height: 6.0,
                },
                Brush::solid(Color::WHITE),
            );
            scope.draw_content();
            scope.draw_rect_at(
                Rect {
                    x: 3.0,
                    y: 1.0,
                    width: 5.0,
                    height: 4.0,
                },
                Brush::solid(Color::BLACK),
            );
        }),
        || child_box(4.0, 5.0),
    );

    let items: Vec<ItemRef<'_>> = composed.root().items().collect();
    let [
        ItemRef::Run(behind),
        ItemRef::Layer(_) | ItemRef::Leaf(_),
        ItemRef::Run(overlay),
    ] = items.as_slice()
    else {
        panic!("expected a run before the child node and one after it");
    };

    assert_eq!(behind.phase, PrimitivePhase::BeforeChildren);
    assert_eq!(overlay.phase, PrimitivePhase::AfterChildren);
}

#[test]
fn a_second_build_records_the_same_draws_and_leaves_the_live_scene_unchanged() {
    let mut composed = parent_box(draws_a_rect(Modifier::empty()), || {});
    let recorded = (*first_run(composed.root()).recording).clone();

    let root = composed.composition.root().expect("a composition root");
    let again = built(&composed.composition.applier_mut(), root).expect("a scene");

    assert_eq!(
        *first_run(again.root_layer().expect("a scene")).recording,
        recorded,
        "re-recording must reproduce the recording"
    );
    assert_eq!(
        *first_run(composed.root()).recording,
        recorded,
        "a recording a live scene holds must never be recorded into"
    );
}

#[test]
fn stored_content_hash_changes_when_child_transform_changes() {
    let at = |x: f32| parent_box(Modifier::empty(), move || child_box(x, 5.0));

    assert_ne!(
        at(4.0).root().target_content_hash(),
        at(11.0).root().target_content_hash(),
        "moving a child within the parent must invalidate the parent subtree hash"
    );
}

#[test]
fn stored_effect_hash_tracks_local_effect_only() {
    let base = parent_box(Modifier::empty(), || {});
    let blurred = parent_box(
        Modifier::empty().graphics_layer(|| GraphicsLayer {
            render_effect: Some(RenderEffect::blur(6.0)),
            ..GraphicsLayer::default()
        }),
        || {},
    );

    let (base, blurred) = (base.root(), blurred.root());
    assert_eq!(
        base.target_content_hash(),
        blurred.target_content_hash(),
        "post-processing effect parameters belong to the effect hash, not the content hash"
    );
    assert_ne!(base.effect_hash(), blurred.effect_hash());
}

// The first text drawn in `layer` or below it, with the record that draws it.
fn first_text(layer: LayerRef<'_>) -> Option<(LayerRef<'_>, &TextPrimitiveNode)> {
    layer.items().find_map(|item| match item {
        ItemRef::Text(text) => Some((layer, text)),
        ItemRef::Layer(child) | ItemRef::Leaf(child) => first_text(child),
        ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => None,
    })
}

// The text that `text` lays out with `style` and `options` in a box of
// `width` by 48.
fn text_in_box(
    text: &'static str,
    style: TextStyle,
    options: TextLayoutOptions,
    width: f32,
) -> TextPrimitiveNode {
    let composed = Composed::new(move || {
        cranpose_ui::BasicTextWithOptions(
            text,
            Modifier::empty().size_points(width, 48.0),
            style.clone(),
            options,
        );
    });
    let (_, text) = first_text(composed.root()).expect("a text");
    text.clone()
}

#[test]
fn text_node_preserves_rtl_alignment_and_baseline_shift() {
    let mut style = TextStyle::default();
    style.paragraph_style.text_align = TextAlign::Start;
    style.paragraph_style.text_direction = TextDirection::Rtl;
    style.span_style.baseline_shift = Some(BaselineShift::SUPERSCRIPT);
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        ..Default::default()
    };

    let text = text_in_box("rtl", style, options, 180.0);

    assert!(
        text.rect.x + text.layout.line_left(0) > 0.0,
        "RTL start alignment should shift the line within the available width"
    );
    assert_eq!(
        text.clip, None,
        "a text that fits its box draws uncut, its superscript glyphs above the box too"
    );
}

#[test]
fn text_node_alignment_obeys_physical_and_relative_edges() {
    for direction in [TextDirection::Ltr, TextDirection::Rtl] {
        for (alignment, ltr, rtl) in [
            (TextAlign::Left, 0.0, 0.0),
            (TextAlign::Right, 1.0, 1.0),
            (TextAlign::Center, 0.5, 0.5),
            (TextAlign::Start, 0.0, 1.0),
            (TextAlign::End, 1.0, 0.0),
            (TextAlign::Justify, 0.0, 1.0),
            (TextAlign::Unspecified, 0.0, 1.0),
        ] {
            let mut style = TextStyle::default();
            style.paragraph_style.text_align = alignment;
            style.paragraph_style.text_direction = direction;
            let text = text_in_box("hello", style, TextLayoutOptions::default(), 180.0);
            let fraction = if direction == TextDirection::Rtl {
                rtl
            } else {
                ltr
            };
            let line_width = text.layout.line_right(0) - text.layout.line_left(0);
            let expected = (180.0 - line_width) * fraction;
            let left = text.rect.x + text.layout.line_left(0);
            assert!(
                (left - expected).abs() < 0.001,
                "{alignment:?}, {direction:?}: expected the line at x={expected}, got {left}"
            );
        }
    }
}

// A text in a node of a fixed width lays out as wide as the node, and one
// that fits it draws uncut, as Compose cuts a text only when its layout
// overflows the box: a slant or a stroke past its glyphs' advances shows.
#[test]
fn a_text_that_fits_its_box_lays_out_as_wide_and_draws_uncut() {
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        ..Default::default()
    };
    let text = text_in_box("short", TextStyle::default(), options, 320.0);

    assert_eq!(
        text.rect.width,
        text.layout.width(),
        "a text in a node of a fixed width lays out as wide as the node"
    );
    assert_eq!(text.clip, None, "a text that fits its box draws uncut");
}

// A text whose line overflows its box is cut at the box, with the clip
// padding around it.
#[test]
fn an_overflowing_text_is_cut_at_its_box_with_the_clip_padding() {
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: false,
        max_lines: 1,
        ..Default::default()
    };
    let text = text_in_box(
        "a line far too long for its box",
        TextStyle::default(),
        options,
        60.0,
    );
    let clip = text.clip.expect("an overflowing text is cut");

    assert_eq!(
        clip.width, 62.0,
        "the clip holds the box and the clip padding"
    );
}

#[test]
fn text_field_pan_shifts_glyphs_and_clips_to_field_bounds() {
    const LINE: &str = "a very long single line of text that cannot fit";
    let field_width = 80.0_f32;
    let unpanned = text_in_box(
        LINE,
        TextStyle::default(),
        TextLayoutOptions::default(),
        field_width,
    );
    let field = Composed::new(move || {
        let state =
            cranpose_core::remember(|| cranpose_ui::TextFieldState::new(LINE)).with(|state| *state);
        cranpose_ui::BasicTextFieldWithOptions(
            state,
            Modifier::empty().width(field_width),
            cranpose_ui::BasicTextFieldOptions {
                line_limits: cranpose_foundation::text::TextFieldLineLimits::SingleLine,
                ..cranpose_ui::BasicTextFieldOptions::default()
            },
        );
    });
    let (_, panned) = first_text(field.root()).expect("the field draws its text");
    let pan_offset = field
        .slices(|slices| slices.text_pan_resolver().is_some())
        .text_pan_resolver()
        .expect("a single-line field pans")(field_width);

    assert!(
        pan_offset > 0.0,
        "a caret at the end of a line longer than the field pans it"
    );
    assert_eq!(
        panned.rect.x, -pan_offset,
        "text glyphs must shift left by the pan offset"
    );
    assert!(
        panned.rect.width > field_width,
        "panned single-line text must draw its whole line, not the field's width, got {}",
        panned.rect.width
    );
    assert!(
        unpanned.rect.width <= field_width,
        "text that does not pan draws within the field, got {}",
        unpanned.rect.width
    );
    let clip = panned
        .clip
        .expect("panned text field must clip to field bounds");
    assert!(
        clip.x + clip.width <= field_width + TEXT_CLIP_PAD + f32::EPSILON,
        "clip must not extend past the field bounds, got {clip:?}"
    );
}

// The motion of the text in `composed` and whether the record drawing it
// moves with an animated motion context.
fn text_motion(composed: &Composed) -> (Option<TextMotion>, bool) {
    let (owner, text) = first_text(composed.root()).expect("a text");
    (
        text.text_style.paragraph_style.text_motion,
        owner.motion_context_animated(),
    )
}

#[test]
fn translated_content_context_preserves_descendant_text_motion_when_unspecified() {
    let composed = scroll_column(18.5, placed_text("scrolling", TextStyle::default()));

    assert_eq!(text_motion(&composed), (None, false));
}

#[test]
fn content_offset_without_translated_context_keeps_descendant_text_unspecified() {
    let composed = parent_box(
        Modifier::empty().padding_each(0.0, 18.0, 0.0, 0.0),
        placed_text("offset", TextStyle::default()),
    );

    assert_eq!(
        text_motion(&composed),
        (None, false),
        "content_offset alone must not force text onto the translated-content motion path"
    );
}

#[test]
fn translated_content_context_preserves_effectful_text_motion_when_unspecified() {
    let shadow_style = TextStyle::from_span_style(SpanStyle {
        shadow: Some(cranpose_ui::text::Shadow {
            color: Color::BLACK,
            offset: Point::new(1.0, 2.0),
            blur_radius: 3.0,
        }),
        ..SpanStyle::default()
    });
    let composed = scroll_column(18.5, placed_text("shadow", shadow_style));

    assert_eq!(text_motion(&composed).0, None);
}

#[test]
fn animated_motion_marker_preserves_descendant_text_motion_when_unspecified() {
    let mut composed = scroll_column(0.0, placed_text("lazy", TextStyle::default()));
    composed.wheel(18.0);

    assert_eq!(text_motion(&composed), (None, true));
    assert!(composed.root().motion_context_animated());
}

#[test]
fn explicit_static_text_motion_is_preserved_under_scrolling_context() {
    let static_style = TextStyle::from_paragraph_style(cranpose_ui::text::ParagraphStyle {
        text_motion: Some(TextMotion::Static),
        ..Default::default()
    });
    let mut composed = scroll_column(0.0, placed_text("static", static_style));
    composed.wheel(18.0);

    assert_eq!(
        text_motion(&composed).0,
        Some(TextMotion::Static),
        "explicit text motion must win over inherited scrolling motion context"
    );
}

#[test]
fn rounded_clip_to_bounds_keeps_existing_effect_inside_mask() {
    let existing = RenderEffect::blur(3.0);
    let composed = Composed::new({
        let existing = existing.clone();
        move || {
            let existing = existing.clone();
            cranpose_ui::Box(
                Modifier::empty()
                    .size_points(100.0, 40.0)
                    .graphics_layer(move || GraphicsLayer {
                        render_effect: Some(existing.clone()),
                        ..GraphicsLayer::default()
                    })
                    .rounded_corner_shape(RoundedCornerShape::uniform(10.0))
                    .clip_to_bounds(),
                cranpose_ui::BoxSpec::default(),
                || {},
            );
        }
    });

    let Some(RenderEffect::Chain { first, second }) = composed.root().effect() else {
        panic!("existing effect should chain into rounded clip mask");
    };
    assert_eq!(**first, existing);
    assert!(
        matches!(**second, RenderEffect::Shader { .. }),
        "rounded mask must be the outer effect"
    );
}
