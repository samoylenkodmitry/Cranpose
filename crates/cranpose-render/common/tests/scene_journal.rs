use cranpose_core::NodeId;
use cranpose_render_common::scene_store::{
    JournalCursor, JournalRead, RecordEvent, RecordEvents, RecordKind, RecordSeed, SceneStore,
};

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
    match store.read_journal(cursor) {
        JournalRead::Changes(entries) => Some(
            entries
                .iter()
                .map(|entry| (entry.event, entry.node.expect("a node's record")))
                .collect(),
        ),
        JournalRead::Lost => None,
    }
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
