use cranpose_core::{NodeId, RecordId};
use cranpose_render_common::scene_store::{RecordKind, RecordSeed, SceneStore};

// Adds a layer for `node` under `parent` in the update in progress.
fn layer(store: &mut SceneStore, node: u32, parent: Option<RecordId>) -> RecordId {
    store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(NodeId(node as usize)),
        parent,
    })
}

// A store that holds a root layer with `children` layers, made in one
// update, with the root and the children.
fn store_with_children(children: u32) -> (SceneStore, RecordId, Vec<RecordId>) {
    let mut store = SceneStore::new();
    store.begin_update();
    let root = layer(&mut store, 1, None);
    let children = (0..children)
        .map(|child| layer(&mut store, 2 + child, Some(root)))
        .collect();
    store.finish_update();
    (store, root, children)
}

// The records the latest update of `store` changed, when it lists them.
fn touched(store: &SceneStore) -> Option<Vec<RecordId>> {
    store
        .touched()
        .map(|(_, records)| records.iter().map(|record| record.id).collect())
}

#[test]
fn an_update_names_each_record_it_made_or_changed_once() {
    let (mut store, root, children) = store_with_children(3);
    let before = store.update();

    store.begin_update();
    store.note_content_change(children[1]);
    store.note_property_change(children[1]);
    let made = layer(&mut store, 9, Some(root));
    store.note_content_change(made);
    store.finish_update();

    assert_eq!(touched(&store), Some(vec![children[1], made]));
    assert_eq!(
        store.touched().map(|(since, _)| since),
        Some(before),
        "the list starts from the update before"
    );
}

#[test]
fn an_update_that_changes_nothing_names_no_record() {
    let (mut store, _, children) = store_with_children(2);
    store.begin_update();
    store.note_content_change(children[0]);
    store.finish_update();

    store.begin_update();
    store.finish_update();

    assert_eq!(touched(&store), Some(Vec::new()));
}

#[test]
fn an_update_that_changes_more_records_than_the_list_keeps_names_none() {
    let (mut store, _, children) = store_with_children(300);

    store.begin_update();
    for &child in &children {
        store.note_content_change(child);
    }
    store.finish_update();
    assert_eq!(touched(&store), None);

    store.begin_update();
    store.note_content_change(children[7]);
    store.finish_update();
    assert_eq!(
        touched(&store),
        Some(vec![children[7]]),
        "the next update lists its records again"
    );
}

#[test]
fn a_cleared_store_names_none() {
    let (mut store, _, children) = store_with_children(2);
    store.begin_update();
    store.note_content_change(children[0]);
    store.clear();
    assert_eq!(touched(&store), None);
}
