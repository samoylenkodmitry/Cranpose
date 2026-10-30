use std::{any::TypeId, cmp::Reverse, collections::BinaryHeap};

use smallvec::SmallVec;

use crate::{
    Applier, Node, NodeError, NodeId, NodeMarks, RecycleOrigin, RecyclePool, RecycledNode,
    RuntimeHandle, bubble_passes,
    collections::map::{HashMap, HashSet},
    layout_dirt_roots,
    node_marks::MarkPlace,
    stable_index,
};

mod applier_impl;

/// The stock [`Applier`]: it keeps nodes in memory under ids that stay stable for the life of the
/// node.
#[derive(Default)]
pub struct MemoryApplier {
    /// Behind one allocation, so lending the applier to a layout pass moves a
    /// pointer.
    pub(crate) state: Box<MemoryApplierState>,
}

#[derive(Default)]
pub(crate) struct MemoryApplierState {
    pub(crate) nodes: Vec<Option<Box<dyn Node>>>,
    pub(crate) physical_stable_ids: Vec<u32>,
    pub(crate) physical_warm_recycled_origins: Vec<RecycleOrigin>,
    pub(crate) stable_index: stable_index::StableIndex,
    pub(crate) free_ids: BinaryHeap<Reverse<usize>>,
    pub(crate) high_ids: HashMap<NodeId, HighIdSlot>,
    pub(crate) next_stable_id: NodeId,
    pub(crate) layout_runtime: Option<RuntimeHandle>,
    pub(crate) recycle_pools: HashMap<TypeId, RecyclePool>,
    pub(crate) structural_change_parents: Vec<NodeId>,
    pub(crate) virtual_node_ids: HashSet<NodeId>,
    pub(crate) bubble_passes: bubble_passes::BubblePasses,
    pub(crate) layout_dirt_roots: layout_dirt_roots::LayoutDirtRoots,
}

// A node stored under an id too high for the dense table: the node while it
// lives, whether it came from a warm shell, and the generation its id has
// reached, which stays after the node goes.
#[derive(Default)]
pub(crate) struct HighIdSlot {
    node: Option<Box<dyn Node>>,
    origin: RecycleOrigin,
    generation: u32,
}

/// Reusable ancestry storage for [`MemoryApplier::scene_nodes_attached_to_into`].
///
/// Attachment results are kept only for one batch, so callers can reuse this
/// value without keeping stale attachment results after graph mutations.
#[derive(Default)]
pub struct SceneNodeAttachmentScratch {
    attached: NodeMarks,
    path: Vec<MarkPlace>,
}

const ATTACHED: u8 = 1;
const DETACHED: u8 = 2;

struct RemovalFrame {
    node_id: NodeId,
    children: SmallVec<[NodeId; 8]>,
    next_child: usize,
}

/// Sizes of a [`MemoryApplier`]'s tables, for leak and memory checks. A `_len` field counts entries
/// and a `_cap` field the capacity reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryApplierDebugStats {
    /// The id that the next created node gets.
    pub next_stable_id: NodeId,
    /// Live nodes.
    pub nodes_len: usize,
    /// Slots in the dense node table, both live and free.
    pub nodes_cap: usize,
    /// Entries in the table that gives each slot its stable id.
    pub physical_stable_ids_len: usize,
    /// Capacity reserved for the stable ids of table slots.
    pub physical_stable_ids_cap: usize,
    /// Live entries in the index from stable ids to table slots.
    pub stable_to_physical_len: usize,
    /// Capacity reserved for the index from stable ids to table slots.
    pub stable_to_physical_cap: usize,
    /// Stable ids that carry a generation counter.
    pub stable_generations_len: usize,
    /// Capacity reserved for the generation counters of stable ids.
    pub stable_generations_cap: usize,
    /// Dense table slots free for reuse.
    pub free_ids_len: usize,
    /// Capacity reserved for the free slot ids.
    pub free_ids_cap: usize,
    /// Nodes stored under ids too high for the dense table.
    pub high_id_nodes_len: usize,
    /// Capacity reserved for the high-id nodes.
    pub high_id_nodes_cap: usize,
    /// Generation counters kept for high-id nodes.
    pub high_id_generations_len: usize,
    /// Capacity reserved for the generations of high-id nodes.
    pub high_id_generations_cap: usize,
    /// Node types that have recycled nodes pooled.
    pub recycled_type_count: usize,
    /// Capacity reserved for the pools of recycled nodes.
    pub recycled_type_cap: usize,
    /// Pooled recycled nodes of all types.
    pub recycled_node_count: usize,
    /// Capacity reserved for the pooled recycled nodes.
    pub recycled_node_capacity: usize,
    /// Nodes recycled through the warm path, both live and pooled.
    pub warm_recycled_node_id_count: usize,
    /// Capacity reserved for the warm recycled nodes.
    pub warm_recycled_node_id_capacity: usize,
}

