use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    Command, Composer, ComposerCore, DirtyBubble, NodeId, RecomposeScope,
    debug_scope_invalidation_sources, debug_scope_label,
};

impl Composer {
    fn scope_child_cursor(
        &self,
        scope: &RecomposeScope,
        parent_hint: Option<NodeId>,
    ) -> Option<usize> {
        let parent_hint = parent_hint?;
        let roots =
            self.with_slot_session_mut(|slots| slots.active_scope_root_node_ids(scope.id()));
        let first = roots.first().copied()?;
        let mut applier = self.borrow_applier();
        let mut siblings: SmallVec<[NodeId; 8]> = SmallVec::new();
        applier
            .get_mut(parent_hint)
            .ok()?
            .collect_owned_children_into(&mut siblings);
        siblings.iter().position(|&sibling| sibling == first)
    }

    pub(crate) fn recompose_group(&self, scope: &RecomposeScope) {
        struct RecomposeGuard {
            composer: Composer,
            scope: RecomposeScope,
        }

        impl Drop for RecomposeGuard {
            fn drop(&mut self) {
                self.composer
                    .close_current_group_body_for_scope(&self.scope);
                self.composer
                    .with_slot_session_mut(|slots| slots.end_recompose());
                log::trace!(
                    target: "cranpose::compose::recompose",
                    "scope_id={} label={:?} ended",
                    self.scope.id(),
                    debug_scope_label(self.scope.id()),
                );
                self.scope.mark_recomposed();
                if let Err(err) = self.composer.flush_pending_commands_if_large() {
                    log::error!("mid-recomposition command flush failed: {err}");
                }
            }
        }

        if !scope.is_effectively_active() {
            scope.defer_until_reactivated();
            return;
        }

        if !scope.is_invalid() {
            scope.mark_recomposed();
            return;
        }
        let started =
            self.with_slot_session_mut(|slots| slots.begin_recompose_at_scope(scope.id()));
        log::trace!(
            target: "cranpose::compose::recompose",
            "scope_id={} label={:?} started_at={started:?} sources={:?}",
            scope.id(),
            debug_scope_label(scope.id()),
            debug_scope_invalidation_sources(scope.id()),
        );
        if started.is_some() {
            let parent_hint = scope.parent_hint();
            let previous_hint = self.core.recompose_parent_hint.replace(parent_hint);
            let previous_cursor = self
                .core
                .recompose_child_cursor
                .replace(self.scope_child_cursor(scope, parent_hint));
            struct HintGuard {
                core: Rc<ComposerCore>,
                previous: Option<NodeId>,
                previous_cursor: Option<usize>,
            }
            impl Drop for HintGuard {
                fn drop(&mut self) {
                    self.core.recompose_parent_hint.set(self.previous);
                    self.core.recompose_child_cursor.set(self.previous_cursor);
                }
            }
            let _hint_guard = HintGuard {
                core: self.clone_core(),
                previous: previous_hint,
                previous_cursor,
            };
            {
                let mut stack = self.scope_stack();
                stack.push(scope.clone());
            }
            let guard = RecomposeGuard {
                composer: self.clone(),
                scope: scope.clone(),
            };
            let saved_locals = self.current_local_stack();
            {
                let mut locals = self.local_stack();
                *locals = scope.local_stack();
            }
            let callback_ran = self.observe_scope(scope, || scope.run_recompose(self));
            log::trace!(
                target: "cranpose::compose::recompose",
                "scope_id={} label={:?} callback_ran={}",
                scope.id(),
                debug_scope_label(scope.id()),
                callback_ran,
            );
            if !callback_ran {
                if let Some(ancestor_scope) = scope.callback_promotion_target() {
                    ancestor_scope.invalidate();
                    scope.request_pending_recompose();
                } else {
                    self.request_root_render();
                }
                self.skip_current_group();
            }
            {
                let mut locals = self.local_stack();
                *locals = saved_locals;
            }
            drop(guard);
        } else {
            if let Some(ancestor_scope) = scope.callback_promotion_target() {
                ancestor_scope.invalidate();
            } else {
                self.request_root_render();
            }
            if let Some(parent_hint) = scope.parent_hint().or_else(|| self.root()) {
                self.commands_mut().push(Command::BubbleDirty {
                    node_id: parent_hint,
                    bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                });
            }
            scope.mark_recomposed();
        }
    }
}
