// The records a renderer draws: a tree of layout nodes' layers, wrappers,
// draw groups and leaves, each a row of fixed-size records with its items in
// one span, its other properties and its draws in side arenas.

use std::sync::atomic::{AtomicU16, Ordering};

use cranpose_core::{NodeId, RecordId, RecordRef, SceneUpdate, StoreId, collections::map::HashSet};
use cranpose_ui::{GraphicsLayer, Point, Rect};

use super::{
    ChildOrder, Facts, GeoRecord, GraphicsLayerId, HashCache, Item, ItemKind, ItemLists, ItemRef,
    Journal, JournalCursor, JournalEntry, JournalRead, LayerProps, LayerRef, PropsId, Reach,
    ReachState, RecordEvent, RecordEvents, RecordFlags, RecordKind, Slab, Span, SpanPool,
    TransformId, TreeRecord, WarmRecord,
    own_marks::{self, OwnMark},
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
    pub(super) items: SpanPool<Item>,
    pub(super) marks: SpanPool<OwnMark>,
    pub(super) props: Slab<LayerProps>,
    pub(super) graphics_layers: Slab<GraphicsLayer>,
    pub(super) transforms: Slab<ProjectiveTransform>,
    pub(super) draws: Slab<DrawPrimitiveNode>,
    pub(super) texts: Slab<TextPrimitiveNode>,
    pub(super) runs: Slab<DrawRunNode>,
    pub(super) hashes: HashCache,
    pub(super) facts: Facts,
    pub(crate) lists: ItemLists,
    root: Option<RecordId>,
    update: SceneUpdate,
    journal: Journal,
    // The records whose own draws the update in progress may have changed,
    // which its end compares with what they drew before.
    pub(super) redrawn: Vec<RecordRef>,
    // The records whose marks wait for a reader that follows redraws.
    pub(super) marks_stale: Vec<RecordRef>,
    pub(super) order: ChildOrder,
    // The marks of the own draws of the record whose end of update is
    // being worked out.
    pub(super) mark_scratch: Vec<OwnMark>,
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
            items: SpanPool::default(),
            marks: SpanPool::default(),
            props: Slab::default(),
            graphics_layers: Slab::default(),
            transforms: Slab::default(),
            draws: Slab::default(),
            texts: Slab::default(),
            runs: Slab::default(),
            hashes: HashCache::default(),
            facts: Facts::default(),
            lists: ItemLists::default(),
            root: None,
            update: SceneUpdate::default(),
            journal: Journal::default(),
            redrawn: Vec::new(),
            marks_stale: Vec::new(),
            order: ChildOrder::default(),
            mark_scratch: Vec::new(),
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
        self.journal.trim(self.tree.len());
        self.update = self.update.next();
        self.update
    }

    /// Starts following the `events` of the store's records: the reader's
    /// first read reports the changes after now.
    pub fn open_journal(&mut self, events: RecordEvents) -> JournalCursor {
        let cursor = self.journal.open(self.id, events);
        self.mark_for_redraws();
        cursor
    }

    /// Stops following the changes of the store's records.
    pub fn close_journal(&mut self, cursor: &JournalCursor) {
        self.journal.close(cursor);
    }

    /// Makes the reader of `cursor` follow `events` from the next update
    /// on.
    pub fn follow_journal(&mut self, cursor: &mut JournalCursor, events: RecordEvents) {
        self.journal.follow(self.id, cursor, events);
        self.mark_for_redraws();
    }

    // Whether a reader follows where the update in progress redraws.
    pub(super) fn follows_redrawn_reach(&self) -> bool {
        self.journal.wants_redrawn_reach(self.update)
    }

    // Works out the marks that wait once a reader follows where records
    // redraw, so it compares the next redraws with the store as it is now.
    fn mark_for_redraws(&mut self) {
        if self.follows_redrawn_reach() {
            own_marks::refresh_stale(self);
        }
    }

    /// Hands `read` the store and the changes of its records since the
    /// reader's last read, then marks them read. A reader of another store
    /// starts following this one, and learns that its changes are lost.
    pub fn read_journal<R>(
        &mut self,
        cursor: &mut JournalCursor,
        read: impl FnOnce(&Self, JournalRead<'_>) -> R,
    ) -> R {
        let result = read(self, self.journal.changes(self.id, cursor));
        self.journal.mark_read(self.id, cursor);
        result
    }

    // Logs `event` for `id` as the record is now, when a reader follows it.
    pub(super) fn log(&mut self, event: RecordEvent, id: RecordId) {
        if !self.journal.wants(event, self.update) {
            return;
        }
        let index = id.index();
        self.journal.log(JournalEntry {
            update: self.update,
            event,
            kind: self.tree[index].kind,
            record: RecordRef {
                id,
                generation: self.generations[index],
            },
            parent: self.tree[index].parent.map(|parent| RecordRef {
                id: parent,
                generation: self.generations[parent.index()],
            }),
            node: self.warm[index].node,
        });
    }

    /// Ends the update in progress: logs the records whose children it put
    /// in another order and the own draws it redrew. Readers read the store
    /// after it.
    pub fn finish_update(&mut self) {
        super::child_order::settle(self);
        own_marks::settle(self);
    }

    // Leaves the own draws of the record at `index` to be compared with what
    // they were at the end of the update.
    fn redraw_pending(&mut self, index: usize) {
        let flags = &mut self.tree[index].flags;
        if !flags.contains(RecordFlags::REDRAW_PENDING) {
            *flags = flags.with(RecordFlags::REDRAW_PENDING);
            self.redrawn.push(RecordRef {
                id: RecordId::from_index(index).expect("a row is a record"),
                generation: self.generations[index],
            });
        }
    }

    // The reach of the record at `index`.
    pub(super) fn reach(&self, index: usize) -> Reach {
        match self.tree[index].reach_state {
            ReachState::Empty => Reach::Empty,
            ReachState::Finite => Reach::Finite(self.geo[index].reach),
            ReachState::Unknown => Reach::Unknown,
        }
    }

    pub(super) fn set_reach(&mut self, index: usize, reach: Reach) {
        let (state, rect) = match reach {
            Reach::Empty => (ReachState::Empty, Rect::default()),
            Reach::Finite(rect) => (ReachState::Finite, rect),
            Reach::Unknown => (ReachState::Unknown, Rect::default()),
        };
        self.tree[index].reach_state = state;
        self.geo[index].reach = rect;
    }

    // Leaves the containment and the reach of the record at `index` and of
    // the ancestors that follow from them to be worked out again.
    pub(super) fn stale(&mut self, index: usize) {
        super::facts::forget_contained(self, index);
        self.reach_stale(index);
    }

    // Leaves the reach of the record at `index` and of its ancestors to be
    // worked out again, stopping at the first that waits for it already.
    fn reach_stale(&mut self, index: usize) {
        let mut current = Some(index);
        while let Some(index) = current {
            let flags = &mut self.tree[index].flags;
            if flags.contains(RecordFlags::REACH_STALE) {
                break;
            }
            *flags = flags.with(RecordFlags::REACH_STALE);
            current = self.tree[index].parent.map(RecordId::index);
        }
    }

    /// Works out the reach of every record whose reach a change beneath it
    /// left to be worked out again, as [`LayerRef::reach`] reads it.
    pub fn refresh_reach(&mut self) {
        own_marks::refresh_stale(self);
        if let Some(root) = self.root {
            super::reach::refresh(self, root);
        }
    }

    // Leaves the containment of the record at `index` and of its parent,
    // which reads what the record draws, to be worked out again.
    fn stale_with_parent(&mut self, index: usize) {
        self.stale(index);
        if let Some(parent) = self.tree[index].parent {
            self.stale(parent.index());
        }
    }

    /// A store that holds `root` and the tree beneath it as records.
    pub fn from_layer(root: &LayerNode) -> Self {
        let mut store = Self::new();
        store.describe(root);
        store
    }

    /// Writes `root` and the tree beneath it as records in place of what the
    /// store held, in a new update. Records of the same kind and node at the
    /// same place stay, so the update changes only what differs.
    pub fn describe(&mut self, root: &LayerNode) {
        self.begin_update();
        super::spec::write_root(self, root);
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
        self.hashes.forget_all();
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
        self.marks.clear();
        self.props.clear();
        self.graphics_layers.clear();
        self.transforms.clear();
        self.draws.clear();
        self.texts.clear();
        self.runs.clear();
        self.hashes.clear();
        self.facts.clear();
        self.root = None;
        self.journal.clear(self.update);
        self.redrawn.clear();
        self.marks_stale.clear();
    }

    /// Makes a record from `seed`, changed in the update in progress, with no
    /// items, at the origin of its parent and of zero size. Its ancestors
    /// take the update as their subtree's; the caller adds it to its
    /// parent's items.
    pub fn insert(&mut self, seed: RecordSeed) -> RecordId {
        let update = self.update;
        let tree = TreeRecord {
            parent: seed.parent,
            items: Span::default(),
            kind: seed.kind,
            reach_state: ReachState::Empty,
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
            own_marks: Span::default(),
            own_reach: Reach::Empty,
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
        self.hashes.open_row(id.index());
        self.facts.open_row(id.index());
        match seed.parent {
            Some(parent) => self.note_subtree_change(Some(parent)),
            None => self.root = Some(id),
        }
        self.stale_with_parent(id.index());
        let flags = &mut self.tree[id.index()].flags;
        *flags = flags.with(RecordFlags::INSERTED);
        self.redraw_pending(id.index());
        self.log(RecordEvent::Inserted, id);
        id
    }

    /// Removes `id`, its draws, its properties and every record beneath it.
    pub fn remove_subtree(&mut self, id: RecordId) {
        self.log(RecordEvent::Removed, id);
        let index = id.index();
        if let Some(parent) = self.tree[index].parent {
            self.stale(parent.index());
        }
        let span = self.tree[index].items;
        for position in 0..span.len() {
            let item = self.items.span(span)[position];
            self.remove_item(item);
        }
        self.items.free_span(span);
        self.marks.free_span(self.warm[index].own_marks);
        self.drop_props(index);
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
    /// longer among them stay in the store: the caller removes them. A
    /// caller that changes the record's own draws notes it with
    /// [`Self::note_content_change`].
    pub fn set_items(&mut self, id: RecordId, items: &[Item]) {
        let index = id.index();
        let old = self.items.span(self.tree[index].items);
        if !super::facts::same_children(old, items) {
            super::facts::forget_children(self, index);
        }
        if self.journal.wants(RecordEvent::Changed, self.update) {
            let record = self.record_ref(id);
            self.order.note(record, old);
        }
        self.tree[index].items = self.items.write(self.tree[index].items, items);
        self.stale(index);
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
        let index = id.index();
        let current = self.tree[index].flags;
        let flags = flags
            .without(RecordFlags::STORE_KEPT)
            .with(current.only(RecordFlags::STORE_KEPT));
        if flags != current {
            self.tree[index].flags = flags;
            if flags.only(RecordFlags::ORIGIN_SINKS) != current.only(RecordFlags::ORIGIN_SINKS) {
                super::facts::forget_children(self, index);
            }
            self.stale_with_parent(index);
        }
    }

    /// The size of the node rect of `id`.
    pub fn node_rect(&self, id: RecordId) -> Rect {
        self.geo[id.index()].node_rect()
    }

    /// Sets the node rect size of `id`.
    pub fn set_size(&mut self, id: RecordId, width: f32, height: f32) {
        let index = id.index();
        let size = [width, height];
        if self.geo[index].size.map(f32::to_bits) != size.map(f32::to_bits) {
            self.geo[index].size = size;
            self.stale_with_parent(index);
        }
    }

    /// The offset of the content box of `id` from its origin.
    pub fn content_offset(&self, id: RecordId) -> Point {
        let [x, y] = self.warm[id.index()].content_offset;
        Point::new(x, y)
    }

    /// Sets the offset of the content box of `id`.
    pub fn set_content_offset(&mut self, id: RecordId, offset: Point) {
        let index = id.index();
        let offset = [offset.x, offset.y];
        if self.warm[index].content_offset.map(f32::to_bits) != offset.map(f32::to_bits) {
            self.warm[index].content_offset = offset;
            self.stale(index);
            self.moved(index);
        }
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
        let placement = [origin_in_parent.x, origin_in_parent.y];
        let moved = self.geo[index].placement.map(f32::to_bits) != placement.map(f32::to_bits);
        self.geo[index].placement = placement;
        let origin = self.placed_origin(index);
        let places_only = super::records::same_bits(
            transform,
            ProjectiveTransform::translation(origin.x, origin.y),
        );
        let entry = self.tree[index].transform;
        // An entry holds a transform that does more than place the record.
        let unchanged = !moved
            && match entry {
                Some(entry) => {
                    !places_only
                        && super::records::same_bits(self.transforms[entry.index()], transform)
                }
                None => places_only,
            };
        if unchanged {
            return;
        }
        self.moved(index);
        if let Some(parent) = self.tree[index].parent {
            self.stale(parent.index());
        }
        match (places_only, entry) {
            (true, Some(old)) => {
                self.tree[index].transform = None;
                self.transforms.remove(old.index());
            }
            (true, None) => {}
            (false, Some(old)) => self.transforms[old.index()] = transform,
            (false, None) => {
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
        self.stale_with_parent(index);
        self.log(RecordEvent::Claimed, id);
    }

    /// Removes every item of `id`: its draws and the records beneath it.
    pub fn clear_items(&mut self, id: RecordId) {
        let index = id.index();
        let span = self.tree[index].items;
        if !super::facts::same_children(self.items.span(span), &[]) {
            super::facts::forget_children(self, index);
        }
        for position in 0..span.len() {
            let item = self.items.span(span)[position];
            self.remove_item(item);
        }
        self.items.free_span(span);
        self.tree[index].items = Span::default();
        self.stale(index);
    }

    /// The properties of `id`, when it has any.
    pub fn props(&self, id: RecordId) -> Option<&LayerProps> {
        self.warm[id.index()]
            .props
            .map(|props| &self.props[props.index()])
    }

    /// Removes the properties of `id`.
    pub fn clear_props(&mut self, id: RecordId) {
        let index = id.index();
        if self.warm[index].props.is_some() {
            self.drop_props(index);
            super::facts::forget_children(self, index);
            self.stale_with_parent(index);
        }
    }

    fn drop_props(&mut self, index: usize) {
        let Some(entry) = self.warm[index].props.take() else {
            return;
        };
        if let Some(slot) = self
            .props
            .remove(entry.index())
            .and_then(|props| props.graphics_layer)
        {
            self.graphics_layers.remove(slot.index());
        }
        self.tree[index].flags = self.tree[index]
            .flags
            .without(RecordFlags::HAS_GRAPHICS_LAYER | RecordFlags::HAS_LAYER_BOUNDS);
    }

    // Sets the flags that tell what the properties of the record at `index`
    // hold, from them.
    fn sync_props_flags(&mut self, index: usize) {
        let (layer, bounds) = self.warm[index]
            .props
            .map(|entry| &self.props[entry.index()])
            .map_or((false, false), |props| {
                (props.graphics_layer.is_some(), props.layer_bounds.is_some())
            });
        let flags = self.tree[index]
            .flags
            .without(RecordFlags::HAS_GRAPHICS_LAYER | RecordFlags::HAS_LAYER_BOUNDS);
        let flags = if layer {
            flags.with(RecordFlags::HAS_GRAPHICS_LAYER)
        } else {
            flags
        };
        self.tree[index].flags = if bounds {
            flags.with(RecordFlags::HAS_LAYER_BOUNDS)
        } else {
            flags
        };
    }

    /// The graphics layer set on `id`, `None` when it reads as the default.
    pub fn graphics_layer(&self, id: RecordId) -> Option<&GraphicsLayer> {
        if !self.tree[id.index()]
            .flags
            .contains(RecordFlags::HAS_GRAPHICS_LAYER)
        {
            return None;
        }
        self.props(id)
            .and_then(|props| props.graphics_layer)
            .map(|slot| &self.graphics_layers[slot.index()])
    }

    /// Sets the graphics layer of `id`; `None` reads as the default. A record
    /// that keeps one keeps its slot.
    pub fn set_graphics_layer(&mut self, id: RecordId, layer: Option<GraphicsLayer>) {
        let slot = self.props(id).and_then(|props| props.graphics_layer);
        let space =
            |layer: Option<&GraphicsLayer>| layer.is_some_and(|layer| layer.space.is_some());
        if space(slot.map(|slot| &self.graphics_layers[slot.index()])) != space(layer.as_ref()) {
            super::facts::forget_children(self, id.index());
        }
        match (slot, layer) {
            (Some(slot), Some(layer)) => {
                let current = &self.graphics_layers[slot.index()];
                if super::facts::layer_inputs(current) != super::facts::layer_inputs(&layer) {
                    self.stale_with_parent(id.index());
                }
                self.graphics_layers[slot.index()] = layer;
            }
            (Some(slot), None) => {
                self.graphics_layers.remove(slot.index());
                self.edit_props(id, |props| props.graphics_layer = None);
                self.stale_with_parent(id.index());
            }
            (None, Some(layer)) => {
                let slot = GraphicsLayerId::from_index(self.graphics_layers.insert(layer));
                self.edit_props(id, |props| props.graphics_layer = Some(slot));
                self.stale_with_parent(id.index());
            }
            (None, None) => {}
        }
    }

    /// Changes the properties of `id` with `edit`, starting from defaults
    /// when it has none; properties left at their defaults go.
    pub fn edit_props(&mut self, id: RecordId, edit: impl FnOnce(&mut LayerProps)) {
        let index = id.index();
        let entry = self.warm[index].props;
        let mut fresh = None;
        let props = match entry {
            Some(entry) => &mut self.props[entry.index()],
            None => fresh.insert(LayerProps::default()),
        };
        let before = (super::facts::props_inputs(props), props.hit_test.is_some());
        edit(props);
        let after = (super::facts::props_inputs(props), props.hit_test.is_some());
        match (entry, props.is_default(), fresh) {
            (Some(entry), true, _) => {
                self.props.remove(entry.index());
                self.warm[index].props = None;
            }
            (None, false, Some(props)) => {
                self.warm[index].props = Some(PropsId::from_index(self.props.insert(props)));
            }
            _ => {}
        }
        self.sync_props_flags(index);
        if before.1 != after.1 {
            super::facts::forget_children(self, index);
        }
        if before.0 != after.0 {
            self.stale_with_parent(index);
        }
    }

    /// Notes that the update in progress changed what `id` draws: its own
    /// draws, properties or children. Its ancestors take the update as their
    /// subtree's.
    pub fn note_content_change(&mut self, id: RecordId) {
        let update = self.update;
        let index = id.index();
        self.redraw_pending(index);
        self.stale(index);
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
        let parent = tree.parent;
        self.hashes.forget(index);
        self.note_subtree_change(parent);
    }

    /// Notes that the update in progress may have moved `id` in its parent,
    /// or its content within it: when it did, the parent's content changed,
    /// while the subtree of `id` holds.
    pub fn note_place_change(&mut self, id: RecordId) {
        let index = id.index();
        if self.tree[index].place_rev == self.update {
            self.note_subtree_change(self.tree[index].parent);
        }
    }

    // Notes that the update in progress moved the record at `index`, or its
    // content within it.
    fn moved(&mut self, index: usize) {
        if self.tree[index].place_rev != self.update {
            self.tree[index].place_rev = self.update;
            self.log(
                RecordEvent::Moved,
                RecordId::from_index(index).expect("a row is a record"),
            );
        }
    }

    // Raises the subtree revision of `from` and its ancestors to the update
    // in progress, and leaves their hashes to be worked out again, stopping
    // at the first ancestor that holds the update already.
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
            current = tree.parent;
            self.hashes.forget(index);
            if let Some(props) = self.warm[index].props {
                self.props[props.index()].content_run.note(update);
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