impl MemoryApplier {
    const EAGER_COMPACT_NODE_LEN: usize = 1_024;
    pub(crate) const HIGH_ID_THRESHOLD: NodeId = NodeId(1_000_000_000);
    const INVALID_STABLE_ID: u32 = u32::MAX;
    const INITIAL_DENSE_NODE_CAP: usize = 32;
    const LARGE_DENSE_NODE_GROWTH_THRESHOLD: usize = 32 * 1024;
    const LARGE_DENSE_NODE_GROWTH_DIVISOR: usize = 4;

    fn pack_stable_id(stable_id: NodeId) -> u32 {
        u32::try_from(stable_id.0).expect("stable id overflow")
    }

    fn unpack_stable_id(stable_id: u32) -> NodeId {
        NodeId(stable_id as usize)
    }

    fn next_dense_node_target_len(old_len: usize) -> usize {
        if old_len < Self::INITIAL_DENSE_NODE_CAP {
            return Self::INITIAL_DENSE_NODE_CAP;
        }
        if old_len < Self::LARGE_DENSE_NODE_GROWTH_THRESHOLD {
            return old_len.saturating_mul(2);
        }

        let incremental_growth =
            (old_len / Self::LARGE_DENSE_NODE_GROWTH_DIVISOR).max(Self::INITIAL_DENSE_NODE_CAP);
        old_len.saturating_add(incremental_growth)
    }

    fn ensure_dense_node_storage_capacity(&mut self) {
        let len = self
            .state
            .nodes
            .len()
            .max(self.state.physical_stable_ids.len())
            .max(self.state.physical_warm_recycled_origins.len());
        if len < self.state.nodes.capacity()
            && len < self.state.physical_stable_ids.capacity()
            && len < self.state.physical_warm_recycled_origins.capacity()
        {
            return;
        }

        let target = Self::next_dense_node_target_len(len);
        if self.state.nodes.capacity() < target {
            self.state
                .nodes
                .reserve_exact(target.saturating_sub(self.state.nodes.len()));
        }
        if self.state.physical_stable_ids.capacity() < target {
            self.state
                .physical_stable_ids
                .reserve_exact(target.saturating_sub(self.state.physical_stable_ids.len()));
        }
        if self.state.physical_warm_recycled_origins.capacity() < target {
            self.state.physical_warm_recycled_origins.reserve_exact(
                target.saturating_sub(self.state.physical_warm_recycled_origins.len()),
            );
        }
    }

    /// Creates an applier with no nodes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves a scene-scope candidate the way structural records are
    /// resolved: to its nearest non-virtual ancestor, and only while still
    /// attached to `root`. A node detached after recording must not reach the
    /// scoped scene update — an id the graph cannot resolve forces it to give
    /// up and rebuild the whole scene.
    pub fn scene_node_attached_to(&mut self, node_id: NodeId, root: NodeId) -> Option<NodeId> {
        let resolved = self.first_non_virtual_ancestor(node_id)?;
        self.is_attached_to(resolved, root).then_some(resolved)
    }

    /// [`Self::scene_node_attached_to`] for each of `nodes`, in order, with
    /// every ancestor looked up at most once: a batch costs the distinct
    /// ancestors of its nodes, not each node's depth.
    pub fn scene_nodes_attached_to(
        &mut self,
        nodes: impl IntoIterator<Item = NodeId>,
        root: NodeId,
    ) -> Vec<Option<NodeId>> {
        let mut result = Vec::new();
        self.scene_nodes_attached_to_into(
            nodes,
            root,
            &mut result,
            &mut SceneNodeAttachmentScratch::default(),
        );
        result
    }

