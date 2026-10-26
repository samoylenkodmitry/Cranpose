//! The chain of modifier nodes a layout node holds, and its reconciliation
//! with the node's modifier.

use std::{any::TypeId, cell::RefCell, rc::Rc};

use cranpose_core::collections::map::HashMap;

use super::{
    AnyModifierElement, DynModifierElement, InvalidationKind, MAX_DELEGATE_DEPTH,
    ModifierChainIter, ModifierChainNodeRef, ModifierNode, ModifierNodeContext, NodeCapabilities,
    NodePath, chain_node_ref::ChainPosition,
};

/// One node of a chain and the element it last took. The element's hash,
/// which matches entries when a chain rebuilds, is worked out then: an
/// element updated in place on every frame is not hashed for it.
#[derive(Debug)]
struct ModifierNodeEntry {
    key: Option<u64>,
    element: DynModifierElement,
    node: Rc<RefCell<dyn ModifierNode>>,
    capabilities: NodeCapabilities,
}

impl ModifierNodeEntry {
    fn new(
        key: Option<u64>,
        element: DynModifierElement,
        node: Rc<RefCell<dyn ModifierNode>>,
        capabilities: NodeCapabilities,
    ) -> Self {
        let entry = Self {
            key,
            element,
            node,
            capabilities,
        };
        entry
            .node
            .borrow()
            .node_state()
            .set_capabilities(entry.capabilities);
        entry
    }
}

fn visit_node_tree_mut(
    node: &mut dyn ModifierNode,
    visitor: &mut dyn FnMut(&mut dyn ModifierNode),
) {
    visitor(node);
    node.for_each_delegate_mut(&mut |child| visit_node_tree_mut(child, visitor));
}

fn nth_delegate(node: &dyn ModifierNode, target: usize) -> Option<&dyn ModifierNode> {
    let mut current = 0usize;
    let mut result: Option<&dyn ModifierNode> = None;
    node.for_each_delegate(&mut |child| {
        if result.is_none() && current == target {
            result = Some(child);
        }
        current += 1;
    });
    result
}

fn nth_delegate_mut(node: &mut dyn ModifierNode, target: usize) -> Option<&mut dyn ModifierNode> {
    let mut current = 0usize;
    let mut result: Option<&mut dyn ModifierNode> = None;
    node.for_each_delegate_mut(&mut |child| {
        if result.is_none() && current == target {
            result = Some(child);
        }
        current += 1;
    });
    result
}

fn with_node_context<F, R>(
    node: &mut dyn ModifierNode,
    context: &mut dyn ModifierNodeContext,
    f: F,
) -> R
where
    F: FnOnce(&mut dyn ModifierNode, &mut dyn ModifierNodeContext) -> R,
{
    context.push_active_capabilities(node.node_state().capabilities());
    let result = f(node, context);
    context.pop_active_capabilities();
    result
}

fn request_update_auto_invalidations(
    element: &dyn AnyModifierElement,
    context: &mut dyn ModifierNodeContext,
    capabilities: NodeCapabilities,
) {
    if let Some(kind) = element.update_invalidation_kind() {
        let capabilities = NodeCapabilities::for_invalidation(kind);
        context.push_active_capabilities(capabilities);
        context.invalidate(kind);
        context.pop_active_capabilities();
    } else if element.auto_invalidates_on_update() {
        request_auto_invalidations(context, capabilities);
    }
}

fn request_auto_invalidations(
    context: &mut dyn ModifierNodeContext,
    capabilities: NodeCapabilities,
) {
    if capabilities.is_empty() {
        return;
    }

    context.push_active_capabilities(capabilities);

    if capabilities.contains(NodeCapabilities::LAYOUT) {
        context.invalidate(InvalidationKind::Layout);
    }
    if capabilities.contains(NodeCapabilities::DRAW) {
        context.invalidate(InvalidationKind::Draw);
    }
    if capabilities.contains(NodeCapabilities::POINTER_INPUT) {
        context.invalidate(InvalidationKind::PointerInput);
    }
    if capabilities.contains(NodeCapabilities::SEMANTICS) {
        context.invalidate(InvalidationKind::Semantics);
    }
    if capabilities.contains(NodeCapabilities::FOCUS) {
        context.invalidate(InvalidationKind::Focus);
    }

    context.pop_active_capabilities();
}

