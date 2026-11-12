use cranpose_render_common::{
    graph::{
        DrawPrimitiveNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderNode,
    },
    scene_store::{ItemRef, LayerRef, OverlayLayer, RecordChanges, SceneStore},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawPrimitive, Point, Rect, Size};

const WINDOW: Size = Size {
    width: 320.0,
    height: 240.0,
};

const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);
const BLUE: Color = Color(0.0, 0.0, 1.0, 1.0);

fn pill(color: Color) -> PrimitiveNode {
    PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
        primitive: DrawPrimitive::RoundRect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 24.0,
            },
            brush: Brush::Solid(color),
            radii: CornerRadii::uniform(4.0),
            stroke: None,
        },
        clip: None,
    }))
}

fn layer(x: f32, primitives: Vec<PrimitiveNode>) -> OverlayLayer {
    OverlayLayer {
        size: Size::new(80.0, 24.0),
        origin: Point::new(x, 8.0),
        primitives,
    }
}

// What the root and each of its layers were at their last changes.
fn changes(store: &SceneStore) -> Vec<RecordChanges> {
    let root = store.root_layer().expect("the overlay has a root");
    std::iter::once(root)
        .chain(root.children())
        .map(LayerRef::changes)
        .collect()
}

#[test]
fn an_overlay_written_again_changes_only_what_differs() {
    let mut store = SceneStore::new();
    let first = layer(8.0, vec![pill(RED)]);
    let second = layer(100.0, vec![pill(BLUE)]);
    store.describe_overlay(WINDOW, [&first, &second]);
    let before = changes(&store);

    store.describe_overlay(WINDOW, [&first, &second]);
    assert_eq!(changes(&store), before, "equal layers leave every record");

    let recolored = layer(100.0, vec![pill(GREEN)]);
    store.describe_overlay(WINDOW, [&first, &recolored]);
    let after = changes(&store);
    assert_eq!(after[1], before[1], "the first layer draws as before");
    assert_ne!(after[2], before[2], "the second layer draws another color");
    assert_ne!(after[0], before[0], "the root holds a changed layer");
    assert_eq!(store.record_count(), 3, "the records stay in place");
}

#[test]
fn a_resized_window_resizes_the_root_and_leaves_the_layers() {
    let mut store = SceneStore::new();
    let badge = layer(8.0, vec![pill(RED)]);
    store.describe_overlay(WINDOW, [&badge]);
    let before = changes(&store);

    let larger = Size::new(640.0, 480.0);
    store.describe_overlay(larger, [&badge]);
    assert_eq!(store.root_layer().expect("root").size(), larger);
    assert_ne!(changes(&store)[0], before[0]);
    assert_eq!(changes(&store)[1], before[1]);
}

#[test]
fn layers_and_draws_that_go_leave_no_records() {
    let mut store = SceneStore::new();
    let two_draws = layer(8.0, vec![pill(RED), pill(BLUE)]);
    let other = layer(100.0, vec![pill(GREEN)]);
    store.describe_overlay(WINDOW, [&two_draws, &other]);
    assert_eq!(store.record_count(), 3);

    let one_draw = layer(8.0, vec![pill(RED)]);
    store.describe_overlay(WINDOW, [&one_draw]);
    assert_eq!(store.record_count(), 2);
    let root = store.root_layer().expect("root");
    let kept = root.children().next().expect("the first layer stays");
    assert_eq!(kept.item_count(), 1);
    assert!(matches!(kept.item(0), Some(ItemRef::Draw(_))));

    store.describe_overlay(WINDOW, []);
    assert_eq!(store.record_count(), 1);
    assert_eq!(store.root_layer().expect("root").item_count(), 0);
}

// What a record and its subtree hold, as a reader sees them.
fn read(layer: LayerRef<'_>) -> String {
    let items: Vec<String> = layer
        .items()
        .map(|item| match item {
            ItemRef::Layer(child) => read(child),
            ItemRef::Draw(draw) => format!("{draw:?}"),
            ItemRef::Leaf(_) | ItemRef::Text(_) | ItemRef::Run(_) | ItemRef::Shape(_) => {
                unreachable!("an overlay holds layers and draws")
            }
        })
        .collect();
    format!(
        "{:?} {:?} {:?} {:?} {:?} {:?} {:?} [{}]",
        layer.kind(),
        layer.node_id(),
        layer.size(),
        layer.origin_in_parent(),
        layer.transform_to_parent(),
        layer.flags(),
        layer.props().is_some(),
        items.join(", ")
    )
}

fn primitive_entry(phase: PrimitivePhase, node: PrimitiveNode) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry { phase, node })
}

#[test]
fn an_overlay_reads_as_the_layer_tree_that_describes_the_same_layers() {
    let badge = layer(8.0, vec![pill(RED), pill(GREEN)]);
    let inspector = OverlayLayer {
        size: WINDOW,
        origin: Point::default(),
        primitives: vec![pill(BLUE), pill(RED)],
    };
    let mut overlay = SceneStore::new();
    overlay.describe_overlay(WINDOW, [&badge, &inspector]);

    let tree = LayerNode {
        size: WINDOW,
        children: vec![
            RenderNode::Layer(Box::new(LayerNode {
                size: badge.size,
                transform_to_parent: ProjectiveTransform::translation(8.0, 8.0),
                children: vec![
                    primitive_entry(PrimitivePhase::BeforeChildren, pill(RED)),
                    primitive_entry(PrimitivePhase::AfterChildren, pill(GREEN)),
                ],
                ..Default::default()
            })),
            RenderNode::Layer(Box::new(LayerNode {
                size: WINDOW,
                children: vec![
                    primitive_entry(PrimitivePhase::AfterChildren, pill(BLUE)),
                    primitive_entry(PrimitivePhase::AfterChildren, pill(RED)),
                ],
                ..Default::default()
            })),
        ],
        ..Default::default()
    };
    let described = SceneStore::from_layer(&tree);

    assert_eq!(
        read(overlay.root_layer().expect("overlay root")),
        read(described.root_layer().expect("tree root"))
    );
}
