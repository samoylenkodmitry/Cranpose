use crate::{
    Command, Composer, ComposerCore, DirtyBubble, NodeId, RecomposeScope,
    debug_scope_invalidation_sources, debug_scope_label,
    slot::{RootBefore, SlotWriteSession},
};

#[derive(Clone, Copy)]
pub(crate) enum RecomposeChildCursor {
    Unknown,
    After { first: NodeId, placed: usize },
    At(usize),
}

impl RecomposeChildCursor {
    // Where the roots of the open group go in `parent_hint`: where its first
    // root is, or after what the composition emitted there before the group
    // when it holds none.
    fn at_open_group(slots: &SlotWriteSession<'_>, parent_hint: Option<NodeId>) -> Self {
        let Some(parent) = parent_hint else {
            return Self::Unknown;
        };
        match slots.open_group_first_root_node() {
            Some(first) => Self::After { first, placed: 0 },
            None => Self::after_root_before(slots, parent),
        }
    }

    // Where the first root of the open group goes when it holds none yet.
    #[cold]
    #[inline(never)]
    fn after_root_before(slots: &SlotWriteSession<'_>, parent: NodeId) -> Self {
        match slots.open_group_root_before(parent) {
            Some(RootBefore::Node(previous)) => Self::After {
                first: previous,
                placed: 1,
            },
            Some(RootBefore::Nothing) => Self::At(0),
            None => Self::Unknown,
        }
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl Composer {
    fn begin_scope_group(&self, scope: &RecomposeScope) -> Option<RecomposeChildCursor> {
        let parent_hint = scope.parent_hint();
        let started = self.with_slot_session_mut(|slots| {
            slots.begin_recompose_at_scope(scope).map(|group| {
                (
                    group,
                    RecomposeChildCursor::at_open_group(slots, parent_hint),
                )
            })
        });
        log::trace!(
            target: "cranpose::compose::recompose",
            "scope_id={} label={:?} started_at={:?} sources={:?}",
            scope.id(),
            debug_scope_label(scope.id()),
            started.map(|(group, _)| group),
            debug_scope_invalidation_sources(scope.id()),
        );
        started.map(|(_, cursor)| cursor)
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

    // A derivation's callback derives its value again from the derived
    // state it holds and reads no slot: it runs without its group, with its
    // scope current for the states it reads to subscribe.
    fn rerun_derivation(&self, scope: &RecomposeScope) {
        self.scope_stack().push(scope.clone());
        let saved_locals = std::mem::replace(&mut *self.local_stack(), scope.local_stack());
        scope.run_recompose(self);
        *self.local_stack() = saved_locals;
        self.scope_stack().pop();
        scope.mark_recomposed();
    }

    pub(crate) fn recompose_group(&self, scope: &RecomposeScope) {
        if !scope.is_effectively_active() {
            scope.defer_until_reactivated();
        } else if !scope.is_invalid() {
            scope.mark_recomposed();
        } else if scope.is_derivation() {
            self.rerun_derivation(scope);
        } else {
            self.recompose_content(scope);
        }
    }

    // Runs the body of an invalid scope again in its group.
    fn recompose_content(&self, scope: &RecomposeScope) {
        struct RecomposeGuard<'a> {
            composer: &'a Composer,
            scope: &'a RecomposeScope,
        }

        impl Drop for RecomposeGuard<'_> {
            fn drop(&mut self) {
                self.composer.close_current_group_body_for_scope(self.scope);
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

        if let Some(cursor) = self.begin_scope_group(scope) {
            let parent_hint = scope.parent_hint();
            let previous_cursor = self.resolve_recompose_child_cursor();
            let previous_hint = self.core.recompose_parent_hint.replace(parent_hint);
            self.core.recompose_child_cursor.set(cursor);
            let previous_restarted = self.core.restarted_scope.replace(Some(scope.clone()));
            struct HintGuard<'a> {
                core: &'a ComposerCore,
                previous: Option<NodeId>,
                previous_cursor: RecomposeChildCursor,
                previous_restarted: Option<RecomposeScope>,
            }
            impl Drop for HintGuard<'_> {
                fn drop(&mut self) {
                    self.core.recompose_parent_hint.set(self.previous);
                    self.core.recompose_child_cursor.set(self.previous_cursor);
                    *self.core.restarted_scope.borrow_mut() = self.previous_restarted.take();
                }
            }
            let _hint_guard = HintGuard {
                core: &self.core,
                previous: previous_hint,
                previous_cursor,
                previous_restarted,
            };
            {
                let mut stack = self.scope_stack();
                stack.push(scope.clone());
            }
            let guard = RecomposeGuard {
                composer: self,
                scope,
            };
            let saved_locals = std::mem::replace(&mut *self.local_stack(), scope.local_stack());
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
            *self.local_stack() = saved_locals;
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