fn attach_node_tree(node: &mut dyn ModifierNode, context: &mut dyn ModifierNodeContext) {
    visit_node_tree_mut(node, &mut |n| {
        if !n.node_state().is_attached() {
            n.node_state().set_attached(true);
            with_node_context(n, context, |node, ctx| node.on_attach(ctx));
        }
    });
}

fn reset_node_tree(node: &mut dyn ModifierNode) {
    visit_node_tree_mut(node, &mut |n| n.on_reset());
}

fn detach_node_tree(node: &mut dyn ModifierNode) {
    visit_node_tree_mut(node, &mut |n| {
        if n.node_state().is_attached() {
            n.on_detach();
            n.node_state().set_attached(false);
        }
    });
}

/// Chain of modifier nodes attached to a layout node.
///
/// The chain tracks ownership of modifier nodes and reuses them across
/// updates when the incoming element list still contains a node of the
/// same type. Removed nodes detach automatically so callers do not need
/// to manually manage their lifetimes.
pub struct ModifierNodeChain {
    entries: Vec<ModifierNodeEntry>,
    pub(super) aggregated_capabilities: NodeCapabilities,
    // The chain's nodes in order, delegates after the node that delegates to
    // them, each with its capabilities and the union of its own and those of
    // every node after it: the links between them, which no node keeps.
    pub(super) ordered_nodes: Vec<(NodePath, NodeCapabilities, NodeCapabilities)>,
    /// Whether the chain's nodes are attached. A node that joins an attached
    /// chain attaches as it joins; one that joins a detached chain waits for
    /// [`Self::attach_nodes`].
    attached: bool,
}

/// Buffers the reconciliation of a chain reuses, held per thread rather than
/// per chain: a chain kept its own after its first reconciliation, a few
/// hundred bytes on every node of a tree.
#[derive(Default)]
struct ReconcileScratch {
    old_used: Vec<bool>,
    match_order: Vec<Option<usize>>,
    final_slots: Vec<Option<ModifierNodeEntry>>,
    elements: Vec<DynModifierElement>,
}

thread_local! {
    static RECONCILE_SCRATCH: RefCell<ReconcileScratch> =
        RefCell::default();
}

impl Default for ModifierNodeChain {
    fn default() -> Self {
        Self::new()
    }
}

struct EntryIndex {
    keyed: HashMap<(TypeId, TypeId, u64), Vec<usize>>,
    hashed: HashMap<(TypeId, TypeId, u64), Vec<usize>>,
    typed: HashMap<(TypeId, TypeId), Vec<usize>>,
}

struct EntryMatchQuery<'a> {
    element_type: TypeId,
    node_type: TypeId,
    key: Option<u64>,
    hash_code: u64,
    element: &'a DynModifierElement,
}

impl EntryIndex {
    fn build(entries: &[ModifierNodeEntry]) -> Self {
        let mut keyed = HashMap::default();
        let mut hashed = HashMap::default();
        let mut typed = HashMap::default();

        for (i, entry) in entries.iter().enumerate() {
            let (element_type, node_type) =
                (entry.element.element_type(), entry.element.node_type());
            if let Some(key_value) = entry.key {
                keyed
                    .entry((element_type, node_type, key_value))
                    .or_insert_with(Vec::new)
                    .push(i);
            } else {
                hashed
                    .entry((element_type, node_type, entry.element.hash_code()))
                    .or_insert_with(Vec::new)
                    .push(i);
                typed
                    .entry((element_type, node_type))
                    .or_insert_with(Vec::new)
                    .push(i);
            }
        }

        Self {
            keyed,
            hashed,
            typed,
        }
    }

