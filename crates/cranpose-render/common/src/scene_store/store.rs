// The records a renderer draws: a tree of layout nodes' layers, wrappers,
// draw groups and leaves, each a row of fixed-size records with its items in
// one span, its other properties and its draws in side arenas.

use std::{
    rc::Rc,
    sync::atomic::{AtomicU16, Ordering},
};

use cranpose_core::{NodeId, RecordId, RecordRef, SceneUpdate, StoreId, collections::map::HashSet};
use cranpose_ui::{GraphicsLayer, Rect};
use cranpose_ui_graphics::CommandRecording;

use super::{
    ChildOrder, Facts, GeoRecord, HashCache, Item, ItemKind, ItemLists, ItemRef, Journal,
    JournalCursor, JournalEntry, JournalRead, LayerProps, LayerRef, Reach, RecordEvent,
    RecordEvents, RecordFlags, RecordKind, RecordNode, ShapeChunks, ShapeRun, Slab, Span, SpanPool,
    TreeRecord, WarmRecord, recordings::Recordings, touched::Touched,
};
#[cfg(feature = "window-damage")]
use super::{ReachState, own_marks};
use crate::graph::{
    DrawCommandId, DrawPrimitiveNode, DrawRunNode, LayerNode, ProjectiveTransform,
    TextPrimitiveNode,
};

