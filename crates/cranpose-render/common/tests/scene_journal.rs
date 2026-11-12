use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, NodeId, RecordRef};
use cranpose_render_common::{
    graph::{DrawRunNode, LayerNode, PrimitivePhase, RenderNode},
    scene_store::{
        JournalRead, Reach, RecordEvent, RecordEvents, RecordKind, RecordSeed, SceneStore,
        SceneVersion,
    },
};
use cranpose_ui::{BoxSpec, Modifier};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect, Size};

use crate::scene_probe::{initial_graph, update_scene};

// A store holding a root layer, which keeps every change.
fn rooted_store() -> SceneStore {
    let mut store = SceneStore::new();
    store.follow(RecordEvents::ALL);
    insert_root(&mut store);
    store
}

// Inserts a root layer into `store`, which holds no records, in its own
// update.
fn insert_root(store: &mut SceneStore) {
    store.begin_update();
    store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(NodeId(1)),
        parent: None,
    });
    store.finish_update();
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

// A reader of a store's changes: the version of the store it read last.
struct Reader(Option<SceneVersion>);

impl Reader {
    // A reader that read `store` as it is now.
    fn of(store: &SceneStore) -> Self {
        Self(Some(store.version()))
    }

    // The changes of `store` since the last read, `None` when lost.
    fn events(&mut self, store: &SceneStore) -> Option<Vec<(RecordEvent, NodeId)>> {
        let read = self.0.replace(store.version());
        match store.changes_since(read) {
            JournalRead::Changes(entries) => Some(
                entries
                    .iter()
                    .map(|entry| (entry.event, entry.node.expect("a node's record")))
                    .collect(),
            ),
            JournalRead::Lost => None,
        }
    }

    // Whether `store` changed since the last read.
    fn changed(&mut self, store: &SceneStore) -> bool {
        store.changed_since(self.0.replace(store.version()))
    }
}

#[test]
fn a_reader_reads_each_change_once() {
    let mut store = rooted_store();
    let mut reader = Reader::of(&store);

    add_and_remove(&mut store, NodeId(2));

    assert_eq!(
        reader.events(&store),
        Some(vec![
            (RecordEvent::Inserted, NodeId(2)),
            (RecordEvent::Removed, NodeId(2)),
        ])
    );
    assert_eq!(reader.events(&store), Some(Vec::new()));
}

#[test]
fn a_reader_further_behind_than_the_journal_keeps_loses_the_changes() {
    let mut store = rooted_store();
    let mut behind = Reader::of(&store);
    let mut current = Reader::of(&store);

    for node in 2..400 {
        add_and_remove(&mut store, NodeId(node));
        assert!(current.events(&store).is_some());
    }

    assert_eq!(behind.events(&store), None);
    add_and_remove(&mut store, NodeId(400));
    assert_eq!(
        behind.events(&store).map(|events| events.len()),
        Some(2),
        "a reader that lost the changes follows the store again"
    );
}

#[test]
fn a_cleared_store_loses_the_changes() {
    let mut store = rooted_store();
    let mut reader = Reader::of(&store);

    add_and_remove(&mut store, NodeId(2));
    store.clear();

    assert_eq!(reader.events(&store), None);
}

#[test]
fn a_reader_of_another_store_follows_this_one() {
    let first = rooted_store();
    let mut second = rooted_store();
    let mut reader = Reader::of(&first);

    assert_eq!(reader.events(&second), None);
    add_and_remove(&mut second, NodeId(2));
    assert_eq!(reader.events(&second).map(|events| events.len()), Some(2));
}

