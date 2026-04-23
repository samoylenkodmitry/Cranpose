use crate::{
    debug_scope_label, Applier, ChildList, Command, CommandQueue, Composer, DirtyBubble,
    EmittedNode, MutableState, Node, NodeError, NodeId, OwnedMutableState, ParentAttachMode,
    ParentFrame,
};
use std::any::TypeId;

impl Composer {
    pub fn use_state<T: Clone + 'static>(&self, init: impl FnOnce() -> T) -> MutableState<T> {
        let runtime = self.runtime_handle();
        let state = self.with_slot_session_mut(|slots| {
            slots.remember(|| OwnedMutableState::with_runtime(init(), runtime.clone()))
        });
        state.with(|state| state.handle())
    }

    fn emit_node_box<N: Node + 'static>(
        &self,
        make_node: impl FnOnce(&mut dyn Applier) -> EmittedNode,
    ) -> NodeId {
        // Peek at the slot without advancing cursor
        let (existing_id, existing_generation, type_matches, gen_matches) = {
            if let Some((id, slot_gen)) =
                self.with_slot_session_mut(|slots| slots.current_node_record())
            {
                let mut applier = self.borrow_applier();
                let gen_ok = applier.node_generation(id) == slot_gen;
                let type_ok = match applier.get_mut(id) {
                    Ok(node) => node.as_any_mut().downcast_ref::<N>().is_some(),
                    Err(_) => false,
                };
                (Some(id), Some(slot_gen), type_ok, gen_ok)
            } else {
                (None, None, false, false)
            }
        };

        // If we have a matching node with correct generation, advance cursor and reuse it
        if let Some(id) = existing_id {
            if type_matches && gen_matches {
                let scope_debug = self
                    .current_recranpose_scope()
                    .map(|scope| (scope.id(), debug_scope_label(scope.id())))
                    .unwrap_or((0, None));
                log::trace!(
                    target: "cranpose::compose::emit",
                    "reusing node #{id} as {} [scope_id={} scope_label={:?}]",
                    std::any::type_name::<N>(),
                    scope_debug.0,
                    scope_debug.1,
                );
                let slot_gen =
                    existing_generation.expect("reused node must keep its slot generation");
                let recorded = self.with_slot_session_mut(|slots| slots.record_node(id, slot_gen));
                debug_assert!(
                    recorded.reused && recorded.id == id,
                    "reused node recording must keep the same node identity"
                );
                self.core.last_node_reused.set(Some(recorded.reused));

                self.commands_mut().push(Command::update_node::<N>(id));
                self.attach_to_parent(id);
                return id;
            }
        }

        // If there was a mismatched node in this slot (wrong type or stale generation),
        // schedule its removal before creating a new one.
        if let Some(old_id) = existing_id {
            if !gen_matches {
                // Stale generation: the slot points to a recycled index.
                // Don't remove the node — it belongs to a different composition context.
                log::trace!(
                    target: "cranpose::compose::emit",
                    "stale generation for node #{old_id} (current={})",
                    self.borrow_applier().node_generation(old_id)
                );
            } else if !type_matches {
                // Same generation but wrong type: genuinely needs replacement.
                log::trace!(
                    target: "cranpose::compose::emit",
                    "replacing node #{old_id} with new {}",
                    std::any::type_name::<N>()
                );
                self.commands_mut().push(Command::RemoveNode { id: old_id });
            }
        }

        // Type mismatch, stale generation, or no node: create new node
        let (id, gen) = {
            let mut applier = self.borrow_applier();
            let emitted = make_node(&mut *applier);
            let id = match emitted {
                EmittedNode::Fresh(node) => applier.create(node),
                EmittedNode::Recycled(recycled) => {
                    let (stable_id, node, warm_origin) = recycled.into_parts();
                    applier
                        .insert_with_id(stable_id, node)
                        .expect("recycled stable id should be available");
                    applier.set_recycled_node_origin(stable_id, warm_origin);
                    stable_id
                }
            };
            let gen = applier.node_generation(id);
            (id, gen)
        };
        let scope_debug = self
            .current_recranpose_scope()
            .map(|scope| (scope.id(), debug_scope_label(scope.id())))
            .unwrap_or((0, None));
        log::trace!(
            target: "cranpose::compose::emit",
            "creating node #{} (gen={}) as {} [scope_id={} scope_label={:?}]",
            id,
            gen,
            std::any::type_name::<N>(),
            scope_debug.0,
            scope_debug.1,
        );
        let recorded = self.with_slot_session_mut(|slots| slots.record_node(id, gen));
        debug_assert!(
            !recorded.reused && recorded.id == id,
            "fresh or replacement node recording must report a non-reused node"
        );
        self.core.last_node_reused.set(Some(recorded.reused));
        self.commands_mut().push(Command::MountNode { id });
        self.attach_to_parent(id);
        id
    }

    pub fn emit_node<N: Node + 'static>(&self, init: impl FnOnce() -> N) -> NodeId {
        self.emit_node_box::<N>(|_| EmittedNode::Fresh(Box::new(init())))
    }

    pub fn emit_recyclable_node<N: Node + 'static>(
        &self,
        init: impl FnOnce() -> N,
        reset: impl FnOnce(&mut N),
    ) -> NodeId {
        self.emit_node_box::<N>(|applier| {
            let key = TypeId::of::<N>();
            if let Some(mut recycled) = applier.take_recycled_node(key) {
                let typed = recycled
                    .node_mut()
                    .as_any_mut()
                    .downcast_mut::<N>()
                    .expect("recycled node type mismatch");
                reset(typed);
                EmittedNode::Recycled(recycled)
            } else {
                let node = Box::new(init());
                applier.record_fresh_recyclable_creation(key);
                if let Some(shell) = node.rehouse_for_recycle() {
                    applier.seed_recycled_node_shell(key, node.recycle_pool_limit(), shell);
                }
                EmittedNode::Fresh(node)
            }
        })
    }

    fn attach_to_parent(&self, id: NodeId) {
        self.attach_to_parent_with_mode(id, false);
    }

    pub(crate) fn attach_to_parent_with_mode(
        &self,
        id: NodeId,
        force_reparent_current_parent: bool,
    ) {
        // IMPORTANT: Check parent_stack FIRST.
        // During subcomposition, if there's an active parent (e.g., Row),
        // child nodes (e.g., Text) should attach to that parent, NOT to the
        // subcompose frame. Only ROOT nodes (nodes with no active parent)
        // should be added to the subcompose frame.
        let mut parent_stack = self.parent_stack();
        if let Some(parent_id) = parent_stack.last().map(|frame| frame.id) {
            let stale_root_parent = self.core.root.get() == Some(parent_id) && {
                let mut applier = self.borrow_applier();
                applier.get_mut(parent_id).is_err()
            };
            if stale_root_parent {
                parent_stack.pop();
                self.set_root(None);
            } else {
                let frame = parent_stack
                    .last_mut()
                    .expect("active parent frame should remain available");
                let attach_mode = frame.attach_mode;
                if parent_id == id {
                    return;
                }
                if matches!(attach_mode, ParentAttachMode::DeferredSync) {
                    frame.new_children.push(id);
                }
                drop(parent_stack);

                // KEY FIX: Set parent link IMMEDIATELY, matching Jetpack Compose's
                // LayoutNode.insertAt pattern where _foldedParent is set synchronously.
                // This ensures that when bubble_measure_dirty runs (in commands),
                // the parent chain is already established.
                //
                // IMPORTANT: Only set parent if node doesn't have one or if the new parent
                // is not the root. This prevents double-recomposition scenarios where a
                // child scope (invalidated by CompositionLocalProvider during parent's
                // recomposition) gets processed again with parent_stack=[root], which would
                // incorrectly reparent nodes to root.
                {
                    let mut applier = self.borrow_applier();
                    if let Ok(child_node) = applier.get_mut(id) {
                        let existing_parent = child_node.parent();
                        // Only set parent if:
                        // 1. Node has no parent, OR
                        // 2. New parent is NOT the root (parent_id != 0 or != self.root)
                        // This prevents root from stealing children that belong to intermediate nodes.
                        let should_set = if force_reparent_current_parent {
                            existing_parent != Some(parent_id)
                        } else {
                            match existing_parent {
                                None => true,
                                Some(existing) => {
                                    // Don't let root steal children from proper parents
                                    let root_id = self.core.root.get();
                                    parent_id != root_id.unwrap_or(0)
                                        || existing == root_id.unwrap_or(0)
                                }
                            }
                        };
                        if should_set {
                            child_node.set_parent_for_bubbling(parent_id);
                        }
                    }
                }
                if matches!(attach_mode, ParentAttachMode::ImmediateAppend) {
                    self.commands_mut().push(Command::AttachChild {
                        parent_id,
                        child_id: id,
                        bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                    });
                }
                return;
            }
        }
        drop(parent_stack);

        // No active parent - check if we're in subcompose
        let in_subcompose = !self.subcompose_stack().is_empty();
        if in_subcompose {
            // During subcompose, only add ROOT nodes (nodes without a parent).
            // Child nodes already have their parent-child relationship from composition;
            // re-adding them to the subcompose frame would cause duplication.
            let has_parent = {
                let mut applier = self.borrow_applier();
                applier
                    .get_mut(id)
                    .map(|node| node.parent().is_some())
                    .unwrap_or(false)
            };

            if !has_parent {
                let mut subcompose_stack = self.subcompose_stack();
                if let Some(frame) = subcompose_stack.last_mut() {
                    frame.nodes.push(id);
                }
            }
            return;
        }

        // During recomposition, preserve the original parent when possible.
        if let Some(parent_hint) = self.core.recranpose_parent_hint.get() {
            let parent_status = {
                let mut applier = self.borrow_applier();
                applier
                    .get_mut(id)
                    .map(|node| node.parent())
                    .unwrap_or(None)
            };
            match parent_status {
                Some(existing) if existing == parent_hint => {}
                None => {
                    self.commands_mut().push(Command::AttachChild {
                        parent_id: parent_hint,
                        child_id: id,
                        bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                    });
                }
                Some(_) => {}
            }
            return;
        }

        // Neither parent nor subcompose - check if this node already has a parent.
        // During recomposition, reused nodes already have their correct parent from
        // initial composition. We should NOT set them as root, as that would corrupt
        // the tree structure and cause duplication.
        let has_parent = {
            let mut applier = self.borrow_applier();
            applier
                .get_mut(id)
                .map(|node| node.parent().is_some())
                .unwrap_or(false)
        };
        if has_parent {
            // Node already has a parent, nothing to do
            return;
        }

        // Node has no parent and is not in subcompose - must be root
        self.set_root(Some(id));
    }

    pub fn with_node_mut<N: Node + 'static, R>(
        &self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> Result<R, NodeError> {
        let mut applier = self.borrow_applier();
        let node = applier.get_mut(id)?;
        let typed = node
            .as_any_mut()
            .downcast_mut::<N>()
            .ok_or(NodeError::TypeMismatch {
                id,
                expected: std::any::type_name::<N>(),
            })?;
        Ok(f(typed))
    }

    pub fn push_parent(&self, id: NodeId) {
        let reused = self.core.last_node_reused.take().unwrap_or(true);
        let in_subcompose = !self.core.subcompose_stack.borrow().is_empty();

        // Fresh parents usually append children directly, but a restored or otherwise
        // non-reused node can still carry attached children in the applier. In that case
        // we must diff against the live child list or stale descendants remain mounted.
        let mut previous = ChildList::new();
        if reused || in_subcompose {
            previous.extend(self.get_node_children(id));
        } else {
            let existing_children = self.get_node_children(id);
            if !existing_children.is_empty() {
                previous.extend(existing_children);
            }
        }
        let attach_mode = if in_subcompose || !previous.is_empty() {
            ParentAttachMode::DeferredSync
        } else {
            ParentAttachMode::ImmediateAppend
        };

        self.parent_stack().push(ParentFrame {
            id,
            previous,
            new_children: ChildList::new(),
            new_children_membership: None,
            attach_mode,
        });
    }

    pub fn pop_parent(&self) {
        let frame_opt = {
            let mut stack = self.parent_stack();
            stack.pop()
        };
        if let Some(frame) = frame_opt {
            let ParentFrame {
                id,
                previous,
                new_children,
                new_children_membership: _new_children_membership,
                attach_mode,
            } = frame;

            log::trace!(target: "cranpose::compose::parent", "pop_parent: node #{}", id);
            log::trace!(
                target: "cranpose::compose::parent",
                "previous children: {:?}",
                previous
            );
            log::trace!(
                target: "cranpose::compose::parent",
                "new children: {:?}",
                new_children
            );
            if matches!(attach_mode, ParentAttachMode::DeferredSync) {
                let _ = previous;
                self.commands_mut().push(Command::SyncChildren {
                    parent_id: id,
                    expected_children: new_children,
                });
            }
        }
    }

    pub(crate) fn take_commands(&self) -> CommandQueue {
        std::mem::take(&mut *self.commands_mut())
    }

    /// Applies any pending applier commands and runtime updates.
    ///
    /// This is useful during measure-time subcomposition to ensure newly created
    /// nodes are available for measurement before the full composition is committed.
    pub fn apply_pending_commands(&self) -> Result<(), NodeError> {
        let commands = self.take_commands();
        let runtime_handle = self.runtime_handle();
        {
            let mut applier = self.borrow_applier();
            commands.apply(&mut *applier)?;
            for update in runtime_handle.take_updates() {
                update.apply(&mut *applier)?;
            }
        }
        runtime_handle.drain_ui();
        Ok(())
    }

    pub fn register_side_effect(&self, effect: impl FnOnce() + 'static) {
        self.side_effects_mut().push(Box::new(effect));
    }

    pub fn take_side_effects(&self) -> Vec<Box<dyn FnOnce()>> {
        std::mem::take(&mut *self.side_effects_mut())
    }

    pub(crate) fn root(&self) -> Option<NodeId> {
        self.core.root.get()
    }

    pub(crate) fn set_root(&self, node: Option<NodeId>) {
        self.core.root.set(node);
    }
}
