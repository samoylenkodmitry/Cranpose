use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, NodeId};
use cranpose_render_common::{
    graph::{DrawRunNode, LayerNode, PrimitivePhase, RenderNode},
    scene_store::{
        JournalCursor, JournalRead, Reach, RecordEvent, RecordEvents, RecordKind, RecordSeed,
        SceneStore,
    },
};
use cranpose_ui::{BoxSpec, Modifier};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect, Size};

use crate::scene_probe::{initial_graph, update_scene};

// A store holding a root layer.
fn rooted_store() -> SceneStore {
    let mut store = SceneStore::new();
    store.begin_update();
    store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(NodeId(1)),
        parent: None,
    });
    store.finish_update();
    store
}

// Adds a layer for `node` under the root and removes it again, in one update
// each.
fn add_and_remove(store: &mut SceneStore, node: NodeId) {
    store.begin_update();
    let child = store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(node),
        parent: store.root(),
    });
    store.finish_update();
    store.begin_update();
    store.remove_subtree(child);
    store.finish_update();
}

fn events(
    store: &mut SceneStore,
    cursor: &mut JournalCursor,
) -> Option<Vec<(RecordEvent, NodeId)>> {
    store.read_journal(cursor, |_, read| match read {
        JournalRead::Changes(entries) => Some(
            entries
                .iter()
                .map(|entry| (entry.event, entry.node.expect("a node's record")))
                .collect(),
        ),
        JournalRead::Lost => None,
    })
}

#[test]
fn a_reader_reads_each_change_once() {
    let mut store = rooted_store();
    let mut cursor = store.open_journal(RecordEvents::ALL);

    add_and_remove(&mut store, NodeId(2));

    assert_eq!(
        events(&mut store, &mut cursor),
        Some(vec![
            (RecordEvent::Inserted, NodeId(2)),
            (RecordEvent::Removed, NodeId(2)),
        ])
    );
    assert_eq!(events(&mut store, &mut cursor), Some(Vec::new()));
}

#[test]
fn a_reader_further_behind_than_the_journal_keeps_loses_the_changes() {
    let mut store = rooted_store();
    let mut behind = store.open_journal(RecordEvents::ALL);
    let mut current = store.open_journal(RecordEvents::ALL);

    for node in 2..400 {
        add_and_remove(&mut store, NodeId(node));
        assert!(events(&mut store, &mut current).is_some());
    }

    assert_eq!(events(&mut store, &mut behind), None);
    add_and_remove(&mut store, NodeId(400));
    assert_eq!(
        events(&mut store, &mut behind).map(|events| events.len()),
        Some(2),
        "a reader that lost the changes follows the store again"
    );
}

#[test]
fn a_cleared_store_loses_the_changes() {
    let mut store = rooted_store();
    let mut cursor = store.open_journal(RecordEvents::ALL);

    add_and_remove(&mut store, NodeId(2));
    store.clear();

    assert_eq!(events(&mut store, &mut cursor), None);
}

#[test]
fn a_reader_of_another_store_follows_this_one() {
    let mut first = rooted_store();
    let mut second = rooted_store();
    let mut cursor = first.open_journal(RecordEvents::ALL);

    assert_eq!(events(&mut second, &mut cursor), None);
    add_and_remove(&mut second, NodeId(2));
    assert_eq!(
        events(&mut second, &mut cursor).map(|events| events.len()),
        Some(2)
    );
}

const DOT: Rect = Rect {
    x: 40.0,
    y: 30.0,
    width: 8.0,
    height: 8.0,
};

// A page of node 1 that paints its whole box and then a dot of `dot`.
fn page(dot: Color) -> LayerNode {
    let run = |rect, color| {
        RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            vec![DrawPrimitive::Rect {
                rect,
                brush: Brush::solid(color),
                stroke: None,
            }],
        ))
    };
    let size = Size::new(200.0, 100.0);
    LayerNode {
        node_id: Some(NodeId(1)),
        size,
        children: vec![run(Rect::from_size(size), Color::BLACK), run(DOT, dot)],
        ..Default::default()
    }
}