    fn find_match(
        &self,
        entries: &[ModifierNodeEntry],
        used: &[bool],
        query: EntryMatchQuery<'_>,
    ) -> Option<usize> {
        if let Some(key_value) = query.key {
            if let Some(candidates) =
                self.keyed
                    .get(&(query.element_type, query.node_type, key_value))
            {
                for &i in candidates {
                    if !used[i] {
                        return Some(i);
                    }
                }
            }
        } else {
            if let Some(candidates) =
                self.hashed
                    .get(&(query.element_type, query.node_type, query.hash_code))
            {
                for &i in candidates {
                    if !used[i]
                        && entries[i]
                            .element
                            .as_ref()
                            .equals_element(query.element.as_ref())
                    {
                        return Some(i);
                    }
                }
            }

            if let Some(candidates) = self.typed.get(&(query.element_type, query.node_type)) {
                for &i in candidates {
                    if !used[i] {
                        return Some(i);
                    }
                }
            }
        }

        None
    }
}

/// What updating an entry in place did to the chain's links.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InPlace {
    /// The links and capability aggregates stand as they were.
    LinksHold,
    /// The entry's capabilities changed, or its updated node delegates and
    /// may have changed its delegates: the chain links again.
    Relink,
}

/// Updates `entry` for `element` where it is, when the element is of the
/// entry's type and key and its node can take it: an element of a node's
/// type and key updates the node whatever its content, as Compose does.
/// `None` leaves the entry as it was.
fn update_entry_in_place(
    entry: &mut ModifierNodeEntry,
    element: &DynModifierElement,
    context: &mut dyn ModifierNodeContext,
) -> Option<InPlace> {
    if entry.element.element_type() != element.element_type()
        || entry.element.node_type() != element.node_type()
        || entry.key != element.key()
        || !element.can_update_node(&*entry.node.borrow())
    {
        return None;
    }
    let same_element = entry.element.as_ref().equals_element(element.as_ref());
    let capabilities = element.capabilities();
    let mut outcome = if capabilities == entry.capabilities {
        InPlace::LinksHold
    } else {
        InPlace::Relink
    };
    if !same_element || element.requires_update() {
        element.update_node(&mut *entry.node.borrow_mut());
        entry.element = Rc::clone(element);
        request_update_auto_invalidations(element.as_ref(), context, capabilities);
        entry
            .node
            .borrow()
            .for_each_delegate(&mut |_| outcome = InPlace::Relink);
    }
    entry.capabilities = capabilities;
    entry
        .node
        .borrow()
        .node_state()
        .set_capabilities(capabilities);
    Some(outcome)
}

/// A new entry for `element` and its node, updated, and attached when
/// `attach`, as it joins an attached chain.
fn fresh_entry(
    element: DynModifierElement,
    context: &mut dyn ModifierNodeContext,
    attach: bool,
) -> ModifierNodeEntry {
    let capabilities = element.capabilities();
    let entry = ModifierNodeEntry::new(
        element.key(),
        Rc::clone(&element),
        element.create_node(),
        capabilities,
    );
    if attach {
        attach_node_tree(&mut *entry.node.borrow_mut(), context);
    }
    element.update_node(&mut *entry.node.borrow_mut());
    request_auto_invalidations(context, capabilities);
    entry
}

/// Matches `element`, at `new_pos` of the rebuilt tail, against the old
/// entries: one of its type and key whose node can take it is updated and
/// marked for `new_pos`, and `None` is returned; otherwise the element's
/// new entry is, attached when `attach`.
fn reconcile_element(
    element: DynModifierElement,
    new_pos: usize,
    (old_entries, index): (&mut [ModifierNodeEntry], &EntryIndex),
    (used, match_order): (&mut [bool], &mut [Option<usize>]),
    context: &mut dyn ModifierNodeContext,
    attach: bool,
) -> Option<ModifierNodeEntry> {
    let element_type = element.element_type();
    let node_type = element.node_type();
    let key = element.key();
    // Only an unkeyed element is matched by its hash.
    let hash_code = if key.is_none() {
        element.hash_code()
    } else {
        0
    };
    let capabilities = element.capabilities();
    let Some(idx) = index.find_match(
        old_entries,
        used,
        EntryMatchQuery {
            element_type,
            node_type,
            key,
            hash_code,
            element: &element,
        },
    ) else {
        return Some(fresh_entry(element, context, attach));
    };
    let entry = &mut old_entries[idx];
    if !element.can_update_node(&*entry.node.borrow()) {
        return Some(fresh_entry(element, context, attach));
    }

    used[idx] = true;
    match_order[idx] = Some(new_pos);
    let same_element = entry.element.as_ref().equals_element(element.as_ref());
    if !same_element || element.requires_update() {
        element.update_node(&mut *entry.node.borrow_mut());
        entry.element = element;
        request_update_auto_invalidations(entry.element.as_ref(), context, capabilities);
    }
    if idx != new_pos {
        request_auto_invalidations(context, capabilities);
    }
    entry.key = key;
    entry.capabilities = capabilities;
    entry
        .node
        .borrow()
        .node_state()
        .set_capabilities(capabilities);
    None
}

