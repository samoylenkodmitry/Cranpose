use cranpose_core::NodeId;
use cranpose_render_common::{
    graph::{DrawRunNode, LayerNode, PrimitivePhase, RenderNode},
    scene_store::{
        JournalCursor, JournalRead, Reach, RecordEvent, RecordEvents, RecordKind, RecordSeed,
        SceneStore,
    },
};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect, Size};

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
    let mut cursor = store.open_journal(RecordEvents::ALL_WITHOUT_REACH);

    store.describe(&page(Color::WHITE));
    assert_eq!(
        events(&mut store, &mut cursor),
        Some(vec![(RecordEvent::Redrawn(Reach::Unknown), NodeId(1))]),
        "a reader without reaches learns that the page redrew"
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
