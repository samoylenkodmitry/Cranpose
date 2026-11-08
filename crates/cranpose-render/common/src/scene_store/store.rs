// The records a renderer draws: a tree of layout nodes' layers, wrappers,
// draw groups and leaves, each a row of fixed-size records with its items in
// one span, its other properties and its draws in side arenas.

use std::sync::atomic::{AtomicU16, Ordering};

use cranpose_core::{NodeId, RecordId, RecordRef, SceneUpdate, StoreId, collections::map::HashSet};
use cranpose_ui::{GraphicsLayer, Point, Rect};

use super::{
    GeoRecord, GraphicsLayerId, Item, ItemKind, ItemPool, ItemRef, ItemSpan, Journal,
    JournalCursor, JournalEntry, JournalRead, LayerProps, LayerRef, PropsId, ReachState,
    RecordEvent, RecordFlags, RecordKind, Slab, TransformId, TreeRecord, WarmRecord,
};
use crate::graph::{
    DrawPrimitiveNode, DrawRunNode, LayerNode, ProjectiveTransform, TextPrimitiveNode,
};

/// The scene a renderer draws and the hit targets come from.
#[derive(Clone)]
pub struct SceneStore {
    id: StoreId,
    pub(super) tree: Vec<TreeRecord>,
    pub(super) geo: Vec<GeoRecord>,
    pub(super) warm: Vec<WarmRecord>,
    pub(super) generations: Vec<u32>,
    pub(super) free_rows: Vec<u32>,
    pub(super) items: ItemPool,
    pub(super) props: Slab<LayerProps>,
    pub(super) graphics_layers: Slab<GraphicsLayer>,
    pub(super) transforms: Slab<ProjectiveTransform>,
    pub(super) draws: Slab<DrawPrimitiveNode>,
    pub(super) texts: Slab<TextPrimitiveNode>,
    pub(super) runs: Slab<DrawRunNode>,
    root: Option<RecordId>,
    update: SceneUpdate,
    journal: Journal,
}

/// One state of one store: what it held after one of its updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneVersion {
    store: StoreId,
    update: SceneUpdate,
}

/// What a new record starts as.
#[derive(Clone, Copy, Debug)]
pub struct RecordSeed {
    /// What the record draws for.
    pub kind: RecordKind,
    /// The layout node the record draws for, `None` for an overlay that
    /// draws for none.
    pub node: Option<NodeId>,
    /// The record that holds it, `None` for the root.
    pub parent: Option<RecordId>,
}

// How many journal entries a record row and a store keep past the slowest
// reader: a reader further behind rebuilds what it follows from the store.
const JOURNAL_ROWS_KEPT: usize = 4;
const JOURNAL_ENTRIES_KEPT: usize = 256;

fn next_store_id() -> StoreId {
    static NEXT: AtomicU16 = AtomicU16::new(1);
    loop {
        if let Some(id) = StoreId::new(NEXT.fetch_add(1, Ordering::Relaxed)) {
            return id;
        }
    }
}

impl Default for SceneStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneStore {
    /// An empty store.
    pub fn new() -> Self {
        Self {
            id: next_store_id(),
            tree: Vec::new(),
            geo: Vec::new(),
            warm: Vec::new(),
            generations: Vec::new(),
            free_rows: Vec::new(),
            items: ItemPool::default(),
            props: Slab::default(),
            graphics_layers: Slab::default(),
            transforms: Slab::default(),
            draws: Slab::default(),
            texts: Slab::default(),
            runs: Slab::default(),
            root: None,
            update: SceneUpdate::default(),
            journal: Journal::default(),
        }
    }

    /// The store's identity among the renderers' stores.
    pub fn id(&self) -> StoreId {
        self.id
    }

    /// The store's identity and latest update.
    pub fn version(&self) -> SceneVersion {
        SceneVersion {
            store: self.id,
            update: self.update,
        }
    }

    /// The root record, `None` before the first build.
    pub fn root(&self) -> Option<RecordId> {
        self.root
    }

    /// The latest scene update.
    pub fn update(&self) -> SceneUpdate {
        self.update
    }

