use crate::collections::map::HashMap;
use crate::{
    composer_context, empty_local_stack, hash_key, remove_child_and_cleanup_now, runtime, Applier,
    ApplierHost, ChildList, Command, CommandQueue, CompositionLocal, DirtyBubble, Key, LocalKey,
    LocalStackSnapshot, LocalStateEntry, MutableState, Node, NodeError, NodeId, Owned,
    ProvidedValue, RecomposeOptions, RecomposeScope, RecycledNode, RuntimeHandle, SlotId,
    SlotTable, SlotsHost, SnapshotStateList, SnapshotStateMap, SnapshotStateObserver, StartGroup,
    StaticCompositionLocal, StaticLocalEntry, SubcomposeState, COMMAND_FLUSH_THRESHOLD,
};
use smallvec::SmallVec;
use std::any::Any;
use std::cell::{Cell, RefCell, RefMut};
use std::hash::Hash;
use std::marker::PhantomData;
use std::rc::Rc;

pub(crate) struct ParentFrame {
    pub(crate) id: NodeId,
    pub(crate) previous: ChildList,
    pub(crate) new_children: ChildList,
    pub(crate) attach_mode: ParentAttachMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParentAttachMode {
    ImmediateAppend,
    DeferredSync,
}

#[derive(Default)]
pub(crate) struct SubcomposeFrame {
    pub(crate) nodes: Vec<NodeId>,
    pub(crate) scopes: Vec<RecomposeScope>,
}

#[derive(Default, Clone)]
pub(crate) struct LocalContext {
    pub(crate) values: HashMap<LocalKey, Rc<dyn Any>>,
}

pub(crate) struct ComposerCore {
    pub(crate) slots: Rc<SlotsHost>,
    pub(crate) slots_override: RefCell<Vec<Rc<SlotsHost>>>,
    pub(crate) applier: Rc<dyn ApplierHost>,
    pub(crate) runtime: RuntimeHandle,
    pub(crate) observer: SnapshotStateObserver,
    pub(crate) parent_stack: RefCell<Vec<ParentFrame>>,
    pub(crate) subcompose_stack: RefCell<Vec<SubcomposeFrame>>,
    pub(crate) root: Cell<Option<NodeId>>,
    pub(crate) commands: RefCell<CommandQueue>,
    pub(crate) scope_stack: RefCell<Vec<RecomposeScope>>,
    pub(crate) local_stack: RefCell<LocalStackSnapshot>,
    pub(crate) side_effects: RefCell<Vec<Box<dyn FnOnce()>>>,
    pub(crate) pending_scope_options: RefCell<Option<RecomposeOptions>>,
    pub(crate) phase: Cell<crate::Phase>,
    pub(crate) last_node_reused: Cell<Option<bool>>,
    pub(crate) recranpose_parent_hint: Cell<Option<NodeId>>,
    pub(crate) root_render_requested: Cell<bool>,
    pub(crate) _not_send: PhantomData<*const ()>,
}

impl ComposerCore {
    pub(crate) fn new(
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        observer: SnapshotStateObserver,
        root: Option<NodeId>,
    ) -> Self {
        let parent_stack = if let Some(root_id) = root {
            vec![ParentFrame {
                id: root_id,
                previous: ChildList::new(),
                new_children: ChildList::new(),
                attach_mode: ParentAttachMode::DeferredSync,
            }]
        } else {
            Vec::new()
        };

        Self {
            slots,
            slots_override: RefCell::new(Vec::new()),
            applier,
            runtime,
            observer,
            parent_stack: RefCell::new(parent_stack),
            subcompose_stack: RefCell::new(Vec::new()),
            root: Cell::new(root),
            commands: RefCell::new(CommandQueue::default()),
            scope_stack: RefCell::new(Vec::new()),
            local_stack: RefCell::new(empty_local_stack()),
            side_effects: RefCell::new(Vec::new()),
            pending_scope_options: RefCell::new(None),
            phase: Cell::new(crate::Phase::Compose),
            last_node_reused: Cell::new(None),
            recranpose_parent_hint: Cell::new(None),
            root_render_requested: Cell::new(false),
            _not_send: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct Composer {
    pub(crate) core: Rc<ComposerCore>,
}

pub(crate) enum EmittedNode {
    Fresh(Box<dyn Node>),
    Recycled(RecycledNode),
}

impl Composer {
    pub fn new(
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        observer: SnapshotStateObserver,
        root: Option<NodeId>,
    ) -> Self {
        let core = Rc::new(ComposerCore::new(slots, applier, runtime, observer, root));
        Self { core }
    }

    pub(crate) fn from_core(core: Rc<ComposerCore>) -> Self {
        Self { core }
    }

    pub(crate) fn clone_core(&self) -> Rc<ComposerCore> {
        Rc::clone(&self.core)
    }

    fn observer(&self) -> SnapshotStateObserver {
        self.core.observer.clone()
    }

    pub(crate) fn request_root_render(&self) {
        self.core.root_render_requested.set(true);
    }

    pub(crate) fn take_root_render_request(&self) -> bool {
        self.core.root_render_requested.replace(false)
    }

    pub(crate) fn observe_scope<R>(&self, scope: &RecomposeScope, block: impl FnOnce() -> R) -> R {
        let observer = self.observer();
        let scope_clone = scope.clone();
        observer.observe_reads(scope_clone, move |scope_ref| scope_ref.invalidate(), block)
    }

    fn active_slots_host(&self) -> Rc<SlotsHost> {
        self.core
            .slots_override
            .borrow()
            .last()
            .cloned()
            .unwrap_or_else(|| Rc::clone(&self.core.slots))
    }

    pub(crate) fn with_slots<R>(&self, f: impl FnOnce(&SlotTable) -> R) -> R {
        let override_host = {
            let overrides = self.core.slots_override.borrow();
            overrides.last().cloned()
        };
        if let Some(host) = override_host {
            let slots = host.borrow();
            f(&slots)
        } else {
            let slots = self.core.slots.borrow();
            f(&slots)
        }
    }

    pub(crate) fn with_slots_mut<R>(&self, f: impl FnOnce(&mut SlotTable) -> R) -> R {
        let override_host = {
            let overrides = self.core.slots_override.borrow();
            overrides.last().cloned()
        };
        if let Some(host) = override_host {
            let mut slots = host.borrow_mut();
            f(&mut slots)
        } else {
            let mut slots = self.core.slots.borrow_mut();
            f(&mut slots)
        }
    }

    pub(crate) fn with_slot_override<R>(
        &self,
        slots: Rc<SlotsHost>,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        self.core.slots_override.borrow_mut().push(slots);
        struct Guard {
            core: Rc<ComposerCore>,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                self.core.slots_override.borrow_mut().pop();
            }
        }
        let guard = Guard {
            core: self.clone_core(),
        };
        let result = f(self);
        drop(guard);
        result
    }

    pub(crate) fn parent_stack(&self) -> RefMut<'_, Vec<ParentFrame>> {
        self.core.parent_stack.borrow_mut()
    }

    pub(crate) fn subcompose_stack(&self) -> RefMut<'_, Vec<SubcomposeFrame>> {
        self.core.subcompose_stack.borrow_mut()
    }