impl ModifierNodeChain {
    /// An attached chain: its nodes attach as they join it.
    pub fn new() -> Self {
        let mut chain = Self::detached();
        chain.attached = true;
        chain
    }

    /// A detached chain: its nodes attach at [`Self::attach_nodes`], as a
    /// layout node's do once the node has its id.
    pub fn detached() -> Self {
        Self {
            entries: Vec::new(),
            aggregated_capabilities: NodeCapabilities::empty(),
            ordered_nodes: Vec::new(),
            attached: false,
        }
    }

    /// Detaches all nodes in the chain.
    pub fn detach_nodes(&mut self) {
        if !self.attached {
            return;
        }
        self.attached = false;
        for entry in &self.entries {
            detach_node_tree(&mut *entry.node.borrow_mut());
        }
    }

    /// Attaches all nodes in the chain.
    pub fn attach_nodes(&mut self, context: &mut dyn ModifierNodeContext) {
        if self.attached {
            return;
        }
        self.attached = true;
        for entry in &self.entries {
            attach_node_tree(&mut *entry.node.borrow_mut(), context);
        }
    }

    /// Reconcile the chain against the provided elements, attaching newly
    /// created nodes and detaching nodes that are no longer required.
    ///
    /// This method delegates to `update_from_ref_iter` which handles the
    /// actual reconciliation logic.
    pub fn update_from_slice(
        &mut self,
        elements: &[DynModifierElement],
        context: &mut dyn ModifierNodeContext,
    ) {
        self.update_from_ref_iter(elements.iter(), context);
    }