/// The scene a renderer draws and the hit targets come from.
pub struct SceneStore {
    // The store's identity among the renderers' stores.
    id: StoreId,
    pub(super) tree: Vec<TreeRecord>,
    pub(super) geo: Vec<GeoRecord>,
    pub(super) warm: Vec<WarmRecord>,
    pub(super) generations: Vec<u32>,
    // The generation a new row starts at: past every one a cleared store
    // gave out.
    generation_floor: u32,
    pub(super) free_rows: Vec<u32>,
    pub(super) items: SpanPool<Item>,
    #[cfg(feature = "window-damage")]
    pub(super) marks: own_marks::Marks,
    pub(super) props: Slab<LayerProps>,
    pub(super) graphics_layers: Slab<GraphicsLayer>,
    pub(super) transforms: Slab<ProjectiveTransform>,
    pub(super) draws: Slab<DrawPrimitiveNode>,
    pub(super) texts: Slab<TextPrimitiveNode>,
    pub(super) runs: Slab<DrawRunNode>,
    pub(super) shape_runs: Slab<ShapeRun>,
    pub(super) shape_chunks: ShapeChunks,
    // The runs of the shape items the update in progress removed, in the
    // order it removed them: an item added with the shapes of the last one
    // takes its run back. They go when the update ends or records other
    // shapes.
    pub(super) removed_shapes: Vec<ShapeRun>,
    pub(super) hashes: HashCache,
    pub(super) facts: Facts,
    pub(crate) lists: ItemLists,
    root: Option<RecordId>,
    update: SceneUpdate,
    journal: Journal,
    touched: Touched,
    pub(super) order: ChildOrder,
    pub(crate) recordings: Recordings,
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
            generation_floor: 0,
            free_rows: Vec::new(),
            items: SpanPool::default(),
            #[cfg(feature = "window-damage")]
            marks: own_marks::Marks::default(),
            props: Slab::default(),
            graphics_layers: Slab::default(),
            transforms: Slab::default(),
            draws: Slab::default(),
            texts: Slab::default(),
            runs: Slab::default(),
            shape_runs: Slab::default(),
            shape_chunks: ShapeChunks::default(),
            removed_shapes: Vec::new(),
            hashes: HashCache::default(),
            facts: Facts::default(),
            lists: ItemLists::default(),
            root: None,
            update: SceneUpdate::default(),
            journal: Journal::default(),
            touched: Touched::default(),
            order: ChildOrder::default(),
            recordings: Recordings::default(),
        }
    }

    /// The store's identity among the renderers' stores.
    #[inline]
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
        self.recordings.begin_update();
        self.touched.begin(self.update);
        self.update = self.update.next();
        self.update
    }

    /// The records the latest update made, changed or moved, with the update
    /// before it. A reader that holds the scene of that update brings itself
    /// up to the store from them, with no walk of the records that held
    /// still. A record may come twice. `None` when the update changed more
    /// records than the store lists, or cleared the store: the reader reads
    /// the store whole.
    pub fn touched(&self) -> Option<(SceneUpdate, &[RecordRef])> {
        self.touched.read()
    }

    /// Whether `id` changed itself or moved in the latest `updates` updates.
    #[inline]
    pub fn changed_within(&self, id: RecordId, updates: u32) -> bool {
        let row = &self.tree[id.index()];
        self.update.since(row.content_rev) < updates || self.update.since(row.place_rev) < updates
    }

    // Lists the record at `index` among those the update changed, unless a
    // stamp of the update shows it listed.
    #[inline]
    fn touch(&mut self, index: usize) {
        let tree = &self.tree[index];
        if tree.content_rev != self.update && tree.place_rev != self.update {
            self.touched.note(RecordRef {
                id: RecordId::from_index(index).expect("a row is a record"),
                generation: self.generations[index],
            });
        }
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
    #[cfg(feature = "window-damage")]
    #[inline]
    pub(super) fn follows_redrawn_reach(&self) -> bool {
        self.journal.wants_redrawn_reach(self.update)
    }

    // Works out the marks that wait once a reader follows where records
    // redraw, so it compares the next redraws with the store as it is now.
    #[cfg(feature = "window-damage")]
    fn mark_for_redraws(&mut self) {
        if self.follows_redrawn_reach() {
            own_marks::refresh_lost(self);
        }
    }

    #[cfg(not(feature = "window-damage"))]
    #[inline]
    fn mark_for_redraws(&mut self) {}

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

    // Whether the reader of `cursor` read every change of the store.
    #[inline]
    pub(crate) fn journal_caught_up(&self, cursor: &JournalCursor) -> bool {
        self.journal.caught_up(self.id, cursor)
    }

    // Logs `event` for `id` as the record is now, when a reader follows it.
    #[inline]
    pub(super) fn log(&mut self, event: RecordEvent, id: RecordId) {
        if self.journal.wants(event, self.update) {
            self.log_entry(event, id);
        }
    }

    fn log_entry(&mut self, event: RecordEvent, id: RecordId) {
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
            node: self.warm[index].node.map(RecordNode::id),
        });
    }

    /// Ends the update in progress: logs the records whose children it put
    /// in another order and the own draws it redrew. Readers read the store
    /// after it.
    pub fn finish_update(&mut self) {
        self.release_removed_shapes();
        super::child_order::settle(self);
        #[cfg(feature = "window-damage")]
        own_marks::settle(self);
        super::growth_slack::trim(self);
    }

    // Leaves the own draws of the record at `index` to be compared with what
    // they were at the end of the update. While no reader follows where
    // records redraw, the record's marks wait at once.
    #[expect(
        clippy::inline_always,
        reason = "the collect walk and the update call it for every record; out of line the gauntlet frame took 2% more instructions"
    )]
    #[inline(always)]
    #[cfg(feature = "window-damage")]
    pub(super) fn redraw_pending(&mut self, index: usize) {
        if !self.follows_redrawn_reach() {
            own_marks::lose(self, index);
            return;
        }
        let flags = &mut self.tree[index].flags;
        if !flags.contains(RecordFlags::REDRAW_PENDING) {
            *flags = flags.with(RecordFlags::REDRAW_PENDING);
            self.marks.redrawn.push(RecordRef {
                id: RecordId::from_index(index).expect("a row is a record"),
                generation: self.generations[index],
            });
        }
    }

    // Without window damage a record keeps no marks, and logs each redraw
    // without its reach.
    #[cfg(not(feature = "window-damage"))]
    #[inline]
    pub(super) fn redraw_pending(&mut self, index: usize) {
        self.log(
            RecordEvent::Redrawn(Reach::Unknown),
            RecordId::from_index(index).expect("a row is a record"),
        );
    }

    // The reach of the record at `index`.
    #[cfg(feature = "window-damage")]
    pub(super) fn reach(&self, index: usize) -> Reach {
        match self.tree[index].reach_state {
            ReachState::Empty => Reach::Empty,
            ReachState::Finite => Reach::Finite(self.geo[index].reach),
            ReachState::Unknown => Reach::Unknown,
        }
    }

    #[cfg(feature = "window-damage")]
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
    // the ancestors that follow from them to be worked out again. Most often
    // an earlier change in the update left the record so already.
    #[cfg(feature = "window-damage")]
    #[inline]
    pub(super) fn stale(&mut self, index: usize) {
        if !self.tree[index].flags.contains(RecordFlags::REACH_STALE)
            || super::facts::keeps_contained(self, index)
        {
            self.stale_climb(index);
        }
    }

    // Without window damage no reader asks for reach or containment.
    #[cfg(not(feature = "window-damage"))]
    #[inline]
    pub(super) fn stale(&mut self, _index: usize) {}

    #[cfg(feature = "window-damage")]
    #[inline(never)]
    fn stale_climb(&mut self, index: usize) {
        super::facts::forget_contained(self, index);
        self.reach_stale(index);
    }

    // Leaves the reach of the record at `index` and of its ancestors to be
    // worked out again, stopping at the first that waits for it already.
    #[cfg(feature = "window-damage")]
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
    #[cfg(feature = "window-damage")]
    pub fn refresh_reach(&mut self) {
        own_marks::refresh_lost(self);
        if let Some(root) = self.root {
            super::reach::refresh(self, root);
        }
    }

    // Leaves the containment of the record at `index` and of its parent,
    // which reads what the record draws, to be worked out again.
    #[inline]
    pub(super) fn stale_with_parent(&mut self, index: usize) {
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
                    ItemRef::Draw(_) | ItemRef::Text(_) | ItemRef::Shape(_) => {}
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
    #[inline]
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
            + self.shape_item_count()
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
        // A record made after the clear takes a generation past every one
        // given out before, so no handle taken before names it.
        if let Some(&last) = self.generations.iter().max() {
            self.generation_floor = last.wrapping_add(1);
        }
        self.generations.clear();
        self.free_rows.clear();
        self.items.clear();
        #[cfg(feature = "window-damage")]
        self.marks.clear();
        self.props.clear();
        self.graphics_layers.clear();
        self.transforms.clear();
        self.draws.clear();
        self.texts.clear();
        self.runs.clear();
        self.clear_shapes();
        self.hashes.clear();
        self.facts.clear();
        self.root = None;
        self.journal.clear(self.update);
        self.touched.lose();
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
            #[cfg(feature = "window-damage")]
            reach_state: ReachState::Empty,
            flags: RecordFlags::default(),
            content_rev: update,
            place_rev: update,
            subtree_rev: update,
            transform: None,
        };
        let warm = WarmRecord {
            node: seed.node.map(RecordNode::of),
            props: None,
            graphics_layer: None,
            content_offset: [0.0; 2],
            #[cfg(feature = "window-damage")]
            own_marks: Span::default(),
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
                self.generations.push(self.generation_floor);
                RecordId::from_index(self.tree.len() - 1).expect("fewer than 2^32 records")
            }
        };
        self.facts.open_row(id.index());
        match seed.parent {
            Some(parent) => self.note_subtree_change(Some(parent)),
            None => self.root = Some(id),
        }
        self.stale_with_parent(id.index());
        #[cfg(feature = "window-damage")]
        own_marks::pend_inserted(self, id.index());
        self.log(RecordEvent::Inserted, id);
        self.touched.note(self.record_ref(id));
        id
    }

    // Out of line: inlined into `remove_item`, a removal most updates never
    // make takes room in the code that drops each replaced draw.
    /// Removes `id`, its draws, its properties and every record beneath it.
    #[inline(never)]
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
        #[cfg(feature = "window-damage")]
        self.marks.pool.free_span(self.warm[index].own_marks);
        self.hashes.remove(index);
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
                if let Some(run) = self.runs.take(item.index()) {
                    self.keep_released(run.command, &run.recording);
                }
            }
            ItemKind::Shape => self.remove_shapes(item.index()),
        }
    }

    /// A handle to `id` that tells a later holder whether the row went to
    /// another record since.
    #[inline]
    pub fn record_ref(&self, id: RecordId) -> RecordRef {
        RecordRef {
            id,
            generation: self.generations[id.index()],
        }
    }

    /// The record `held` names, while its row still holds that record.
    #[inline]
    pub fn resolve(&self, held: RecordRef) -> Option<RecordId> {
        (self.generations.get(held.id.index()) == Some(&held.generation)).then_some(held.id)
    }

    /// The items of `id` in draw order.
    #[inline]
    pub fn items(&self, id: RecordId) -> &[Item] {
        self.items.span(self.tree[id.index()].items)
    }

    /// Replaces the items of `id` with `items`. The draws and children no
    /// longer among them stay in the store: the caller removes them. A
    /// caller that changes the record's own draws notes it with
    /// [`Self::note_content_change`].
    pub fn set_items(&mut self, id: RecordId, items: &[Item]) {
        if self.items.span(self.tree[id.index()].items) != items {
            // The note stamps the record, so the write lists it once.
            self.note_content_change(id);
            self.write_items(id, items);
        }
    }

    // Writes the items of `id` for the scene builder, which notes the
    // change itself.
    pub(crate) fn write_items(&mut self, id: RecordId, items: &[Item]) {
        self.replace_items(id, items);
        self.stale(id.index());
    }

    // Replaces the items of `id` with `items` and logs the change, leaving
    // its containment and reach to the caller.
    pub(super) fn replace_items(&mut self, id: RecordId, items: &[Item]) {
        let index = id.index();
        let old = self.items.span(self.tree[index].items);
        // Draws written in place of removed ones of the same kinds take
        // their handles: the items read as before.
        if old == items {
            return;
        }
        if !super::facts::same_children(old, items) {
            super::facts::forget_children(self, index);
        }
        super::facts::forget_own(self, index);
        let noted = self.journal.follows(RecordEvent::Changed);
        let changed = !noted && self.journal.wants(RecordEvent::Changed, self.update);
        if noted {
            let record = self.record_ref(id);
            self.order.note(record, old);
        }
        self.tree[index].items = self.items.write(self.tree[index].items, items);
        self.touch(index);
        // A reader that learns only whether the update changed anything
        // learns it from the first write of other items.
        if changed {
            self.log(RecordEvent::Changed, id);
        }
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
    #[inline]
    pub fn draw(&self, item: Item) -> &DrawPrimitiveNode {
        &self.draws[item.index()]
    }

    /// The text of a text item.
    #[inline]
    pub fn text(&self, item: Item) -> &TextPrimitiveNode {
        &self.texts[item.index()]
    }

    /// The text of a text item, to change in place.
    pub fn text_mut(&mut self, item: Item) -> &mut TextPrimitiveNode {
        &mut self.texts[item.index()]
    }

    /// The draw run of a run item.
    #[inline]
    pub fn run(&self, item: Item) -> &DrawRunNode {
        &self.runs[item.index()]
    }

    // The draw run of a run item, to draw again in place.
    #[inline]
    pub(crate) fn run_mut(&mut self, item: Item) -> &mut DrawRunNode {
        &mut self.runs[item.index()]
    }

    // Puts `recording` in place of the recording of the run `item`.
    pub(crate) fn swap_run_recording(&mut self, item: Item, recording: Rc<CommandRecording>) {
        let run = &mut self.runs[item.index()];
        let command = run.command;
        let old = std::mem::replace(&mut run.recording, recording);
        self.keep_released(command, &old);
    }

    // Keeps `recording`, which `command` recorded and which the store lets
    // go of, for the command's next recording once no reader holds it.
    fn keep_released(&mut self, command: Option<DrawCommandId>, recording: &Rc<CommandRecording>) {
        if let Some(id) = command {
            self.recordings.keep(id, recording);
        }
    }

    /// What `id` draws for.
    #[inline]
    pub fn kind(&self, id: RecordId) -> RecordKind {
        self.tree[id.index()].kind
    }

    /// The layout node `id` draws for.
    #[inline]
    pub fn node(&self, id: RecordId) -> Option<NodeId> {
        self.warm[id.index()].node.map(RecordNode::id)
    }

    /// The record that holds `id`.
    #[inline]
    pub fn parent(&self, id: RecordId) -> Option<RecordId> {
        self.tree[id.index()].parent
    }

    /// Sets the record that holds `id`.
    pub fn set_parent(&mut self, id: RecordId, parent: Option<RecordId>) {
        self.tree[id.index()].parent = parent;
    }

    /// The flags of `id`.
    #[inline]
    pub fn flags(&self, id: RecordId) -> RecordFlags {
        self.tree[id.index()].flags
    }

    /// Sets the flags of `id`.
    pub fn set_flags(&mut self, id: RecordId, flags: RecordFlags) {
        let drawn = self.tree[id.index()].flags.only(RecordFlags::DRAWN);
        self.write_flags(id, flags);
        if self.tree[id.index()].flags.only(RecordFlags::DRAWN) != drawn {
            self.note_property_change(id);
        }
    }

    // Writes the flags of `id` for the scene builder, which notes the
    // change itself.
    pub(crate) fn write_flags(&mut self, id: RecordId, flags: RecordFlags) {
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
    #[inline]
    pub fn node_rect(&self, id: RecordId) -> Rect {
        self.geo[id.index()].node_rect()
    }

    /// Sets the node rect size of `id`.
    pub fn set_size(&mut self, id: RecordId, width: f32, height: f32) {
        if self.write_size(id, width, height) {
            self.note_property_change(id);
        }
    }

    // Writes the node rect size of `id` for the scene builder, which notes
    // the change itself; `false` when it stays.
    #[inline]
    pub(crate) fn write_size(&mut self, id: RecordId, width: f32, height: f32) -> bool {
        let index = id.index();
        let size = [width, height];
        if super::records::same_pair(self.geo[index].size, size) {
            return false;
        }
        self.geo[index].size = size;
        self.stale_with_parent(index);
        true
    }

    /// Sets what `id` draws for and the layout node it draws for.
    #[inline]
    pub fn set_kind(&mut self, id: RecordId, kind: RecordKind, node: Option<NodeId>) {
        let index = id.index();
        if self.tree[index].kind != kind || self.warm[index].node != node.map(RecordNode::of) {
            self.change_kind(id, kind, node);
        }
    }

    #[inline(never)]
    fn change_kind(&mut self, id: RecordId, kind: RecordKind, node: Option<NodeId>) {
        let index = id.index();
        self.log(RecordEvent::Released, id);
        self.tree[index].kind = kind;
        self.warm[index].node = node.map(RecordNode::of);
        self.stale_with_parent(index);
        self.log(RecordEvent::Claimed, id);
        self.touch(index);
    }

    /// Removes every item of `id`: its draws and the records beneath it.
    pub fn clear_items(&mut self, id: RecordId) {
        if self.tree[id.index()].items.len() != 0 {
            // The note stamps the record, so the removal lists it once.
            self.note_content_change(id);
            self.drop_items(id);
        }
    }

    // Removes every item of `id` for the scene builder, which notes the
    // change itself.
    pub(crate) fn drop_items(&mut self, id: RecordId) {
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
        super::facts::forget_own(self, index);
        self.stale(index);
        if span.len() != 0 {
            self.touch(index);
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
        super::facts::forget_own(self, index);
        self.touch(index);
        let tree = &mut self.tree[index];
        tree.content_rev = update;
        let raise = tree.subtree_rev != update;
        self.note_content_run(index);
        if raise {
            self.raise_subtree_revs(id);
        }
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
        self.touch(index);
        let tree = &mut self.tree[index];
        tree.content_rev = update;
        tree.subtree_rev = update;
        let parent = tree.parent;
        self.note_subtree_change(parent);
    }

    // Notes that the update in progress moved the record at `index`, or its
    // content within it. Its ancestors take the update as their subtree's.
    #[inline]
    pub(super) fn moved(&mut self, index: usize) {
        if self.tree[index].place_rev != self.update {
            self.touch(index);
            self.tree[index].place_rev = self.update;
            self.log(
                RecordEvent::Moved,
                RecordId::from_index(index).expect("a row is a record"),
            );
            self.note_subtree_change(self.tree[index].parent);
        }
    }

    // Raises the subtree revision of `from` and its ancestors to the update
    // in progress, and leaves their hashes to be worked out again, stopping
    // at the first ancestor that holds the update already.
    #[inline]
    pub(super) fn note_subtree_change(&mut self, from: Option<RecordId>) {
        if let Some(record) = from
            && self.tree[record.index()].subtree_rev != self.update
        {
            self.raise_subtree_revs(record);
        }
    }

    #[inline(never)]
    fn raise_subtree_revs(&mut self, from: RecordId) {
        let update = self.update;
        let mut current = Some(from);
        while let Some(record) = current {
            let index = record.index();
            let tree = &mut self.tree[index];
            if tree.subtree_rev == update {
                break;
            }
            tree.subtree_rev = update;
            current = tree.parent;
            self.note_content_run(index);
        }
    }

    // Counts the update in progress in the content run of the record at
    // `index`, which `LayerRef::content_churns` reads.
    #[cfg(feature = "window-damage")]
    #[inline]
    fn note_content_run(&mut self, index: usize) {
        if self.tree[index].flags.contains(RecordFlags::HAS_PROPS)
            && let Some(props) = self.warm[index].props
        {
            self.props[props.index()].content_run.note(self.update);
        }
    }

    // Without window damage no reader asks whether a record's content churns.
    #[cfg(not(feature = "window-damage"))]
    #[inline]
    fn note_content_run(&mut self, _index: usize) {}

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