    pub(crate) fn commands_mut(&self) -> RefMut<'_, CommandQueue> {
        self.core.commands.borrow_mut()
    }

    pub(crate) fn enqueue_semantics_invalidation(&self, id: NodeId) {
        self.commands_mut().push(Command::BubbleDirty {
            node_id: id,
            bubble: DirtyBubble::SEMANTICS,
        });
    }

    pub(crate) fn scope_stack(&self) -> RefMut<'_, Vec<RecomposeScope>> {
        self.core.scope_stack.borrow_mut()
    }

    pub(crate) fn local_stack(&self) -> RefMut<'_, LocalStackSnapshot> {
        self.core.local_stack.borrow_mut()
    }

    pub(crate) fn current_local_stack(&self) -> LocalStackSnapshot {
        self.core.local_stack.borrow().clone()
    }

    pub(crate) fn side_effects_mut(&self) -> RefMut<'_, Vec<Box<dyn FnOnce()>>> {
        self.core.side_effects.borrow_mut()
    }

    fn pending_scope_options(&self) -> RefMut<'_, Option<RecomposeOptions>> {
        self.core.pending_scope_options.borrow_mut()
    }

    pub(crate) fn borrow_applier(&self) -> RefMut<'_, dyn Applier> {
        self.core.applier.borrow_dyn()
    }

    /// Registers a virtual node in the Applier.
    ///
    /// This is used by SubcomposeLayoutNode to register virtual container nodes
    /// so that subsequent insert_child commands can find them and attach children.
    /// Without this, virtual nodes would only exist in SubcomposeLayoutNodeInner.virtual_nodes
    /// and applier.get_mut(virtual_node_id) would fail, breaking child attachment.
    pub fn register_virtual_node(
        &self,
        node_id: NodeId,
        node: Box<dyn Node>,
    ) -> Result<(), NodeError> {
        let mut applier = self.borrow_applier();
        applier.insert_with_id(node_id, node)
    }

    /// Checks if a node has no parent (is a root node).
    /// Used by SubcomposeMeasureScope to filter subcompose results.
    pub fn node_has_no_parent(&self, node_id: NodeId) -> bool {
        let mut applier = self.borrow_applier();
        match applier.get_mut(node_id) {
            Ok(node) => node.parent().is_none(),
            Err(_) => true,
        }
    }

    /// Gets the children of a node from the Applier.
    ///
    /// This is used by SubcomposeLayoutNode to get children of virtual nodes
    /// directly from the Applier, where insert_child commands have been applied.
    pub fn get_node_children(&self, node_id: NodeId) -> SmallVec<[NodeId; 8]> {
        let mut applier = self.borrow_applier();
        match applier.get_mut(node_id) {
            Ok(node) => {
                let mut children = SmallVec::<[NodeId; 8]>::new();
                node.collect_children_into(&mut children);
                children
            }
            Err(_) => SmallVec::<[NodeId; 8]>::new(),
        }
    }

    /// Records a child node in the current parent frame's expected children list.
    ///
    /// Used by SubcomposeLayout's `perform_subcompose` to register virtual nodes
    /// with the outer composer's parent frame. This ensures that the `pop_parent`
    /// call at the end of `subcompose_slot` generates a correct `SyncChildren`
    /// command that preserves (rather than removes) the virtual nodes.
    ///
    /// Without this, `pop_parent` would generate `SyncChildren { expected: [] }`,
    /// which removes all virtual nodes and their subtrees from the applier.
    pub fn record_subcompose_child(&self, child_id: NodeId) {
        let mut parent_stack = self.parent_stack();
        if let Some(frame) = parent_stack.last_mut() {
            if matches!(frame.attach_mode, ParentAttachMode::DeferredSync)
                && !frame.new_children.contains(&child_id)
            {
                frame.new_children.push(child_id);
            }
        }
    }

    /// Clears all children of a node in the Applier.
    ///
    /// This is used by SubcomposeLayoutNode when reusing a virtual node for
    /// different content. Without clearing, old children remain attached,
    /// causing duplicate/interleaved items in lazy lists after scrolling.
    pub fn clear_node_children(&self, node_id: NodeId) {
        let mut applier = self.borrow_applier();
        if let Ok(node) = applier.get_mut(node_id) {
            node.update_children(&[]);
        }
    }

    pub fn install<R>(&self, f: impl FnOnce(&Composer) -> R) -> R {
        let _composer_guard = composer_context::enter(self);
        runtime::push_active_runtime(&self.core.runtime);
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                runtime::pop_active_runtime();
            }
        }
        let guard = Guard;
        let result = f(self);
        drop(guard);
        result
    }

    pub(crate) fn flush_pending_commands_if_large(&self) {
        let queued = self.core.commands.borrow().len();
        if queued < COMMAND_FLUSH_THRESHOLD {
            return;
        }
        self.apply_pending_commands()
            .expect("mid-composition command flush failed");
    }

    pub(crate) fn drain_orphaned_nodes_from_slots(&self) {
        let orphaned = self.with_slots_mut(|slots| slots.drain_orphaned_node_ids());
        if orphaned.is_empty() {
            return;
        }
        let mut deferred = Vec::new();
        let mut applier = self.borrow_applier();
        for orphaned in orphaned {
            match self.with_slots(|slots| slots.orphaned_node_state(orphaned)) {
                crate::slot_table::NodeSlotState::Active => continue,
                crate::slot_table::NodeSlotState::PreservedGap => {
                    deferred.push(orphaned);
                    continue;
                }
                crate::slot_table::NodeSlotState::Missing => {}
            }
            if applier.node_generation(orphaned.id) != orphaned.generation {
                continue;
            }
            let parent_id = applier
                .get_mut(orphaned.id)
                .ok()
                .and_then(|node| node.parent());
            if let Some(parent_id) = parent_id {
                let _ = remove_child_and_cleanup_now(&mut *applier, parent_id, orphaned.id);
                continue;
            }
            if let Ok(node) = applier.get_mut(orphaned.id) {
                node.on_removed_from_parent();
                node.unmount();
            }
            let _ = applier.remove(orphaned.id);
        }
        if !deferred.is_empty() {
            self.with_slots_mut(|slots| {
                for orphaned in deferred {
                    slots.requeue_orphaned_node(orphaned);
                }
            });
        }
    }

    pub fn with_group<R>(&self, key: Key, f: impl FnOnce(&Composer) -> R) -> R {
        let parent_scope = self.current_recranpose_scope();
        let (group, group_anchor, scope_ref, restored_from_gap) = self.with_slots_mut(|slots| {
            let StartGroup {
                group,
                anchor,
                restored_from_gap,
            } = slots.begin_group(key);
            let scope_slot = slots.use_value_slot(|| RecomposeScope::new(self.runtime_handle()));
            let scope_ref = slots.read_value::<RecomposeScope>(scope_slot).clone();
            (group, anchor, scope_ref, restored_from_gap)
        });

        scope_ref.reactivate();
        scope_ref.set_group_anchor(group_anchor);
        scope_ref.set_parent_scope(parent_scope);

        if let Some(options) = self.pending_scope_options().take() {
            if options.force_recompose {
                scope_ref.force_recompose();
            } else if options.force_reuse {
                scope_ref.force_reuse();
            }
        }
        if restored_from_gap {
            scope_ref.force_recompose();
        }

        self.with_slots_mut(|slots| {
            slots.set_group_scope(group.0, scope_ref.id());
        });

        let slots_host = self.active_slots_host();
        scope_ref.set_slots_host(Rc::downgrade(&slots_host));

        {
            let mut stack = self.scope_stack();
            stack.push(scope_ref.clone());
        }

        {
            let mut stack = self.subcompose_stack();
            if let Some(frame) = stack.last_mut() {
                frame.scopes.push(scope_ref.clone());
            }
        }

        scope_ref.snapshot_locals(self.current_local_stack());
        {
            let parent_hint = self.parent_stack().last().map(|frame| frame.id);
            scope_ref.set_parent_hint(parent_hint);
        }

        let result = self.observe_scope(&scope_ref, || f(self));

        scope_ref.mark_composed_once();

        let trimmed = self.with_slots_mut(|slots| slots.trim_to_cursor());
        if trimmed {
            scope_ref.force_recompose();
        }
        self.drain_orphaned_nodes_from_slots();

        {
            let mut stack = self.scope_stack();
            stack.pop();
        }
        scope_ref.mark_recomposed();
        self.with_slots_mut(|slots| slots.end_group());
        self.flush_pending_commands_if_large();
        result
    }

    pub fn cranpose_with_reuse<R>(
        &self,
        key: Key,
        options: RecomposeOptions,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        self.pending_scope_options().replace(options);
        self.with_group(key, f)
    }

    pub fn with_key<K: Hash, R>(&self, key: &K, f: impl FnOnce(&Composer) -> R) -> R {
        let hashed = hash_key(key);
        self.with_group(hashed, f)
    }

    pub fn remember<T: 'static>(&self, init: impl FnOnce() -> T) -> Owned<T> {
        self.with_slots_mut(|slots| slots.remember(init))
    }

    pub fn use_value_slot<T: 'static>(&self, init: impl FnOnce() -> T) -> usize {
        self.with_slots_mut(|slots| slots.use_value_slot(init))
    }

    pub fn with_slot_value<T: 'static, R>(&self, idx: usize, f: impl FnOnce(&T) -> R) -> R {
        self.with_slots(|slots| f(slots.read_value(idx)))
    }

    pub fn with_slot_value_mut<T: 'static, R>(&self, idx: usize, f: impl FnOnce(&mut T) -> R) -> R {
        self.with_slots_mut(|slots| f(slots.read_value_mut(idx)))
    }

    pub fn write_slot_value<T: 'static>(&self, idx: usize, value: T) {
        self.with_slots_mut(|slots| slots.write_value(idx, value));
    }

    pub fn mutable_state_of<T: Clone + 'static>(&self, initial: T) -> MutableState<T> {
        MutableState::with_runtime(initial, self.runtime_handle())
    }

    pub fn mutable_state_list_of<T, I>(&self, values: I) -> SnapshotStateList<T>
    where
        T: Clone + 'static,
        I: IntoIterator<Item = T>,
    {
        SnapshotStateList::with_runtime(values, self.runtime_handle())
    }

    pub fn mutable_state_map_of<K, V, I>(&self, pairs: I) -> SnapshotStateMap<K, V>
    where
        K: Clone + Eq + Hash + 'static,
        V: Clone + 'static,
        I: IntoIterator<Item = (K, V)>,
    {
        SnapshotStateMap::with_runtime(pairs, self.runtime_handle())
    }

    pub fn read_composition_local<T: Clone + 'static>(&self, local: &CompositionLocal<T>) -> T {
        let stack = self.core.local_stack.borrow();
        for context in stack.iter().rev() {
            if let Some(entry) = context.values.get(&local.key) {
                let typed = entry
                    .clone()
                    .downcast::<LocalStateEntry<T>>()
                    .expect("composition local type mismatch");
                return typed.value();
            }
        }
        local.default_value()
    }

    pub fn read_static_composition_local<T: Clone + 'static>(
        &self,
        local: &StaticCompositionLocal<T>,
    ) -> T {
        let stack = self.core.local_stack.borrow();
        for context in stack.iter().rev() {
            if let Some(entry) = context.values.get(&local.key) {
                let typed = entry
                    .clone()
                    .downcast::<StaticLocalEntry<T>>()
                    .expect("static composition local type mismatch");
                return typed.value();
            }
        }
        local.default_value()
    }

    pub fn current_recranpose_scope(&self) -> Option<RecomposeScope> {
        self.core.scope_stack.borrow().last().cloned()
    }

    pub fn phase(&self) -> crate::Phase {
        self.core.phase.get()
    }

    pub(crate) fn set_phase(&self, phase: crate::Phase) {
        self.core.phase.set(phase);
    }

    pub fn enter_phase(&self, phase: crate::Phase) {
        self.set_phase(phase);
    }

    pub(crate) fn subcompose<R>(
        &self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&Composer) -> R,
    ) -> (R, Vec<NodeId>) {
        match self.phase() {
            crate::Phase::Measure | crate::Phase::Layout => {}
            current => panic!(
                "subcompose() may only be called during measure or layout; current phase: {:?}",
                current
            ),
        }

        self.subcompose_stack().push(SubcomposeFrame::default());
        struct StackGuard {
            core: Rc<ComposerCore>,
            leaked: bool,
        }
        impl Drop for StackGuard {
            fn drop(&mut self) {
                if !self.leaked {
                    self.core.subcompose_stack.borrow_mut().pop();
                }
            }
        }
        let mut guard = StackGuard {
            core: self.clone_core(),
            leaked: false,
        };

        let slot_host = state.get_or_create_slots(slot_id);
        {
            let mut slots = slot_host.borrow_mut();
            slots.reset();
        }
        let result = self.with_slot_override(slot_host.clone(), |composer| {
            composer.with_group(slot_id.raw(), |composer| content(composer))
        });
        {
            let mut slots = slot_host.borrow_mut();
            slots.finalize_current_group();
            slots.flush();
        }

        let frame = {
            let mut stack = guard.core.subcompose_stack.borrow_mut();
            let frame = stack.pop().expect("subcompose stack underflow");
            guard.leaked = true;
            frame
        };
        let nodes = frame.nodes;
        let scopes = frame.scopes;
        state.register_active(slot_id, &nodes, &scopes);
        (result, nodes)
    }

    pub fn subcompose_measurement<R>(
        &self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&Composer) -> R,
    ) -> (R, Vec<NodeId>) {
        let (result, nodes) = self.subcompose(state, slot_id, content);
        let roots = nodes
            .into_iter()
            .filter(|&id| self.node_has_no_parent(id))
            .collect();

        (result, roots)
    }

    pub fn subcompose_in<R>(
        &self,
        slots: &Rc<SlotsHost>,
        root: Option<NodeId>,
        f: impl FnOnce(&Composer) -> R,
    ) -> Result<R, NodeError> {
        let runtime_handle = self.runtime_handle();
        slots.borrow_mut().reset();
        let phase = self.phase();
        let locals = self.current_local_stack();
        let core = Rc::new(ComposerCore::new(
            Rc::clone(slots),
            Rc::clone(&self.core.applier),
            runtime_handle.clone(),
            self.observer(),
            root,
        ));
        core.phase.set(phase);
        *core.local_stack.borrow_mut() = locals;
        let composer = Composer::from_core(core);
        let (result, commands, side_effects) = composer.install(|composer| {
            let output = f(composer);
            let commands = composer.take_commands();
            let side_effects = composer.take_side_effects();
            (output, commands, side_effects)
        });
        {
            let mut applier = self.borrow_applier();
            commands.apply(&mut *applier)?;
            for update in runtime_handle.take_updates() {
                update.apply(&mut *applier)?;
            }
        }
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        {
            let mut slots_mut = slots.borrow_mut();
            slots_mut.finalize_current_group();
            slots_mut.flush();
        }
        Ok(result)
    }

    /// Subcomposes content using an isolated SlotsHost without resetting it.
    /// Unlike `subcompose_in`, this preserves existing slot state across calls,
    /// allowing efficient reuse during measurement passes. This is critical for
    /// lazy lists where items need stable slot positions.
    pub fn subcompose_slot<R>(
        &self,
        slots: &Rc<SlotsHost>,
        root: Option<NodeId>,
        f: impl FnOnce(&Composer) -> R,
    ) -> Result<(R, Vec<RecomposeScope>), NodeError> {
        let runtime_handle = self.runtime_handle();
        slots.borrow_mut().reset();
        let phase = self.phase();
        let locals = self.current_local_stack();
        let core = Rc::new(ComposerCore::new(
            Rc::clone(slots),
            Rc::clone(&self.core.applier),
            runtime_handle.clone(),
            self.observer(),
            root,
        ));
        core.phase.set(phase);
        *core.local_stack.borrow_mut() = locals;
        let composer = Composer::from_core(core);
        composer.subcompose_stack().push(SubcomposeFrame::default());
        struct StackGuard {
            core: Rc<ComposerCore>,
            leaked: bool,
        }
        impl Drop for StackGuard {
            fn drop(&mut self) {
                if !self.leaked {
                    self.core.subcompose_stack.borrow_mut().pop();
                }
            }
        }
        let mut guard = StackGuard {
            core: composer.clone_core(),
            leaked: false,
        };
        let root_group_key = crate::location_key(file!(), line!(), column!());
        let (result, commands, side_effects) = composer.install(|composer| {
            let output = composer.with_group(root_group_key, |composer| f(composer));
            if root.is_some() {
                composer.pop_parent();
            }
            let commands = composer.take_commands();
            let side_effects = composer.take_side_effects();
            (output, commands, side_effects)
        });
        let frame = {
            let mut stack = guard.core.subcompose_stack.borrow_mut();
            let frame = stack.pop().expect("subcompose stack underflow");
            guard.leaked = true;
            frame
        };

        {
            let mut applier = self.borrow_applier();
            commands.apply(&mut *applier)?;
            for update in runtime_handle.take_updates() {
                update.apply(&mut *applier)?;
            }
        }
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        {
            let mut slots_mut = slots.borrow_mut();
            let _ = slots_mut.finalize_current_group();
            slots_mut.flush();
        }
        Ok((result, frame.scopes))
    }

    pub(crate) fn skipped_group_root_nodes(&self, nodes: &[NodeId]) -> Vec<NodeId> {
        let node_set: std::collections::HashSet<NodeId> = nodes.iter().copied().collect();
        let mut applier = self.borrow_applier();
        nodes
            .iter()
            .copied()
            .filter(|id| {
                let parent = applier.get_mut(*id).ok().and_then(|node| node.parent());
                parent.is_none_or(|parent_id| !node_set.contains(&parent_id))
            })
            .collect()
    }

    pub fn skip_current_group(&self) {
        let nodes = self.with_slots(|slots| slots.nodes_in_current_group());
        let root_nodes = self.skipped_group_root_nodes(&nodes);
        self.with_slots_mut(|slots| slots.skip_current_group());
        for id in root_nodes {
            self.attach_to_parent_with_mode(id, true);
        }
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.core.runtime.clone()
    }

    pub fn set_recranpose_callback<F>(&self, callback: F)
    where
        F: FnMut(&Composer) + 'static,
    {
        if let Some(scope) = self.current_recranpose_scope() {
            let observer = self.observer();
            let scope_weak = scope.downgrade();
            let mut callback = callback;
            scope.set_recompose(Box::new(move |composer: &Composer| {
                if let Some(inner) = scope_weak.upgrade() {
                    let scope_instance = RecomposeScope { inner };
                    observer.observe_reads(
                        scope_instance.clone(),
                        move |scope_ref| scope_ref.invalidate(),
                        || {
                            callback(composer);
                        },
                    );
                }
            }));
        }
    }

    pub fn set_recranpose_fn(&self, callback: fn(&Composer)) {
        if let Some(scope) = self.current_recranpose_scope() {
            scope.set_recompose_fn(callback);
        }
    }

    pub fn with_composition_locals<R>(
        &self,
        provided: Vec<ProvidedValue>,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        if provided.is_empty() {
            return f(self);
        }
        let mut context = LocalContext::default();
        for value in provided {
            let (key, entry) = value.into_entry(self);
            context.values.insert(key, entry);
        }
        {
            let mut stack = self.local_stack();
            Rc::make_mut(&mut *stack).push(context);
        }
        let result = f(self);
        {
            let mut stack = self.local_stack();
            Rc::make_mut(&mut *stack).pop();
        }
        result
    }
}