    /// Fills `output` with [`Self::scene_nodes_attached_to`] results, reusing
    /// its storage and the ancestry cache in `scratch` across batches.
    ///
    /// Each distinct ancestor is inspected at most once per call. The cache
    /// is cleared for each call, so graph mutations are observed by the next
    /// batch.
    ///
    /// ```
    /// use cranpose_core::{MemoryApplier, NodeId, SceneNodeAttachmentScratch};
    ///
    /// let mut applier = MemoryApplier::new();
    /// let mut scratch = SceneNodeAttachmentScratch::default();
    /// let mut attached: Vec<Option<NodeId>> = Vec::new();
    /// let root = NodeId::default();
    /// applier.scene_nodes_attached_to_into([], root, &mut attached, &mut scratch);
    /// assert!(attached.is_empty());
    /// ```
    pub fn scene_nodes_attached_to_into(
        &mut self,
        nodes: impl IntoIterator<Item = NodeId>,
        root: NodeId,
        output: &mut Vec<Option<NodeId>>,
        scratch: &mut SceneNodeAttachmentScratch,
    ) {
        let nodes = nodes.into_iter();
        output.clear();
        let (lower_bound, upper_bound) = nodes.size_hint();
        if lower_bound == 0 && upper_bound == Some(0) {
            scratch.path.clear();
            return;
        }
        scratch.attached.reset(self);
        scratch.attached.set(self, root, ATTACHED);
        output.reserve(lower_bound);
        scratch.path.clear();
        for node_id in nodes {
            let Some(resolved) = self.first_non_virtual_ancestor(node_id) else {
                output.push(None);
                continue;
            };
            let mut current = MarkPlace::of(self, resolved);
            scratch.path.clear();
            let answer = loop {
                match scratch.attached.mark_at(current) {
                    ATTACHED => break true,
                    DETACHED => break false,
                    _ => {}
                }
                scratch.path.push(current);
                match self.parent_at(current) {
                    Some(parent) if scratch.path.len() < 100_000 => {
                        current = MarkPlace::of(self, parent);
                    }
                    _ => break false,
                }
            };
            let mark = if answer { ATTACHED } else { DETACHED };
            for visited in scratch.path.drain(..) {
                *scratch.attached.entry_at(visited) = mark;
            }
            output.push(answer.then_some(resolved));
        }
    }

    /// Drains the parents recorded via [`Applier::record_structural_change`],
    /// keeping only nodes still attached to `root` (a parent that was itself
    /// removed is covered by its own surviving ancestor's record). A virtual
    /// parent — a subcompose slot wrapper the render graph never contains —
    /// is reported as its nearest non-virtual ancestor: that is the node
    /// whose graph child set the change altered, and an id the graph cannot
    /// resolve would force the scoped scene update to give up and rebuild.
    pub fn take_structural_change_parents_attached_to(&mut self, root: NodeId) -> Vec<NodeId> {
        let mut candidates = Vec::new();
        self.take_structural_change_parents_into(&mut candidates);
        let mut seen = HashSet::<NodeId>::default();
        candidates.retain_mut(|parent_id| {
            let Some(resolved) = self.first_non_virtual_ancestor(*parent_id) else {
                return false;
            };
            *parent_id = resolved;
            self.is_attached_to(resolved, root) && seen.insert(resolved)
        });
        candidates
    }

    /// Takes the recorded structural-change candidates into reusable storage.
    /// Candidates are not resolved or attachment-filtered; callers that route
    /// them into scenes should use [`Self::scene_nodes_attached_to_into`].
    ///
    /// ```
    /// use cranpose_core::MemoryApplier;
    ///
    /// let mut applier = MemoryApplier::new();
    /// let mut nodes = Vec::new();
    /// applier.take_structural_change_parents_into(&mut nodes);
    /// assert!(nodes.is_empty());
    /// ```
    pub fn take_structural_change_parents_into(&mut self, output: &mut Vec<NodeId>) {
        output.clear();
        std::mem::swap(&mut self.state.structural_change_parents, output);
    }

    fn first_non_virtual_ancestor(&mut self, node_id: NodeId) -> Option<NodeId> {
        if self.state.virtual_node_ids.is_empty() {
            return Some(node_id);
        }
        let mut current = node_id;
        for _ in 0..100_000 {
            if !self.state.virtual_node_ids.contains(&current) {
                return Some(current);
            }
            match self.get_mut(current) {
                Ok(node) => current = node.parent()?,
                Err(_) => return None,
            }
        }
        None
    }

    fn is_attached_to(&mut self, node_id: NodeId, root: NodeId) -> bool {
        let mut current = node_id;
        for _ in 0..100_000 {
            if current == root {
                return true;
            }
            match self.get_mut(current) {
                Ok(node) => match node.parent() {
                    Some(parent) => current = parent,
                    None => return false,
                },
                Err(_) => return false,
            }
        }
        false
    }

