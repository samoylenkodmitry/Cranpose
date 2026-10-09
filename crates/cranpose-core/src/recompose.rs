use std::rc::Rc;

use crate::{
    Command, Composer, ComposerCore, DirtyBubble, NodeId, RecomposeScope,
    debug_scope_invalidation_sources, debug_scope_label,
};

#[derive(Clone, Copy)]
pub(crate) enum RecomposeChildCursor {
    Unknown,
    After { first: NodeId, placed: usize },
    At(usize),
}

impl Composer {
    fn scope_child_cursor(
        &self,
        scope: &RecomposeScope,
        parent_hint: Option<NodeId>,
    ) -> RecomposeChildCursor {
        if parent_hint.is_none() {
            return RecomposeChildCursor::Unknown;
        }
        self.with_slot_session_mut(|slots| slots.active_scope_first_root_node_id(scope))
            .map_or(RecomposeChildCursor::Unknown, |first| {
                RecomposeChildCursor::After { first, placed: 0 }
            })
    }

    pub(crate) fn resolve_recompose_child_cursor(&self) -> RecomposeChildCursor {
        let cursor = self.core.recompose_child_cursor.get();
        let RecomposeChildCursor::After { first, placed } = cursor else {
            return cursor;
        };
        let index = self.core.recompose_parent_hint.get().and_then(|parent| {
            let mut applier = self.borrow_applier();
            applier.get_mut(parent).ok()?.owned_child_index(first)
        });
        let resolved = index.map_or(RecomposeChildCursor::Unknown, |index| {
            RecomposeChildCursor::At(index + placed)
        });
        self.core.recompose_child_cursor.set(resolved);
        resolved
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
                #[expect(
                    clippy::redundant_closure_for_method_calls,
                    reason = "the method path is not general over the session lifetime"
                )]
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
        let started = self.with_slot_session_mut(|slots| slots.begin_recompose_at_scope(scope));
        log::trace!(
            target: "cranpose::compose::recompose",
            "scope_id={} label={:?} started_at={started:?} sources={:?}",
            scope.id(),
            debug_scope_label(scope.id()),
            debug_scope_invalidation_sources(scope.id()),
        );
        if started.is_some() {
            let parent_hint = scope.parent_hint();
            let previous_cursor = self.resolve_recompose_child_cursor();
            let cursor = self.scope_child_cursor(scope, parent_hint);
            let previous_hint = self.core.recompose_parent_hint.replace(parent_hint);
            self.core.recompose_child_cursor.set(cursor);
            struct HintGuard {
                core: Rc<ComposerCore>,
                previous: Option<NodeId>,
                previous_cursor: RecomposeChildCursor,
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
            let callback_ran = scope.run_recompose(self);
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