    /// Updates in place the entries `changed` names, one bit an index, for
    /// `elements` that differ from the chain's only in those: entries and
    /// elements line up one to one, and every other entry stays as it is.
    /// `false` when they do not line up or an entry cannot take its element;
    /// a full update then reconciles the chain.
    pub fn update_entries_in_place<'a, I>(
        &mut self,
        elements: I,
        element_count: usize,
        changed: u64,
        context: &mut dyn ModifierNodeContext,
    ) -> bool
    where
        I: Iterator<Item = &'a DynModifierElement>,
    {
        if element_count != self.entries.len() {
            return false;
        }
        let mut relink = false;
        let updated = self
            .entries
            .iter_mut()
            .zip(elements)
            .enumerate()
            .filter(|&(index, _)| {
                u32::try_from(index)
                    .ok()
                    .and_then(|bit| changed.checked_shr(bit))
                    .is_some_and(|rest| rest & 1 == 1)
            })
            .map(|(_, pair)| pair)
            .all(
                |(entry, element)| match update_entry_in_place(entry, element, context) {
                    Some(outcome) => {
                        relink |= outcome == InPlace::Relink;
                        true
                    }
                    None => false,
                },
            );
        if relink {
            self.sync_chain_links();
        }
        updated
    }

    /// Reconciles the chain against the elements that `elements` yields, without collecting them
    /// first. See `update_from_slice`.
    pub fn update_from_ref_iter<'a, I>(
        &mut self,
        elements: I,
        context: &mut dyn ModifierNodeContext,
    ) where
        I: Iterator<Item = &'a DynModifierElement>,
    {
        // Taken out for the reconciliation, so one that reconciles another
        // chain inside it works on buffers of its own.
        let mut scratch = RECONCILE_SCRATCH
            .try_with(|scratch| std::mem::take(&mut *scratch.borrow_mut()))
            .unwrap_or_default();
        self.reconcile(elements, context, &mut scratch);
        scratch.elements.clear();
        scratch.final_slots.clear();
        let _ = RECONCILE_SCRATCH.try_with(|slot| *slot.borrow_mut() = scratch);
    }

    fn reconcile<'a, I>(
        &mut self,
        elements: I,
        context: &mut dyn ModifierNodeContext,
        scratch: &mut ReconcileScratch,
    ) where
        I: Iterator<Item = &'a DynModifierElement>,
    {
        let old_len = self.entries.len();
        let mut fast_path_failed_at: Option<usize> = None;
        let mut elements_count = 0;
        // Entries updated where they are keep the links between them, unless
        // their capabilities or delegates change.
        let mut relink = false;

        scratch.elements.clear();

        for (idx, element) in elements.enumerate() {
            elements_count = idx + 1;
            if fast_path_failed_at.is_none() && idx < old_len {
                if let Some(outcome) =
                    update_entry_in_place(&mut self.entries[idx], element, context)
                {
                    relink |= outcome == InPlace::Relink;
                    continue;
                }
                fast_path_failed_at = Some(idx);
            }
            scratch.elements.push(Rc::clone(element));
        }

        if fast_path_failed_at.is_none() && scratch.elements.is_empty() {
            if elements_count < self.entries.len() {
                for entry in self.entries.drain(elements_count..) {
                    request_auto_invalidations(context, entry.capabilities);
                    detach_node_tree(&mut *entry.node.borrow_mut());
                }
                relink = true;
            }
            if relink {
                self.sync_chain_links();
            }
            return;
        }

        self.rebuild_tail(fast_path_failed_at.unwrap_or(old_len), context, scratch);
    }

    /// Rebuilds the chain from `fail_idx` for the elements in `scratch`:
    /// each takes over an old entry of its type and key, wherever it was,
    /// or gets a node of its own; old entries nothing took over detach.
    fn rebuild_tail(
        &mut self,
        fail_idx: usize,
        context: &mut dyn ModifierNodeContext,
        scratch: &mut ReconcileScratch,
    ) {
        let mut old_entries: Vec<ModifierNodeEntry> = self.entries.drain(fail_idx..).collect();
        let processed_entries_len = self.entries.len();
        let old_len = old_entries.len();

        scratch.old_used.clear();
        scratch.old_used.resize(old_len, false);

        scratch.match_order.clear();
        scratch.match_order.resize(old_len, None);

        let index = EntryIndex::build(&old_entries);

        let new_elements_count = scratch.elements.len();
        scratch.final_slots.clear();
        scratch.final_slots.reserve(new_elements_count);

        for (new_pos, element) in scratch.elements.drain(..).enumerate() {
            let fresh = reconcile_element(
                element,
                new_pos,
                (&mut old_entries, &index),
                (&mut scratch.old_used, &mut scratch.match_order),
                context,
                self.attached,
            );
            scratch.final_slots.push(fresh);
        }

        for (i, entry) in old_entries.into_iter().enumerate() {
            match scratch.match_order[i] {
                Some(pos) if scratch.old_used[i] => scratch.final_slots[pos] = Some(entry),
                _ => {
                    request_auto_invalidations(context, entry.capabilities);
                    detach_node_tree(&mut *entry.node.borrow_mut());
                }
            }
        }

        // Exactly: a chain is rebuilt only when its modifiers change, and
        // most hold one to three entries, where growth would make room for
        // four.
        self.entries.reserve_exact(scratch.final_slots.len());
        for slot in scratch.final_slots.drain(..) {
            if let Some(entry) = slot {
                self.entries.push(entry);
            } else {
                log::error!("modifier reconciliation produced an empty final slot");
            }
        }

        debug_assert_eq!(
            self.entries.len(),
            processed_entries_len + new_elements_count
        );
        self.sync_chain_links();
    }

    /// Convenience wrapper that accepts any iterator of type-erased
    /// modifier elements. Elements are collected into a temporary vector
    /// before reconciliation.
    pub fn update<I>(&mut self, elements: I, context: &mut dyn ModifierNodeContext)
    where
        I: IntoIterator<Item = DynModifierElement>,
    {
        let collected: Vec<DynModifierElement> = elements.into_iter().collect();
        self.update_from_slice(&collected, context);
    }

    /// Resets all nodes in the chain. This mirrors the behaviour of
    /// Jetpack Compose's `onReset` callback.
    pub fn reset(&mut self) {
        for entry in &mut self.entries {
            reset_node_tree(&mut *entry.node.borrow_mut());
        }
    }

    /// Detaches every node in the chain and clears internal storage.
    pub fn detach_all(&mut self) {
        for entry in std::mem::take(&mut self.entries) {
            detach_node_tree(&mut *entry.node.borrow_mut());
            {
                let node_borrow = entry.node.borrow();
                let state = node_borrow.node_state();
                state.set_capabilities(NodeCapabilities::empty());
            }
        }
        self.sync_chain_links();
    }

    /// The number of nodes in the chain.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the chain holds no nodes.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the aggregated capability mask for the entire chain.
    pub fn capabilities(&self) -> NodeCapabilities {
        self.aggregated_capabilities
    }

    /// Returns true if the chain contains at least one node with the requested capability.
    pub fn has_capability(&self, capability: NodeCapabilities) -> bool {
        self.aggregated_capabilities.contains(capability)
    }

    /// Returns the sentinel head reference for traversal.
    pub fn head(&self) -> ModifierChainNodeRef<'_> {
        self.node_ref(ChainPosition::Head)
    }

    /// Returns the sentinel tail reference for traversal.
    pub fn tail(&self) -> ModifierChainNodeRef<'_> {
        self.node_ref(ChainPosition::Tail)
    }

    /// Iterates over the chain from head to tail, skipping sentinels.
    pub fn head_to_tail(&self) -> ModifierChainIter<'_> {
        ModifierChainIter::forward(self)
    }

    /// Iterates over the chain from tail to head, skipping sentinels.
    pub fn tail_to_head(&self) -> ModifierChainIter<'_> {
        ModifierChainIter::backward(self)
    }

    /// Calls `f` for every node in insertion order.
    pub fn for_each_forward<F>(&self, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>),
    {
        for node in self.head_to_tail() {
            f(node);
        }
    }

    /// Calls `f` for every node containing any capability from `mask`.
    pub fn for_each_forward_matching<F>(&self, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>),
    {
        if mask.is_empty() {
            self.for_each_forward(f);
            return;
        }

        if !self.head().aggregate_child_capabilities().intersects(mask) {
            return;
        }

        for node in self.head_to_tail() {
            if node.kind_set().intersects(mask) {
                f(node);
            }
        }
    }

    /// Calls `f` for every node containing any capability from `mask`, providing the node ref.
    pub fn for_each_node_with_capability<F>(&self, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>, &dyn ModifierNode),
    {
        self.for_each_forward_matching(mask, |node_ref| {
            node_ref.with_node(|node| f(node_ref.clone(), node));
        });
    }

    /// Calls `f` for every node in reverse insertion order.
    pub fn for_each_backward<F>(&self, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>),
    {
        for node in self.tail_to_head() {
            f(node);
        }
    }

    /// Returns the node reference that owns `node`.
    pub fn find_node_ref(&self, node: &dyn ModifierNode) -> Option<ModifierChainNodeRef<'_>> {
        fn node_data_ptr(node: &dyn ModifierNode) -> *const () {
            node as *const dyn ModifierNode as *const ()
        }

        let target = node_data_ptr(node);
        // A node comes before its delegates, so a delegate that starts its
        // node's memory finds the node.
        (0..self.ordered_nodes.len())
            .find(|&index| {
                self.with_path(self.ordered_nodes[index].0, |node| {
                    node_data_ptr(node) == target
                })
                .unwrap_or(false)
            })
            .map(|index| self.node_ref(ChainPosition::Node(index)))
    }

    /// Downcasts the node at `index` to the requested type.
    /// Returns a `Ref` guard that dereferences to the node type.
    pub fn node<N: ModifierNode + 'static>(&self, index: usize) -> Option<std::cell::Ref<'_, N>> {
        self.entries.get(index).and_then(|entry| {
            std::cell::Ref::filter_map(entry.node.borrow(), |boxed_node| {
                boxed_node.as_any().downcast_ref::<N>()
            })
            .ok()
        })
    }

    /// Downcasts the node at `index` to the requested mutable type.
    /// Returns a `RefMut` guard that dereferences to the node type.
    pub fn node_mut<N: ModifierNode + 'static>(
        &self,
        index: usize,
    ) -> Option<std::cell::RefMut<'_, N>> {
        self.entries.get(index).and_then(|entry| {
            std::cell::RefMut::filter_map(entry.node.borrow_mut(), |boxed_node| {
                boxed_node.as_any_mut().downcast_mut::<N>()
            })
            .ok()
        })
    }

    /// Returns the shared node at the given index. Coordinators compare it
    /// with the node they hold and clone it only to hold a new one.
    pub fn get_node_rc(&self, index: usize) -> Option<&Rc<RefCell<dyn ModifierNode>>> {
        self.entries.get(index).map(|entry| &entry.node)
    }

    /// Returns true if the chain contains any nodes matching the given invalidation kind.
    pub fn has_nodes_for_invalidation(&self, kind: InvalidationKind) -> bool {
        self.aggregated_capabilities
            .contains(NodeCapabilities::for_invalidation(kind))
    }

    /// Visits every node mutably in insertion order together with its capability mask.
    pub fn visit_nodes_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut dyn ModifierNode, NodeCapabilities),
    {
        for index in 0..self.ordered_nodes.len() {
            let (path, cached_caps, _agg) = self.ordered_nodes[index];
            let mut node_borrow = self.entries[path.entry()].node.borrow_mut();
            let mut current: &mut dyn ModifierNode = &mut *node_borrow;
            for &delegate_index in path.delegates() {
                match nth_delegate_mut(current, delegate_index as usize) {
                    Some(delegate) => current = delegate,
                    None => return,
                }
            }
            f(current, cached_caps);
        }
    }

    pub(super) fn node_ref(&self, position: ChainPosition) -> ModifierChainNodeRef<'_> {
        ModifierChainNodeRef {
            chain: self,
            position,
        }
    }

    // Runs `f` on the node at `path`; `None` when a delegate of it is gone.
    pub(super) fn with_path<R>(
        &self,
        path: NodePath,
        f: impl FnOnce(&dyn ModifierNode) -> R,
    ) -> Option<R> {
        let node_borrow = self.entries[path.entry()].node.borrow();
        let mut current: &dyn ModifierNode = &*node_borrow;
        for &delegate_index in path.delegates() {
            current = nth_delegate(current, delegate_index as usize)?;
        }
        Some(f(current))
    }

    // Orders the nodes again and works out the union of capabilities from
    // each node to the tail.
    fn sync_chain_links(&mut self) {
        self.rebuild_ordered_nodes();
        let mut aggregate = NodeCapabilities::empty();
        for (_, caps, aggregate_child) in self.ordered_nodes.iter_mut().rev() {
            aggregate |= *caps;
            *aggregate_child = aggregate;
        }
        self.aggregated_capabilities = aggregate;
    }

    fn rebuild_ordered_nodes(&mut self) {
        self.ordered_nodes.clear();
        // One link per entry unless an entry delegates; see `rebuild_tail`.
        self.ordered_nodes.reserve_exact(self.entries.len());
        let mut path_buf = [0usize; MAX_DELEGATE_DEPTH];
        for (index, entry) in self.entries.iter().enumerate() {
            let node_borrow = entry.node.borrow();
            Self::enumerate_link_order(
                &*node_borrow,
                index,
                &mut path_buf,
                0,
                &mut self.ordered_nodes,
            );
        }
    }

    fn enumerate_link_order(
        node: &dyn ModifierNode,
        entry: usize,
        path_buf: &mut [usize; MAX_DELEGATE_DEPTH],
        path_len: usize,
        out: &mut Vec<(NodePath, NodeCapabilities, NodeCapabilities)>,
    ) {
        let caps = node.node_state().capabilities();
        out.push((
            NodePath::from_slice(entry, &path_buf[..path_len]),
            caps,
            NodeCapabilities::empty(),
        ));
        let mut delegate_index = 0usize;
        node.for_each_delegate(&mut |child| {
            if path_len < MAX_DELEGATE_DEPTH {
                path_buf[path_len] = delegate_index;
                Self::enumerate_link_order(child, entry, path_buf, path_len + 1, out);
            }
            delegate_index += 1;
        });
    }
}