    /// Starts a scene update and returns its number.
    pub fn begin_update(&mut self) -> SceneUpdate {
        self.journal
            .trim(JOURNAL_ROWS_KEPT * self.tree.len() + JOURNAL_ENTRIES_KEPT);
        self.update = self.update.next();
        self.update
    }

    /// Starts following the changes of the store's records: the reader's
    /// first read reports the changes after now.
    pub fn open_journal(&mut self) -> JournalCursor {
        self.journal.open(self.id)
    }

    /// Stops following the changes of the store's records.
    pub fn close_journal(&mut self, cursor: &JournalCursor) {
        self.journal.close(cursor);
    }

    /// The changes of the store's records since the reader's last read. A
    /// reader of another store starts following this one, and learns that
    /// its changes are lost.
    pub fn read_journal(&mut self, cursor: &mut JournalCursor) -> JournalRead<'_> {
        self.journal.read(self.id, cursor)
    }

    // Logs `event` for `id` as the record is now.
    fn log(&mut self, event: RecordEvent, id: RecordId) {
        let index = id.index();
        self.journal.log(JournalEntry {
            update: self.update,
            event,
            kind: self.tree[index].kind,
            record: RecordRef {
                id,
                generation: self.generations[index],
            },
            parent: self.tree[index].parent,
            node: self.warm[index].node,
        });
    }

    /// Works out what the walks read of the records the update in progress
    /// changed: whether a subtree handles pointers, holds a space or draws
    /// within its bounds. Readers read the store after it.
    pub fn finish_update(&mut self) {
        if let Some(root) = self.root
            && self.tree[root.index()].subtree_rev == self.update
        {
            super::facts::refresh(self, root);
        }
    }

    /// A store that holds `root` and the tree beneath it as records.
    pub fn from_layer(root: &LayerNode) -> Self {
        let mut store = Self::new();
        store.describe(root);
        store
    }

    /// Writes `root` and the tree beneath it as records in place of what the
    /// store held, in a new update.
    pub fn describe(&mut self, root: &LayerNode) {
        self.begin_update();
        self.clear();
        super::spec::write_layer(self, None, root);
        self.finish_update();
    }

    /// Replaces `nodes` with the layout nodes whose draws the store holds,
    /// keeping its capacity.
    pub fn collect_visual_observation_nodes(&self, nodes: &mut HashSet<NodeId>) {
        fn collect(layer: LayerRef<'_>, nodes: &mut HashSet<NodeId>) {
            if let Some(node_id) = layer.node_id() {
                nodes.insert(node_id);
            }
            for item in layer.items() {
                match item {
                    ItemRef::Run(run) => {
                        if let Some(command) = run.command {
                            nodes.insert(command.node_id);
                        }
                    }
                    ItemRef::Layer(child) | ItemRef::Leaf(child) => collect(child, nodes),
                    ItemRef::Draw(_) | ItemRef::Text(_) => {}
                }
            }
        }

        nodes.clear();
        if let Some(root) = self.root_layer() {
            collect(root, nodes);
        }
    }

    // Forgets the hashes the records keep, so that they are worked out again
    // when asked for.
    #[cfg(test)]
    pub(crate) fn forget_hashes(&self) {
        for props in self.props.values() {
            props.cache_hashes.set(None);
        }
    }

    /// The record `id` as its readers see it.
    pub fn layer(&self, id: RecordId) -> LayerRef<'_> {
        LayerRef::new(self, id)
    }

    /// The root record as its readers see it.
    pub fn root_layer(&self) -> Option<LayerRef<'_>> {
        self.root.map(|root| self.layer(root))
    }

    /// Estimated heap memory the store holds, in bytes, for memory reports.
    pub fn heap_bytes(&self) -> usize {
        super::heap_bytes::store_heap_bytes(self)
    }

    /// How many records and draws the store holds: each record counts once,
    /// each primitive of a draw run on its own.
    pub fn node_count(&self) -> usize {
        self.record_count()
            + self.draws.values().count()
            + self.texts.values().count()
            + self.runs.values().map(DrawRunNode::len).sum::<usize>()
    }

    /// How many records the store holds.
    pub fn record_count(&self) -> usize {
        self.tree.len() - self.free_rows.len()
    }

    /// Removes every record.
    pub fn clear(&mut self) {
        self.tree.clear();
        self.geo.clear();
        self.warm.clear();
        self.generations.clear();
        self.free_rows.clear();
        self.items.clear();
        self.props.clear();
        self.graphics_layers.clear();
        self.transforms.clear();
        self.draws.clear();
        self.texts.clear();
        self.runs.clear();
        self.root = None;
        self.journal.clear(self.update);
    }

    /// Makes a record from `seed`, changed in the update in progress, with no
    /// items, at the origin of its parent and of zero size. Its ancestors
    /// take the update as their subtree's; the caller adds it to its
    /// parent's items.
    pub fn insert(&mut self, seed: RecordSeed) -> RecordId {
        let update = self.update;
        let tree = TreeRecord {
            parent: seed.parent,
            items: ItemSpan::default(),
            kind: seed.kind,
            reach_state: ReachState::Stale,
            flags: RecordFlags::default(),
            content_rev: update,
            place_rev: update,
            subtree_rev: update,
            transform: None,
        };
        let warm = WarmRecord {
            node: seed.node,
            props: None,
            content_offset: [0.0; 2],
        };
        let id = match self.free_rows.pop() {
            Some(row) => {
                let index = row as usize;
                self.tree[index] = tree;
                self.geo[index] = GeoRecord::default();
                self.warm[index] = warm;
                RecordId::from_index(index).expect("a freed row was a record")
            }
            None => {
                self.tree.push(tree);
                self.geo.push(GeoRecord::default());
                self.warm.push(warm);
                self.generations.push(0);
                RecordId::from_index(self.tree.len() - 1).expect("fewer than 2^32 records")
            }
        };
        match seed.parent {
            Some(parent) => self.note_subtree_change(Some(parent)),
            None => self.root = Some(id),
        }
        self.log(RecordEvent::Inserted, id);
        id
    }

    /// Removes `id`, its draws, its properties and every record beneath it.
    pub fn remove_subtree(&mut self, id: RecordId) {
        self.log(RecordEvent::Removed, id);
        let index = id.index();
        let span = self.tree[index].items;
        for position in 0..span.len() {
            let item = self.items.span(span)[position];
            self.remove_item(item);
        }
        self.items.free_span(span);
        self.clear_props(id);
        if let Some(transform) = self.tree[index].transform.take() {
            self.transforms.remove(transform.index());
        }
        self.generations[index] = self.generations[index].wrapping_add(1);
        self.free_rows
            .push(u32::try_from(index).expect("fewer than 2^32 records"));
        if self.root == Some(id) {
            self.root = None;
        }
    }

    /// Removes what `item` holds: a child record's subtree or a draw.
    pub fn remove_item(&mut self, item: Item) {
        match item.kind() {
            ItemKind::Child => {
                if let Some(child) = item.as_child() {
                    self.remove_subtree(child);
                }
            }
            ItemKind::Draw => {
                self.draws.remove(item.index());
            }
            ItemKind::Text => {
                self.texts.remove(item.index());
            }
            ItemKind::Run => {
                self.runs.remove(item.index());
            }
        }
    }

    /// A handle to `id` that tells a later holder whether the row went to
    /// another record since.
    pub fn record_ref(&self, id: RecordId) -> RecordRef {
        RecordRef {
            id,
            generation: self.generations[id.index()],
        }
    }

    /// The record `held` names, while its row still holds that record.
    pub fn resolve(&self, held: RecordRef) -> Option<RecordId> {
        (self.generations.get(held.id.index()) == Some(&held.generation)).then_some(held.id)
    }

    /// The items of `id` in draw order.
    pub fn items(&self, id: RecordId) -> &[Item] {
        self.items.span(self.tree[id.index()].items)
    }

    /// Replaces the items of `id` with `items`. The draws and children no
    /// longer among them stay in the store: the caller removes them.
    pub fn set_items(&mut self, id: RecordId, items: &[Item]) {
        let index = id.index();
        self.tree[index].items = self.items.write(self.tree[index].items, items);
    }

    /// Adds a draw primitive for an item.
    pub fn add_draw(&mut self, draw: DrawPrimitiveNode) -> Item {
        Item::new(ItemKind::Draw, self.draws.insert(draw))
    }

    /// Adds a text for an item.
    pub fn add_text(&mut self, text: TextPrimitiveNode) -> Item {
        Item::new(ItemKind::Text, self.texts.insert(text))
    }

    /// Adds a draw run for an item.
    pub fn add_run(&mut self, run: DrawRunNode) -> Item {
        Item::new(ItemKind::Run, self.runs.insert(run))
    }

    /// The draw primitive of a draw item.
    pub fn draw(&self, item: Item) -> &DrawPrimitiveNode {
        &self.draws[item.index()]
    }

    /// The text of a text item.
    pub fn text(&self, item: Item) -> &TextPrimitiveNode {
        &self.texts[item.index()]
    }

    /// The text of a text item, to change in place.
    pub fn text_mut(&mut self, item: Item) -> &mut TextPrimitiveNode {
        &mut self.texts[item.index()]
    }

    /// The draw run of a run item.
    pub fn run(&self, item: Item) -> &DrawRunNode {
        &self.runs[item.index()]
    }

    /// What `id` draws for.
    pub fn kind(&self, id: RecordId) -> RecordKind {
        self.tree[id.index()].kind
    }

    /// The layout node `id` draws for.
    pub fn node(&self, id: RecordId) -> Option<NodeId> {
        self.warm[id.index()].node
    }

    /// The record that holds `id`.
    pub fn parent(&self, id: RecordId) -> Option<RecordId> {
        self.tree[id.index()].parent
    }

    /// Sets the record that holds `id`.
    pub fn set_parent(&mut self, id: RecordId, parent: Option<RecordId>) {
        self.tree[id.index()].parent = parent;
    }

    /// The flags of `id`.
    pub fn flags(&self, id: RecordId) -> RecordFlags {
        self.tree[id.index()].flags
    }

    /// Sets the flags of `id`.
    pub fn set_flags(&mut self, id: RecordId, flags: RecordFlags) {
        self.tree[id.index()].flags = flags;
    }

    /// The size of the node rect of `id`.
    pub fn node_rect(&self, id: RecordId) -> Rect {
        self.geo[id.index()].node_rect()
    }

    /// Sets the node rect size of `id`.
    pub fn set_size(&mut self, id: RecordId, width: f32, height: f32) {
        self.geo[id.index()].size = [width, height];
    }

    /// The offset of the content box of `id` from its origin.
    pub fn content_offset(&self, id: RecordId) -> Point {
        let [x, y] = self.warm[id.index()].content_offset;
        Point::new(x, y)
    }

    /// Sets the offset of the content box of `id`.
    pub fn set_content_offset(&mut self, id: RecordId, offset: Point) {
        self.warm[id.index()].content_offset = [offset.x, offset.y];
    }

    /// Where layout placed `id` in its parent's content, before the
    /// parent's content offset.
    pub fn origin_in_parent(&self, id: RecordId) -> Point {
        let [x, y] = self.geo[id.index()].placement;
        Point::new(x, y)
    }

    /// The top left of `id` in its parent's layer: its placement and the
    /// parent's content offset.
    pub fn origin(&self, id: RecordId) -> Point {
        let index = id.index();
        match self.tree[index].transform {
            Some(transform) => self.transforms[transform.index()]
                .as_translation()
                .unwrap_or_else(|| self.placed_origin(index)),
            None => self.placed_origin(index),
        }
    }

    // Where the placement of the record at `index` puts it in its parent's
    // layer, past the parent's content offset.
    fn placed_origin(&self, index: usize) -> Point {
        let [x, y] = self.geo[index].placement;
        let offset = self.parent_content_offset(index);
        match self.tree[index].kind {
            RecordKind::Leaf => Point::new(x + offset.x, y + offset.y),
            RecordKind::Layer | RecordKind::Wrapper => {
                Point::new(x + 0.0 + offset.x, y + 0.0 + offset.y)
            }
            // A draw group lies in its node's own space, where the content
            // offset does not move it.
            RecordKind::DrawGroup => Point::new(x, y),
        }
    }

    fn parent_content_offset(&self, index: usize) -> Point {
        self.tree[index]
            .parent
            .map(|parent| self.content_offset(parent))
            .unwrap_or_default()
    }

    /// The transform from `id` to its parent's content.
    pub fn transform_to_parent(&self, id: RecordId) -> ProjectiveTransform {
        match self.tree[id.index()].transform {
            Some(transform) => self.transforms[transform.index()],
            None => {
                let origin = self.placed_origin(id.index());
                ProjectiveTransform::translation(origin.x, origin.y)
            }
        }
    }

    /// Places `id` at `origin_in_parent` in its parent's content by
    /// `transform`: a transform that only moves the record there, past the
    /// parent's content offset, takes no entry of its own. The parent's
    /// content offset is set before.
    pub fn set_placement(
        &mut self,
        id: RecordId,
        origin_in_parent: Point,
        transform: ProjectiveTransform,
    ) {
        let index = id.index();
        self.geo[index].placement = [origin_in_parent.x, origin_in_parent.y];
        let origin = self.placed_origin(index);
        let placed = ProjectiveTransform::translation(origin.x, origin.y);
        if same_bits(transform, placed) {
            if let Some(old) = self.tree[index].transform.take() {
                self.transforms.remove(old.index());
            }
            return;
        }
        match self.tree[index].transform {
            Some(old) => self.transforms[old.index()] = transform,
            None => {
                let entry = TransformId::from_index(self.transforms.insert(transform));
                self.tree[index].transform = Some(entry);
            }
        }
    }

    /// Sets what `id` draws for and the layout node it draws for.
    pub fn set_kind(&mut self, id: RecordId, kind: RecordKind, node: Option<NodeId>) {
        let index = id.index();
        if self.tree[index].kind == kind && self.warm[index].node == node {
            return;
        }
        self.log(RecordEvent::Released, id);
        self.tree[index].kind = kind;
        self.warm[index].node = node;
        self.log(RecordEvent::Claimed, id);
    }

    /// Removes every item of `id`: its draws and the records beneath it.
    pub fn clear_items(&mut self, id: RecordId) {
        let index = id.index();
        let span = self.tree[index].items;
        for position in 0..span.len() {
            let item = self.items.span(span)[position];
            self.remove_item(item);
        }
        self.items.free_span(span);
        self.tree[index].items = ItemSpan::default();
    }

    /// The properties of `id`, when it has any.
    pub fn props(&self, id: RecordId) -> Option<&LayerProps> {
        self.warm[id.index()]
            .props
            .map(|props| &self.props[props.index()])
    }

    /// Removes the properties of `id`.
    pub fn clear_props(&mut self, id: RecordId) {
        let Some(entry) = self.warm[id.index()].props.take() else {
            return;
        };
        if let Some(slot) = self
            .props
            .remove(entry.index())
            .and_then(|props| props.graphics_layer)
        {
            self.graphics_layers.remove(slot.index());
        }
    }

    /// The graphics layer set on `id`, `None` when it reads as the default.
    pub fn graphics_layer(&self, id: RecordId) -> Option<&GraphicsLayer> {
        self.props(id)
            .and_then(|props| props.graphics_layer)
            .map(|slot| &self.graphics_layers[slot.index()])
    }

    /// Sets the graphics layer of `id`; `None` reads as the default. A record
    /// that keeps one keeps its slot.
    pub fn set_graphics_layer(&mut self, id: RecordId, layer: Option<GraphicsLayer>) {
        let slot = self.props(id).and_then(|props| props.graphics_layer);
        match (slot, layer) {
            (Some(slot), Some(layer)) => self.graphics_layers[slot.index()] = layer,
            (Some(slot), None) => {
                self.graphics_layers.remove(slot.index());
                self.edit_props(id, |props| props.graphics_layer = None);
            }
            (None, Some(layer)) => {
                let slot = GraphicsLayerId::from_index(self.graphics_layers.insert(layer));
                self.edit_props(id, |props| props.graphics_layer = Some(slot));
            }
            (None, None) => {}
        }
    }

    // Takes the graphics layer of `id` out, leaving its other properties: it
    // reads as the default until `set_graphics_layer` puts it back.
    pub(crate) fn take_graphics_layer(&mut self, id: RecordId) -> Option<GraphicsLayer> {
        let entry = self.warm[id.index()].props?;
        let slot = self.props[entry.index()].graphics_layer.take()?;
        self.graphics_layers.remove(slot.index())
    }

    /// Changes the properties of `id` with `edit`, starting from defaults
    /// when it has none; properties left at their defaults go.
    pub fn edit_props(&mut self, id: RecordId, edit: impl FnOnce(&mut LayerProps)) {
        let index = id.index();
        let entry = match self.warm[index].props {
            Some(entry) => entry,
            None => {
                let entry = PropsId::from_index(self.props.insert(LayerProps::default()));
                self.warm[index].props = Some(entry);
                entry
            }
        };
        let props = &mut self.props[entry.index()];
        edit(props);
        if props.is_default() {
            self.props.remove(entry.index());
            self.warm[index].props = None;
        }
    }

    /// Notes that the update in progress changed what `id` draws: its own
    /// draws, properties or children. Its ancestors take the update as their
    /// subtree's.
    pub fn note_content_change(&mut self, id: RecordId) {
        let update = self.update;
        let index = id.index();
        if self.tree[index].content_rev != update {
            self.log(RecordEvent::Changed, id);
        }
        self.tree[index].content_rev = update;
        if let Some(props) = self.warm[index].props {
            self.props[props.index()].content_run.note(update);
        }
        self.note_subtree_change(Some(id));
    }

    /// Notes that the update in progress changed the properties of `id`
    /// without changing what it draws: its hashes go, while a raster cache
    /// keyed on its content does not count the update as a content change.
    pub fn note_property_change(&mut self, id: RecordId) {
        let update = self.update;
        let index = id.index();
        if self.tree[index].content_rev != update {
            self.log(RecordEvent::Changed, id);
        }
        let tree = &mut self.tree[index];
        tree.content_rev = update;
        tree.subtree_rev = update;
        tree.reach_state = ReachState::Stale;
        let parent = tree.parent;
        if let Some(props) = self.warm[index].props {
            self.props[props.index()].cache_hashes.set(None);
        }
        self.note_subtree_change(parent);
    }

    /// Notes that the update in progress moved `id` in its parent: the
    /// parent's content changed, while the subtree of `id` holds.
    pub fn note_place_change(&mut self, id: RecordId) {
        let index = id.index();
        if self.tree[index].place_rev != self.update {
            self.log(RecordEvent::Moved, id);
        }
        self.tree[index].place_rev = self.update;
        self.note_subtree_change(self.tree[index].parent);
    }

    // Raises the subtree revision of `from` and its ancestors to the update
    // in progress, and leaves their reach and hashes to be worked out again,
    // stopping at the first ancestor that holds the update already.
    fn note_subtree_change(&mut self, from: Option<RecordId>) {
        let update = self.update;
        let mut current = from;
        while let Some(record) = current {
            let index = record.index();
            let tree = &mut self.tree[index];
            if tree.subtree_rev == update {
                break;
            }
            tree.subtree_rev = update;
            tree.reach_state = ReachState::Stale;
            current = tree.parent;
            if let Some(props) = self.warm[index].props {
                let props = &mut self.props[props.index()];
                props.cache_hashes.set(None);
                props.content_run.note(update);
            }
        }
    }

    /// The update that last changed what `id` itself draws.
    pub fn content_rev(&self, id: RecordId) -> SceneUpdate {
        self.tree[id.index()].content_rev
    }

    /// The latest update that changed `id` or a record beneath it.
    pub fn subtree_rev(&self, id: RecordId) -> SceneUpdate {
        self.tree[id.index()].subtree_rev
    }

    /// The update that last moved `id` in its parent.
    pub fn place_rev(&self, id: RecordId) -> SceneUpdate {
        self.tree[id.index()].place_rev
    }
}

// Whether two transforms hold the same numbers, bit for bit.
fn same_bits(a: ProjectiveTransform, b: ProjectiveTransform) -> bool {
    a.matrix()
        .iter()
        .flatten()
        .zip(b.matrix().iter().flatten())
        .all(|(a, b)| a.to_bits() == b.to_bits())
}
