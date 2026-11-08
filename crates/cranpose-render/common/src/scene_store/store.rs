// The records a renderer draws: a tree of layout nodes' layers, wrappers,
// draw groups and leaves, each a row of fixed-size records with its items in
// one span, its other properties and its draws in side arenas.

use std::sync::atomic::{AtomicU16, Ordering};

use cranpose_core::{NodeId, RecordId, RecordRef, SceneUpdate, StoreId};
use cranpose_ui::{Point, Rect};

use super::{
    GeoRecord, Item, ItemKind, ItemPool, ItemSpan, LayerProps, LayerRef, PropsId, ReachState,
    RecordFlags, RecordKind, Slab, TransformId, TreeRecord, WarmRecord,
};
use crate::graph::{DrawPrimitiveNode, DrawRunNode, ProjectiveTransform, TextPrimitiveNode};

/// The scene a renderer draws and the hit targets come from.
#[derive(Clone)]
pub struct SceneStore {
    id: StoreId,
    pub(super) tree: Vec<TreeRecord>,
    pub(super) geo: Vec<GeoRecord>,
    pub(super) warm: Vec<WarmRecord>,
    generations: Vec<u32>,
    free_rows: Vec<u32>,
    pub(super) items: ItemPool,
    props: Slab<LayerProps>,
    transforms: Slab<ProjectiveTransform>,
    draws: Slab<DrawPrimitiveNode>,
    texts: Slab<TextPrimitiveNode>,
    runs: Slab<DrawRunNode>,
    root: Option<RecordId>,
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
            items: ItemPool::default(),
            props: Slab::default(),
            transforms: Slab::default(),
            draws: Slab::default(),
            texts: Slab::default(),
            runs: Slab::default(),
            root: None,
            update: SceneUpdate::default(),
        }
    }

    /// The store's identity among the renderers' stores.
    pub fn id(&self) -> StoreId {
        self.id
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
        self.update = self.update.next();
        self.update
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

    /// The record `id` as its readers see it.
    pub fn layer(&self, id: RecordId) -> LayerRef<'_> {
        LayerRef::new(self, id)
    }

    /// The root record as its readers see it.
    pub fn root_layer(&self) -> Option<LayerRef<'_>> {
        self.root.map(|root| self.layer(root))
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
        self.transforms.clear();
        self.draws.clear();
        self.texts.clear();
        self.runs.clear();
        self.root = None;
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
        id
    }

    /// Removes `id`, its draws, its properties and every record beneath it.
    pub fn remove_subtree(&mut self, id: RecordId) {
        let index = id.index();
        let span = self.tree[index].items;
        for position in 0..span.len() {
            let item = self.items.span(span)[position];
            self.remove_item(item);
        }
        self.items.free_span(span);
        if let Some(props) = self.warm[index].props.take() {
            self.props.remove(props.index());
        }
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
                let origin = self.origin(id);
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
        let origin = self.origin(id);
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
        self.tree[index].kind = kind;
        self.warm[index].node = node;
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

    /// Sets the properties of `id`; `None` removes them.
    pub fn set_props(&mut self, id: RecordId, props: Option<LayerProps>) {
        let index = id.index();
        match (self.warm[index].props, props) {
            (Some(entry), Some(props)) => self.props[entry.index()] = props,
            (Some(entry), None) => {
                self.props.remove(entry.index());
                self.warm[index].props = None;
            }
            (None, Some(props)) => {
                self.warm[index].props = Some(PropsId::from_index(self.props.insert(props)));
            }
            (None, None) => {}
        }
    }

    /// The properties of `id` to change in place, when it has any.
    pub fn props_mut(&mut self, id: RecordId) -> Option<&mut LayerProps> {
        self.warm[id.index()]
            .props
            .map(|props| &mut self.props[props.index()])
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
