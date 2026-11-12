use cranpose_core::{NodeId, RecordId};
use cranpose_render_common::scene_store::{
    RecordEvents, RecordKind, RecordSeed, SceneStore, SceneVersion,
};

// Adds a layer for `node` under `parent` in the update in progress.
fn layer(store: &mut SceneStore, node: u32, parent: Option<RecordId>) -> RecordId {
    store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(NodeId(node as usize)),
        parent,
    })
}

// A store that keeps the records each update stamps and holds a root layer
// with `children` layers, made in one update, with the root and the
// children.
fn store_with_children(children: u32) -> (SceneStore, RecordId, Vec<RecordId>) {
    let mut store = SceneStore::new();
    store.follow(RecordEvents::STAMPS);
    store.begin_update();
    let root = layer(&mut store, 1, None);
    let children = (0..children)
        .map(|child| layer(&mut store, 2 + child, Some(root)))
        .collect();
    store.finish_update();
    (store, root, children)
}

// The records `store` stamped since `read`, when it keeps them.
fn stamped_since(store: &SceneStore, read: SceneVersion) -> Option<Vec<RecordId>> {
    store
        .stamps_since(Some(read))
        .map(|records| records.iter().map(|record| record.id).collect())
}

#[test]
fn an_update_stamps_each_record_it_made_or_changed_once() {
    let (mut store, root, children) = store_with_children(3);
    let before = store.version();

    store.begin_update();
    store.note_content_change(children[1]);
    store.note_property_change(children[1]);
    let made = layer(&mut store, 9, Some(root));
    store.note_content_change(made);
    store.finish_update();

    assert_eq!(stamped_since(&store, before), Some(vec![children[1], made]));
}

#[test]
fn a_reader_behind_by_two_updates_reads_the_stamps_of_both() {
    let (mut store, _, children) = store_with_children(3);
    let before = store.version();

    for &child in &children[..2] {
        store.begin_update();
        store.note_content_change(child);
        store.finish_update();
    }

    assert_eq!(stamped_since(&store, before), Some(children[..2].to_vec()));
}

#[test]
fn an_update_that_changes_nothing_stamps_no_record() {
    let (mut store, _, children) = store_with_children(2);
    store.begin_update();
    store.note_content_change(children[0]);
    store.finish_update();
    let before = store.version();

    store.begin_update();
    store.finish_update();

    assert_eq!(stamped_since(&store, before), Some(Vec::new()));
}

#[test]
fn an_update_that_stamps_more_records_than_the_journal_keeps_stamps_none() {
    let (mut store, _, children) = store_with_children(300);
    let before = store.version();

    store.begin_update();
    for &child in &children {
        store.note_content_change(child);
    }
    store.finish_update();
    assert_eq!(stamped_since(&store, before), None);

    let after = store.version();
    store.begin_update();
    store.note_content_change(children[7]);
    store.finish_update();
    assert_eq!(
        stamped_since(&store, after),
        Some(vec![children[7]]),
        "the next update keeps its stamps again"
    );
}

#[test]
fn a_cleared_store_loses_its_stamps() {
    let (mut store, _, children) = store_with_children(2);
    let before = store.version();
    store.begin_update();
    store.note_content_change(children[0]);
    store.clear();
    assert_eq!(stamped_since(&store, before), None);
}