#[test]
fn a_reader_learns_whether_the_store_changed_while_it_follows_nothing() {
    let mut store = SceneStore::new();
    store.follow(RecordEvents::NONE);
    insert_root(&mut store);
    let mut reader = Reader::of(&store);

    assert!(!reader.changed(&store), "no update came");
    store.begin_update();
    store.finish_update();
    assert!(!reader.changed(&store), "an update wrote nothing");
    add_and_remove(&mut store, NodeId(2));
    assert!(reader.changed(&store), "a record came and went");
    assert!(!reader.changed(&store), "the change was read once");

    for _ in 0..400 {
        store.begin_update();
        store.finish_update();
    }
    assert!(
        !reader.changed(&store),
        "updates that write nothing change nothing, however many came"
    );
    store.clear();
    assert!(reader.changed(&store), "a cleared store changed");
    assert!(
        store.changed_since(None),
        "a reader that read nothing finds a change"
    );
    assert!(
        rooted_store().changed_since(Some(store.version())),
        "a version of another store finds a change"
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
    store.follow(RecordEvents::ALL);
    let mut reader = Reader::of(&store);

    store.describe(&page(Color::WHITE));
    assert_eq!(
        reader.events(&store),
        Some(vec![(RecordEvent::Redrawn(Reach::Finite(DOT)), NodeId(1))]),
        "the page redrew the dot alone"
    );

    store.describe(&page(Color::WHITE));
    assert_eq!(
        reader.events(&store),
        Some(Vec::new()),
        "the same page changes nothing"
    );
}

#[test]
fn a_reader_that_follows_reaches_again_learns_each_redraw_after() {
    let mut store = SceneStore::new();
    store.describe(&page(Color::RED));
    store.follow(RecordEvents::NONE);
    let mut reader = Reader::of(&store);

    store.describe(&page(Color::WHITE));
    assert!(
        reader.changed(&store),
        "a reader that follows nothing learns that the page changed"
    );
    store.describe(&page(Color::WHITE));
    assert!(
        !reader.changed(&store),
        "a reader that follows nothing learns that the same page changed nothing"
    );

    store.follow(RecordEvents::ALL);
    for (colour, label) in [(Color::RED, "back to red"), (Color::WHITE, "white again")] {
        store.describe(&page(colour));
        assert_eq!(
            reader.events(&store),
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
    scene.follow(RecordEvents::ALL);
    let read = Some(scene.store().version());

    swapped.borrow().expect("the order").set(true);
    update_scene(&mut composition, root, viewport, &mut scene);
    let changed = match scene.store().changes_since(read) {
        JournalRead::Changes(entries) => entries
            .iter()
            .any(|entry| entry.event == RecordEvent::Changed && entry.node == Some(root)),
        JournalRead::Lost => false,
    };
    assert!(changed, "the box draws its children in another order");
}

// A box that clips a child wider than it, with a width `width` gives.
fn clipping_box(width: Rc<RefCell<Option<MutableState<f32>>>>) -> cranpose_ui::TestComposition {
    cranpose_ui::run_test_composition(move || {
        let state = cranpose_core::rememberMutableStateOf(|| 40.0f32);
        *width.borrow_mut() = Some(state);
        cranpose_ui::Box(
            Modifier::empty().size(Size::new(100.0, 100.0)),
            BoxSpec::default(),
            move || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .size(Size::new(state.get(), 16.0))
                        .clip_to_bounds(),
                    BoxSpec::default(),
                    || {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .required_size(Size::new(40.0, 16.0))
                                .background(Color::RED),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    })
}

#[test]
fn a_clipping_box_that_shrinks_around_a_still_child_reports_its_subtree_changed() {
    let width = Rc::default();
    let mut composition = clipping_box(Rc::clone(&width));
    let root = composition.root().expect("a root");
    let viewport = Size::new(120.0, 120.0);
    let mut scene = initial_graph(&mut composition, root, viewport);
    scene.follow(RecordEvents::ALL);
    let read = Some(scene.store().version());

    width.borrow().expect("the width").set(30.0);
    update_scene(&mut composition, root, viewport, &mut scene);
    let changed = match scene.store().changes_since(read) {
        JournalRead::Changes(entries) => entries
            .iter()
            .any(|entry| entry.event == RecordEvent::Changed),
        JournalRead::Lost => false,
    };
    assert!(changed, "the box cuts less of its child now");
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

// Adds a layer under the root in its own update and returns its handle.
fn add_child(store: &mut SceneStore, node: NodeId) -> RecordRef {
    store.begin_update();
    let child = store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(node),
        parent: store.root(),
    });
    store.finish_update();
    store.record_ref(child)
}

// Removes the record `held` names in its own update: the last row, which the
// update gives back.
fn remove(store: &mut SceneStore, held: RecordRef) {
    store.begin_update();
    store.remove_subtree(store.resolve(held).expect("a live record"));
    store.finish_update();
}

#[test]
fn a_handle_to_a_removed_last_row_names_no_record_after_a_clear() {
    let mut store = rooted_store();
    let first = add_child(&mut store, NodeId(2));
    remove(&mut store, first);
    let held = add_child(&mut store, NodeId(3));
    remove(&mut store, held);

    store.clear();
    insert_root(&mut store);
    for node in 4..8 {
        add_child(&mut store, NodeId(node));
    }

    assert_eq!(store.resolve(held), None);
}