    /// Runs `f` on the node `id` as an `N`. Returns an error if the node does not exist or is not
    /// an `N`.
    pub fn with_node<N: Node + 'static, R>(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> Result<R, NodeError> {
        let typed = self
            .get_mut(id)?
            .as_any_mut()
            .downcast_mut::<N>()
            .ok_or_else(|| NodeError::TypeMismatch {
                id,
                expected: std::any::type_name::<N>(),
            })?;
        Ok(f(typed))
    }

    /// Returns the number of live nodes.
    pub fn len(&self) -> usize {
        self.state.nodes.iter().filter(|n| n.is_some()).count()
    }

    /// Returns the number of slots in the dense node table, both live and free.
    pub fn capacity(&self) -> usize {
        self.state.nodes.len()
    }

    /// Returns the number of free slots in the dense node table.
    pub fn tombstone_count(&self) -> usize {
        self.state.nodes.iter().filter(|n| n.is_none()).count()
    }

    /// Returns the number of freed table slots that new nodes can reuse.
    pub fn freelist_len(&self) -> usize {
        self.state.free_ids.len()
    }

    /// Returns the number of pooled recycled nodes of all types.
    pub fn debug_recycled_node_count(&self) -> usize {
        self.total_recycled_node_count()
    }

    /// Returns the number of pooled recycled nodes of type `N`.
    pub fn debug_recycled_node_count_for<N: Node + 'static>(&self) -> usize {
        let key = TypeId::of::<N>();
        self.state
            .recycle_pools
            .get(&key)
            .map_or(0, RecyclePool::len)
    }

    /// Returns the sizes of the applier's tables, for leak and memory checks.
    pub fn debug_stats(&self) -> MemoryApplierDebugStats {
        MemoryApplierDebugStats {
            next_stable_id: self.state.next_stable_id,
            nodes_len: self.len(),
            nodes_cap: self.state.nodes.len(),
            physical_stable_ids_len: self.state.physical_stable_ids.len(),
            physical_stable_ids_cap: self.state.physical_stable_ids.capacity(),
            stable_to_physical_len: self.state.stable_index.live(),
            stable_to_physical_cap: self.state.stable_index.capacity(),
            stable_generations_len: self.state.stable_index.occupied(),
            stable_generations_cap: self.state.stable_index.capacity(),
            free_ids_len: self.state.free_ids.len(),
            free_ids_cap: self.state.free_ids.capacity(),
            high_id_nodes_len: self.high_id_nodes().count(),
            high_id_nodes_cap: self.state.high_ids.capacity(),
            high_id_generations_len: self.state.high_ids.len(),
            high_id_generations_cap: self.state.high_ids.capacity(),
            recycled_type_count: self
                .state
                .recycle_pools
                .values()
                .filter(|pool| pool.len() > 0)
                .count(),
            recycled_type_cap: self.state.recycle_pools.capacity(),
            recycled_node_count: self.total_recycled_node_count(),
            recycled_node_capacity: self.total_recycled_node_capacity(),
            warm_recycled_node_id_count: self.total_warm_recycled_node_id_count(),
            warm_recycled_node_id_capacity: self.total_warm_recycled_node_id_capacity(),
        }
    }

    /// Returns `true` if the applier holds no live nodes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Calls `visit` with every live node, dense and high-id alike.
    pub fn for_each_node_mut(&mut self, mut visit: impl FnMut(&mut dyn Node)) {
        for node in self.state.nodes.iter_mut().flatten() {
            visit(node.as_mut());
        }
        for node in self
            .state
            .high_ids
            .values_mut()
            .filter_map(|slot| slot.node.as_mut())
        {
            visit(node.as_mut());
        }
    }

    /// Returns the heap bytes that the live nodes take, including what they own.
    pub fn debug_live_node_heap_bytes(&self) -> usize {
        let dense_nodes = self
            .state
            .nodes
            .iter()
            .flatten()
            .map(|node| size_of_val(&**node) + node.debug_heap_bytes())
            .sum::<usize>();
        let high_id_nodes = self
            .high_id_nodes()
            .map(|node| size_of_val(node) + node.debug_heap_bytes())
            .sum::<usize>();
        dense_nodes + high_id_nodes
    }

    /// Returns the heap bytes that the pooled recycled nodes take, including what they own.
    pub fn debug_recycled_node_heap_bytes(&self) -> usize {
        self.state
            .recycle_pools
            .values()
            .flat_map(RecyclePool::nodes)
            .map(|node| size_of_val(&*node.node) + node.node.debug_heap_bytes())
            .sum::<usize>()
    }

    /// Gives the applier the runtime of its composition, so code that runs during layout with only
    /// the applier in hand can reach the runtime.
    pub fn set_runtime_handle(&mut self, handle: RuntimeHandle) {
        self.state.layout_runtime = Some(handle);
    }

    /// Removes the runtime handle.
    pub fn clear_runtime_handle(&mut self) {
        self.state.layout_runtime = None;
    }

    /// Returns the runtime handle set with `set_runtime_handle`, if any.
    pub fn runtime_handle(&self) -> Option<RuntimeHandle> {
        self.state.layout_runtime.clone()
    }

    fn total_recycled_node_count(&self) -> usize {
        self.state
            .recycle_pools
            .values()
            .map(RecyclePool::len)
            .sum()
    }

    fn total_recycled_node_capacity(&self) -> usize {
        self.state
            .recycle_pools
            .values()
            .map(RecyclePool::capacity)
            .sum()
    }

    fn total_warm_recycled_node_id_count(&self) -> usize {
        self.live_warm_recycled_origin_count()
            + self
                .state
                .recycle_pools
                .values()
                .map(|pool| pool.warm.len() + pool.returning.len())
                .sum::<usize>()
    }

    fn total_warm_recycled_node_id_capacity(&self) -> usize {
        self.live_warm_recycled_origin_capacity()
            + self
                .state
                .recycle_pools
                .values()
                .map(|pool| pool.warm.capacity() + pool.returning.capacity())
                .sum::<usize>()
    }

    fn remember_recycle_pool_limit(&mut self, key: TypeId, recycle_pool_limit: Option<usize>) {
        if recycle_pool_limit.is_some() || self.state.recycle_pools.contains_key(&key) {
            self.state.recycle_pools.entry(key).or_default().limit = recycle_pool_limit;
        }
    }

    fn warm_recycled_pool_len(&self, key: TypeId) -> usize {
        self.state
            .recycle_pools
            .get(&key)
            .map_or(0, |pool| pool.warm.len())
    }

    fn live_warm_recycled_origin_count(&self) -> usize {
        self.state
            .physical_warm_recycled_origins
            .iter()
            .zip(self.state.nodes.iter())
            .filter(|(origin, node)| **origin == RecycleOrigin::Warm && node.is_some())
            .count()
            + self
                .state
                .high_ids
                .values()
                .filter(|slot| slot.origin == RecycleOrigin::Warm && slot.node.is_some())
                .count()
    }

    fn high_id_nodes(&self) -> impl Iterator<Item = &dyn Node> {
        self.state
            .high_ids
            .values()
            .filter_map(|slot| slot.node.as_deref())
    }

    fn live_warm_recycled_origin_capacity(&self) -> usize {
        self.state.physical_warm_recycled_origins.capacity() + self.state.high_ids.capacity()
    }

    fn push_recycled_node(
        &mut self,
        key: TypeId,
        recycle_pool_limit: Option<usize>,
        recycled: RecycledNode,
    ) {
        self.remember_recycle_pool_limit(key, recycle_pool_limit);
        let pool = self.state.recycle_pools.entry(key).or_default();
        pool.remember_prototype(recycled.node.as_ref());
        pool.push(recycled);
    }

    fn seed_recycled_node_shell_impl(
        &mut self,
        key: TypeId,
        recycle_pool_limit: Option<usize>,
        shell: Box<dyn Node>,
    ) {
        let limit = recycle_pool_limit.unwrap_or(usize::MAX);
        if self.warm_recycled_pool_len(key) >= limit {
            return;
        }

        self.remember_recycle_pool_limit(key, recycle_pool_limit);
        let stable_id = self.state.next_stable_id;
        self.state.next_stable_id = self.state.next_stable_id.next();
        let pool = self.state.recycle_pools.entry(key).or_default();
        pool.remember_prototype(shell.as_ref());
        pool.push_warm(RecycledNode::from_shell(
            stable_id,
            shell,
            RecycleOrigin::Warm,
        ));
    }

    fn prune_stable_generations(&mut self) {
        let retained_len = self.state.stable_index.live() + self.total_recycled_node_count();
        if retained_len == self.state.stable_index.occupied() {
            return;
        }

        let recycled: HashSet<NodeId> = self
            .state
            .recycle_pools
            .values()
            .flat_map(RecyclePool::nodes)
            .map(RecycledNode::stable_id)
            .collect();
        self.state
            .stable_index
            .retain_retired(|stable_id| recycled.contains(&stable_id));
    }

    /// Returns an indented text outline of the tree below `root` for debugging, or a `(no root)`
    /// line when `root` is `None`.
    pub fn dump_tree(&self, root: Option<NodeId>) -> String {
        let mut output = String::new();
        if let Some(root_id) = root {
            self.dump_node(&mut output, root_id, 0);
        } else {
            output.push_str("(no root)\n");
        }
        output
    }

    fn dump_node(&self, output: &mut String, id: NodeId, depth: usize) {
        let indent = "  ".repeat(depth);
        let Ok(node) = self.get_ref(id) else {
            if let Some(physical_id) = self.resolve_node_index(id) {
                output.push_str(&format!(
                    "{indent}[{id}] (missing physical node {physical_id})\n"
                ));
            } else {
                output.push_str(&format!("{indent}[{id}] (missing)\n"));
            }
            return;
        };
        let type_name = std::any::type_name_of_val(node);
        output.push_str(&format!("{indent}[{id}] {type_name}\n"));

        let mut children = SmallVec::<[NodeId; 8]>::new();
        node.collect_children_into(&mut children);
        for child_id in children {
            self.dump_node(output, child_id, depth + 1);
        }
    }

    /// Moves into `out` the nodes the layout dirty bubbles started from
    /// since the last call, for a layout pass that starts from them, and
    /// forgets them. `false` when there were more than the applier keeps:
    /// `out` is then empty and the pass walks down from the root.
    pub fn take_layout_dirt_roots(&mut self, out: &mut Vec<NodeId>) -> bool {
        self.state.layout_dirt_roots.take_into(out)
    }

    #[inline]
    pub(crate) fn resolve_node_index(&self, id: NodeId) -> Option<usize> {
        self.state.stable_index.slot(id)
    }

    fn contains_node_id(&self, id: NodeId) -> bool {
        self.resolve_node_index(id).is_some() || self.high_id_node(id).is_ok()
    }

    fn insert_high_id_node(
        &mut self,
        stable_id: NodeId,
        node: Box<dyn Node>,
        origin: RecycleOrigin,
    ) {
        let slot = self.state.high_ids.entry(stable_id).or_default();
        slot.node = Some(node);
        slot.origin = origin;
    }

    fn insert_available_with_id(&mut self, stable_id: NodeId, node: Box<dyn Node>) {
        if stable_id >= Self::HIGH_ID_THRESHOLD {
            self.insert_high_id_node(stable_id, node, RecycleOrigin::Cold);
            return;
        }

        let physical_id = if let Some(Reverse(free_physical_id)) = self.state.free_ids.pop() {
            self.state.nodes[free_physical_id] = Some(node);
            self.state.physical_stable_ids[free_physical_id] = Self::pack_stable_id(stable_id);
            self.state.physical_warm_recycled_origins[free_physical_id] = RecycleOrigin::Cold;
            free_physical_id
        } else {
            self.ensure_dense_node_storage_capacity();
            let physical_id = self.state.nodes.len();
            self.state.nodes.push(Some(node));
            self.state
                .physical_stable_ids
                .push(Self::pack_stable_id(stable_id));
            self.state
                .physical_warm_recycled_origins
                .push(RecycleOrigin::Cold);
            physical_id
        };

        self.state.next_stable_id = self.state.next_stable_id.max(stable_id.next());
        self.state.physical_stable_ids[physical_id] = Self::pack_stable_id(stable_id);
        self.state.stable_index.set_slot(stable_id, physical_id);
    }

    /// Borrows a live node without requiring exclusive access to the tree.
    /// Returns [`NodeError::Missing`] when `id` does not name a live node.
    #[inline]
    pub fn get_ref(&self, id: NodeId) -> Result<&dyn Node, NodeError> {
        match self.resolve_node_index(id) {
            Some(physical_id) => self
                .state
                .nodes
                .get(physical_id)
                .and_then(Option::as_deref)
                .ok_or(NodeError::Missing { id }),
            None => self.high_id_node(id),
        }
    }

    // The parent of the node at `place`, `None` for a root or a node that
    // is gone.
    #[inline]
    pub(crate) fn parent_at(&self, place: MarkPlace) -> Option<NodeId> {
        match place {
            MarkPlace::Dense(slot) => self.state.nodes.get(slot)?.as_deref()?.parent(),
            MarkPlace::Elsewhere(id) => self.high_id_node(id).ok()?.parent(),
        }
    }

    pub(crate) fn high_id_node(&self, id: NodeId) -> Result<&dyn Node, NodeError> {
        self.state
            .high_ids
            .get(&id)
            .and_then(|slot| slot.node.as_deref())
            .ok_or(NodeError::Missing { id })
    }

    fn high_id_node_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        self.state
            .high_ids
            .get_mut(&id)
            .and_then(|slot| slot.node.as_deref_mut())
            .ok_or(NodeError::Missing { id })
    }

    fn node_parent(&self, id: NodeId) -> Result<Option<NodeId>, NodeError> {
        Ok(self.get_ref(id)?.parent())
    }

    fn collect_owned_children(
        &self,
        node_id: NodeId,
        out: &mut SmallVec<[NodeId; 8]>,
    ) -> Result<(), NodeError> {
        self.get_ref(node_id)?.collect_owned_children_into(out);
        out.retain(|child_id| {
            self.node_parent(*child_id)
                .is_ok_and(|parent| parent == Some(node_id))
        });
        Ok(())
    }

    fn remove_node_storage(&mut self, node_id: NodeId) -> Result<(), NodeError> {
        self.state.virtual_node_ids.remove(&node_id);
        if let Some(slot) = self.state.high_ids.get_mut(&node_id)
            && let Some(node) = slot.node.take()
        {
            let origin = std::mem::take(&mut slot.origin);
            slot.generation = slot.generation.wrapping_add(1);
            if let Some(key) = node.recycle_key() {
                let recycle_pool_limit = node.recycle_pool_limit();
                self.push_recycled_node(
                    key,
                    recycle_pool_limit,
                    RecycledNode::new(node_id, node, origin),
                );
            }
            return Ok(());
        }

        let physical_id = self
            .resolve_node_index(node_id)
            .ok_or(NodeError::Missing { id: node_id })?;
        if let Some(node) = self.state.nodes[physical_id].take()
            && let Some(key) = node.recycle_key()
        {
            let recycle_pool_limit = node.recycle_pool_limit();
            let origin = self
                .state
                .physical_warm_recycled_origins
                .get_mut(physical_id)
                .map(std::mem::take)
                .unwrap_or_default();
            self.push_recycled_node(
                key,
                recycle_pool_limit,
                RecycledNode::new(node_id, node, origin),
            );
        }
        self.state.physical_stable_ids[physical_id] = Self::INVALID_STABLE_ID;
        self.state.stable_index.release(node_id);
        self.state.free_ids.push(Reverse(physical_id));
        Ok(())
    }

    fn remove_subtree_postorder(&mut self, id: NodeId) -> Result<usize, NodeError> {
        self.get_ref(id)?;

        let mut root_children = SmallVec::<[NodeId; 8]>::new();
        self.collect_owned_children(id, &mut root_children)?;

        let mut stack = Vec::new();
        stack.push(RemovalFrame {
            node_id: id,
            children: root_children,
            next_child: 0,
        });
        let mut max_depth = stack.len();

        while let Some(frame) = stack.last_mut() {
            if frame.next_child < frame.children.len() {
                let child_id = frame.children[frame.next_child];
                frame.next_child += 1;

                if let Ok(child) = self.get_mut(child_id) {
                    child.on_removed_from_parent();
                    child.unmount();
                }

                let mut child_children = SmallVec::<[NodeId; 8]>::new();
                self.collect_owned_children(child_id, &mut child_children)?;
                stack.push(RemovalFrame {
                    node_id: child_id,
                    children: child_children,
                    next_child: 0,
                });
                max_depth = max_depth.max(stack.len());
                continue;
            }

            let node_id = frame.node_id;
            stack.pop();
            self.remove_node_storage(node_id)?;
        }

        Ok(max_depth)
    }

    #[cfg(test)]
    pub(crate) fn debug_remove_max_traversal_depth(
        &mut self,
        id: NodeId,
    ) -> Result<usize, NodeError> {
        self.remove_subtree_postorder(id)
    }
}
