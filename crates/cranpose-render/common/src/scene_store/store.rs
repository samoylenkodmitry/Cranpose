// The records a renderer draws: a tree of layout nodes' layers, wrappers,
// draw groups and leaves, each a row of fixed-size records with its items in
// one span, its other properties and its draws in side arenas.

use std::sync::atomic::{AtomicU16, Ordering};

use cranpose_core::{NodeId, RecordId, RecordRef, SceneUpdate, StoreId};
use cranpose_ui::{Point, Rect};

use super::{
    GeoRecord, Item, ItemKind, ItemPool, ItemSpan, LayerProps, PropsId, ReachState, RecordFlags,
    RecordKind, Slab, TransformId, TreeRecord, WarmRecord,
};
use crate::graph::{DrawPrimitiveNode, DrawRunNode, ProjectiveTransform, TextPrimitiveNode};

/// The scene a renderer draws and the hit targets come from.
pub struct SceneStore {
    id: StoreId,
    tree: Vec<TreeRecord>,
    geo: Vec<GeoRecord>,
    warm: Vec<WarmRecord>,
    generations: Vec<u32>,
    free_rows: Vec<u32>,
    items: ItemPool,
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
    /// The layout node the record draws for.
    pub node: NodeId,
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
    /// items, at the origin of its parent and of zero size.
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
        if seed.parent.is_none() {
            self.root = Some(id);
        }
        id
    }

    /// Removes `id`, its draws, its properties and every record beneath it.
    pub fn remove_subtree(&mut self, id: RecordId) {
        let index = id.index();
        let span = self.tree[index].items;
        for position in 0..span.len() {
            let item = self.items.get(span)[position];
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

    // Removes what `item` holds: a child record's subtree or a draw.
    fn remove_item(&mut self, item: Item) {
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
        self.items.get(self.tree[id.index()].items)
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

    /// Removes the draw `item` holds; a child item stays.
    pub fn remove_draw(&mut self, item: Item) {
        if item.kind() != ItemKind::Child {
            self.remove_item(item);
        }
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
    pub fn node(&self, id: RecordId) -> NodeId {
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

    /// The transform from `id` to its parent's content.
    pub fn transform_to_parent(&self, id: RecordId) -> ProjectiveTransform {
        let index = id.index();
        match self.tree[index].transform {
            Some(transform) => self.transforms[transform.index()],
            None => {
                let [x, y] = self.geo[index].placement;
                ProjectiveTransform::translation(x, y)
            }
        }
    }

    /// Sets the transform from `id` to its parent's content: a translation
    /// stays in the record, anything more takes an entry of its own.
    pub fn set_transform_to_parent(&mut self, id: RecordId, transform: ProjectiveTransform) {
        let index = id.index();
        match translation_of(transform) {
            Some(translation) => {
                if let Some(old) = self.tree[index].transform.take() {
                    self.transforms.remove(old.index());
                }
                self.geo[index].placement = [translation.x, translation.y];
            }
            None => {
                let [[_, _, x], [_, _, y], _] = transform.matrix();
                self.geo[index].placement = [x, y];
                match self.tree[index].transform {
                    Some(old) => self.transforms[old.index()] = transform,
                    None => {
                        let entry = TransformId::from_index(self.transforms.insert(transform));
                        self.tree[index].transform = Some(entry);
                    }
                }
            }
        }
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

    /// Notes that update `update` changed what `id` draws: its own draws,
    /// properties or children. Its ancestors take the update as their
    /// subtree's.
    pub fn note_content_change(&mut self, id: RecordId) {
        let update = self.update;
        self.tree[id.index()].content_rev = update;
        self.tree[id.index()].reach_state = ReachState::Stale;
        self.note_subtree_change(id);
    }

    /// Notes that the update in progress moved `id` in its parent.
    pub fn note_place_change(&mut self, id: RecordId) {
        let update = self.update;
        self.tree[id.index()].place_rev = update;
        self.note_subtree_change(id);
    }

    // Raises the subtree revision of `id` and its ancestors to the update in
    // progress, stopping at the first ancestor that holds it already.
    fn note_subtree_change(&mut self, id: RecordId) {
        let update = self.update;
        let mut current = Some(id);
        while let Some(record) = current {
            let tree = &mut self.tree[record.index()];
            if tree.subtree_rev == update && record != id {
                break;
            }
            tree.subtree_rev = update;
            if record != id {
                tree.reach_state = ReachState::Stale;
            }
            current = tree.parent;
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

// The translation of a transform that only moves.
fn translation_of(transform: ProjectiveTransform) -> Option<Point> {
    let [[a, b, x], [c, d, y], [p, q, w]] = transform.matrix();
    (a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 && p == 0.0 && q == 0.0 && w == 1.0)
        .then(|| Point::new(x, y))
}
