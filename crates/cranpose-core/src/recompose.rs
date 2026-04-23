use crate::{
    debug_scope_invalidation_sources, debug_scope_label, Composer, ComposerCore, NodeId,
    RecomposeScope,
};
use std::rc::Rc;

impl Composer {
    pub(crate) fn recranpose_group(&self, scope: &RecomposeScope) {
        struct RecomposeGuard {
            composer: Composer,
            scope: RecomposeScope,
        }

        impl Drop for RecomposeGuard {
            fn drop(&mut self) {
                let crate::slot::FinishGroupResult {
                    detached_children,
                    structure_changed: _structure_changed,
                    direct_nodes,
                } = self
                    .composer
                    .with_slot_session_mut(|slots| slots.finish_group_body());
                self.composer.dispose_detached_nodes(direct_nodes);
                self.composer
                    .handle_detached_children(Some(self.scope.id()), detached_children);
                self.composer.scope_stack().pop();
                self.composer
                    .with_slot_session_mut(|slots| slots.end_recompose());
                log::trace!(
                    target: "cranpose::compose::recompose",
                    "scope_id={} label={:?} ended",
                    self.scope.id(),
                    debug_scope_label(self.scope.id()),
                );
                self.scope.mark_recomposed();
                self.composer.flush_pending_commands_if_large();
            }
        }

        // CRITICAL FIX: Check if scope is still invalid before recomposing.
        // When parent and child scopes are both invalidated, the child may be
        // visited (and marked recomposed) during parent's recomposition.
        // Without this check, we'd recompose the child again with wrong parent_stack,
        // causing nodes to get attached to root instead of their actual parent.
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
            let previous_hint = self
                .core
                .recranpose_parent_hint
                .replace(scope.parent_hint());
            struct HintGuard {
                core: Rc<ComposerCore>,
                previous: Option<NodeId>,
            }
            impl Drop for HintGuard {
                fn drop(&mut self) {
                    self.core.recranpose_parent_hint.set(self.previous);
                }
            }
            let _hint_guard = HintGuard {
                core: self.clone_core(),
                previous: previous_hint,
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
                } else {
                    self.request_root_render();
                }
                // Preserve all existing slot content until the promoted ancestor
                // or the requested root render replays the subtree.
                self.skip_current_group();
            }
            {
                let mut locals = self.local_stack();
                *locals = saved_locals;
            }
            drop(guard);
        } else {
            scope.mark_recomposed();
        }
    }
}
