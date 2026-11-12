// The layers a window draws over its scene, such as the frame counter and
// the developer inspector, written as records in a store of their own. A
// description written over the one before keeps each record at its place, so
// the journal logs only what changed.

use cranpose_core::RecordId;
use cranpose_ui::{Point, Size};

use super::{Item, ItemKind, RecordKind, RecordSeed, SceneStore};
use crate::graph::{PrimitiveNode, ProjectiveTransform};

/// A layer of primitives that a window draws over its scene, in window space.
#[derive(Clone, Debug)]
pub struct OverlayLayer {
    /// The layer's size in logical pixels.
    pub size: Size,
    /// Where the layer's top left lies in the window.
    pub origin: Point,
    /// What the layer draws, in draw order.
    pub primitives: Vec<PrimitiveNode>,
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// Writes a root layer of `window` size that holds `layers` in order, in
    /// place of what the store held, in a new update. Records at the same
    /// place stay, so the update changes only what differs.
    pub fn describe_overlay<'a>(
        &mut self,
        window: Size,
        layers: impl IntoIterator<Item = &'a OverlayLayer>,
    ) {
        self.begin_update();
        let old = self.root();
        let root = write_layer(self, None, old, (window, Point::default()));
        write_layers(self, root, layers);
        if let Some(old) = old.filter(|&old| old != root) {
            self.remove_subtree(old);
        }
        self.finish_update();
    }

    // Adds `primitive` for an item.
    pub(super) fn add_primitive(&mut self, primitive: &PrimitiveNode) -> Item {
        match primitive {
            PrimitiveNode::Draw(draw) => self.add_draw((**draw).clone()),
            PrimitiveNode::Text(text) => self.add_text((**text).clone()),
        }
    }

    // Whether `item` holds what `primitive` draws.
    pub(super) fn holds(&self, item: Item, primitive: &PrimitiveNode) -> bool {
        match (item.kind(), primitive) {
            (ItemKind::Draw, PrimitiveNode::Draw(draw)) => **draw == *self.draw(item),
            (ItemKind::Text, PrimitiveNode::Text(text)) => **text == *self.text(item),
            _ => false,
        }
    }
}

// Writes a layer of the size and origin in `(size, origin)` over `old`, when
// it is one, else as a new record, which the caller puts in place of `old`.
fn write_layer(
    store: &mut SceneStore,
    parent: Option<RecordId>,
    old: Option<RecordId>,
    (size, origin): (Size, Point),
) -> RecordId {
    let kept = old.filter(|&old| store.kind(old) == RecordKind::Layer && store.node(old).is_none());
    let record = kept.unwrap_or_else(|| {
        store.insert(RecordSeed {
            kind: RecordKind::Layer,
            node: None,
            parent,
        })
    });
    let resized = store.write_size(record, size.width, size.height);
    store.set_placement(
        record,
        Point::default(),
        Some(ProjectiveTransform::translation(origin.x, origin.y)),
    );
    if resized && kept.is_some() {
        store.note_property_change(record);
    }
    record
}

// Writes `layers` as the children of `root`, each over the record at its
// place among the children.
fn write_layers<'a>(
    store: &mut SceneStore,
    root: RecordId,
    layers: impl IntoIterator<Item = &'a OverlayLayer>,
) {
    let mut items = store.lists.take();
    for (index, layer) in layers.into_iter().enumerate() {
        let old = store
            .items(root)
            .get(index)
            .copied()
            .and_then(Item::as_child);
        let record = write_layer(store, Some(root), old, (layer.size, layer.origin));
        if let Some(old) = old.filter(|&old| old != record) {
            store.remove_subtree(old);
        }
        write_draws(store, record, &layer.primitives);
        items.push(Item::child(record));
    }
    let removed = remove_past(store, root, items.len());
    write_items(store, root, items, removed);
}

// Writes `primitives` as the draws of `record`, each over the equal draw at
// its place among them.
fn write_draws(store: &mut SceneStore, record: RecordId, primitives: &[PrimitiveNode]) {
    let mut items = store.lists.take();
    let mut redrawn = false;
    for (index, primitive) in primitives.iter().enumerate() {
        let item = match store.items(record).get(index).copied() {
            Some(old) if store.holds(old, primitive) => old,
            old => {
                if let Some(old) = old {
                    store.remove_item(old);
                }
                redrawn = true;
                store.add_primitive(primitive)
            }
        };
        items.push(item);
    }
    redrawn |= remove_past(store, record, items.len());
    write_items(store, record, items, redrawn);
}

// Removes the items of `record` from the one at `kept` on; whether there were
// any.
fn remove_past(store: &mut SceneStore, record: RecordId, kept: usize) -> bool {
    let mut index = kept;
    while let Some(old) = store.items(record).get(index).copied() {
        store.remove_item(old);
        index += 1;
    }
    index > kept
}

// Writes `items` as the items of `record`, noting a content change when
// they or the draws behind them differ from what the record held.
fn write_items(store: &mut SceneStore, record: RecordId, items: Vec<Item>, redrawn: bool) {
    if redrawn || store.items(record) != items.as_slice() {
        store.note_content_change(record);
    }
    store.write_items(record, &items);
    store.lists.give(items);
}