#[test]
fn a_page_described_again_reports_only_the_draw_that_changed() {
    let mut store = SceneStore::new();
    store.describe(&page(Color::RED));
    let mut cursor = store.open_journal(RecordEvents::ALL);

    store.describe(&page(Color::WHITE));
    assert_eq!(
        events(&mut store, &mut cursor),
        Some(vec![(RecordEvent::Redrawn(Reach::Finite(DOT)), NodeId(1))]),
        "the page redrew the dot alone"
    );

    store.describe(&page(Color::WHITE));
    assert_eq!(
        events(&mut store, &mut cursor),
        Some(Vec::new()),
        "the same page changes nothing"
    );
}

#[test]
fn a_reader_that_follows_reaches_again_learns_each_redraw_after() {
    let mut store = SceneStore::new();
    store.describe(&page(Color::RED));
    let mut cursor = store.open_journal(RecordEvents::ANY);

    store.describe(&page(Color::WHITE));
    assert_eq!(
        events(&mut store, &mut cursor).map(|events| events.len()),
        Some(1),
        "a reader of any change learns that the page changed, once"
    );
    store.describe(&page(Color::WHITE));
    assert_eq!(
        events(&mut store, &mut cursor),
        Some(Vec::new()),
        "a reader of any change learns that the same page changed nothing"
    );

    store.follow_journal(&mut cursor, RecordEvents::ALL);
    for (colour, label) in [(Color::RED, "back to red"), (Color::WHITE, "white again")] {
        store.describe(&page(colour));
        assert_eq!(
            events(&mut store, &mut cursor),
            Some(vec![(RecordEvent::Redrawn(Reach::Finite(DOT)), NodeId(1))]),
            "{label}: the page redrew the dot"
        );
    }
}

// A box of two keyed squares, painted in the order `swapped` gives.
fn keyed_squares(swapped: Rc<RefCell<Option<MutableState<bool>>>>) -> cranpose_ui::TestComposition {
    cranpose_ui::run_test_composition(move || {
        let state = cranpose_core::rememberMutableStateOf(|| false);
        *swapped.borrow_mut() = Some(state);
        cranpose_ui::Box(
            Modifier::empty().size(Size::new(100.0, 100.0)),
            BoxSpec::default(),
            move || {
                let colours = [(1, Color::RED), (2, Color::BLUE)];
                let order = if state.get() { [1, 0] } else { [0, 1] };
                for at in order {
                    let (key, colour) = colours[at];
                    cranpose_core::key(key, || {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .size(Size::new(40.0, 40.0))
                                .background(colour),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                }
            },
        );
    })
}

#[test]
fn a_box_whose_children_swap_places_reports_its_subtree_changed() {
    let swapped = Rc::default();
    let mut composition = keyed_squares(Rc::clone(&swapped));
    let root = composition.root().expect("a root");
    let viewport = Size::new(120.0, 120.0);
    let mut scene = initial_graph(&mut composition, root, viewport);
    let mut cursor = scene.open_journal(RecordEvents::ALL);

    swapped.borrow().expect("the order").set(true);
    update_scene(&mut composition, root, viewport, &mut scene);
    let changed = scene.read_journal(&mut cursor, |_, read| match read {
        JournalRead::Changes(entries) => entries
            .iter()
            .any(|entry| entry.event == RecordEvent::Changed && entry.node == Some(root)),
        JournalRead::Lost => false,
    });
    assert!(changed, "the box draws its children in another order");
}

#[test]
fn two_stores_with_the_same_updates_keep_versions_apart() {
    let mut first = rooted_store();
    let mut second = rooted_store();

    add_and_remove(&mut first, NodeId(2));
    add_and_remove(&mut second, NodeId(2));

    assert_ne!(first.version(), second.version());
}

#[test]
fn a_handle_taken_before_a_clear_names_no_record_after_it() {
    let mut store = rooted_store();
    let held = store.record_ref(store.root().expect("a root"));

    store.clear();
    store.begin_update();
    store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(NodeId(9)),
        parent: None,
    });
    store.finish_update();

    assert_eq!(store.resolve(held), None);
}
