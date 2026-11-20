// Movable content coordination across slot tables during a composition pass.

use std::rc::{Rc, Weak};

use super::ComposerRuntimeState;
use crate::{
    AnchorId, Composer, Key, NodeError, RecomposeScope, RecomposeScopeInner, RetentionMode,
    ScopeId, SlotsHost, retention::RetainKey,
};

pub(super) struct GroupEntry {
    pub(super) key: crate::slot::GroupKey,
    // Boxed: a restore is rare, and the entry moves on every call.
    pub(super) restored: Option<Box<crate::slot::DetachedSubtree>>,
    pub(super) placeholder_for: Option<crate::slot::GroupKey>,
}

pub(super) fn movable_retain_key(id: Key) -> RetainKey {
    RetainKey::for_group(
        None,
        crate::slot::GroupKey::new(crate::slot::MOVABLE_STATIC_KEY, Some(id), 0),
    )
}

pub(crate) struct PendingMovable {
    pub(crate) key: crate::slot::GroupKey,
    pub(crate) placeholder: AnchorId,
    pub(crate) parent_scope: Option<Weak<RecomposeScopeInner>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MovableLookup {
    // First `show()` of a freshly created `rememberMovableContentOf` handle:
    // its private identity cannot be retained or attached in any slot table.
    FreshUnshown,
    // `rememberMovableContentOf` handle shown only in its owning slot table.
    OwnerHostOnly,
    // Keyed movable content or a handle shown across slot tables.
    AnyHost,
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl Composer {
    pub(crate) fn shared_state_weak(&self) -> Weak<ComposerRuntimeState> {
        Rc::downgrade(&self.core.shared_state)
    }

    pub(crate) fn active_slots_host_storage_key(&self) -> usize {
        self.active_slots_host().storage_key()
    }

    pub(super) fn wake_sites_whose_movable_arrived(&self) {
        for host in self.core.shared_state.hosts_awaiting_movables() {
            self.wake_sites_in_host(&host);
        }
    }

    fn wake_sites_in_host(&self, slots: &Rc<SlotsHost>) {
        let pending = self.core.shared_state.take_pending_movables(slots);
        if pending.is_empty() {
            return;
        }
        let mut waiting = Vec::new();
        for site in pending {
            if !slots.borrow().group_is_active(site.placeholder) {
                continue;
            }
            let retain_key = RetainKey::for_group(None, site.key);
            if !self.core.shared_state.movable_retained_anywhere(retain_key) {
                waiting.push(site);
                continue;
            }
            match site.parent_scope.as_ref().and_then(RecomposeScope::upgrade) {
                Some(scope) => {
                    scope.force_recompose();
                    scope.invalidate();
                }
                None => log::error!(
                    "movable content {:?} arrived for a site whose scope is gone",
                    site.key
                ),
            }
        }
        self.core.shared_state.keep_pending_movables(slots, waiting);
    }

    pub(crate) fn forget_movables(&self, ids: &[Key]) -> Result<(), NodeError> {
        let _pass = self.core.shared_state.movable_owners.enter_pass();
        for id in ids {
            let Some((host, subtree)) = self.core.shared_state.take_retained_movable(*id) else {
                continue;
            };
            self.dispose_movable_subtree_in_host(&host, subtree)?;
            host.flush_pending_drops();
        }
        self.dispose_dropped_movable_owners()
    }

    pub(super) fn handle_detached_children_in_host(
        &self,
        slots_host: &Rc<SlotsHost>,
        parent_scope: Option<ScopeId>,
        detached: Vec<crate::slot::DetachedSubtree>,
    ) -> Result<(), NodeError> {
        for mut subtree in detached {
            let is_movable_root = subtree
                .root_key_checked()
                .is_some_and(crate::slot::GroupKey::is_movable);
            let retention_mode = if is_movable_root {
                RetentionMode::RetainWhenInactive
            } else {
                subtree
                    .root_scope()
                    .map_or_default(RecomposeScope::retention_mode)
            };
            let nested_movables =
                if !is_movable_root && retention_mode == RetentionMode::DisposeWhenInactive {
                    subtree.split_off_unowned_nested_movables()
                } else {
                    subtree.split_off_nested_movables()
                };
            for movable in nested_movables {
                self.handle_detached_movable_in_host(slots_host, movable)?;
            }
            if is_movable_root {
                self.handle_detached_movable_in_host(slots_host, subtree)?;
                continue;
            }
            match retention_mode {
                RetentionMode::DisposeWhenInactive => {
                    self.dispose_detached_subtree_in_host(slots_host, subtree)?;
                }
                RetentionMode::RetainWhenInactive => {
                    self.retain_detached_subtree_in_host(slots_host, parent_scope, subtree)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn handle_detached_movable_in_host(
        &self,
        host: &Rc<SlotsHost>,
        subtree: crate::slot::DetachedSubtree,
    ) -> Result<(), NodeError> {
        if self.should_dispose_detached_movable(&subtree) {
            self.dispose_movable_subtree_in_host(host, subtree)
        } else {
            self.retain_detached_subtree_in_host(host, None, subtree)
        }
    }

    fn should_dispose_detached_movable(&self, subtree: &crate::slot::DetachedSubtree) -> bool {
        subtree
            .root_key_checked()
            .and_then(crate::slot::GroupKey::movable_id)
            .is_some_and(|id| {
                self.core
                    .shared_state
                    .movable_owners
                    .take_disposed_attached(id)
            })
    }

    fn dispose_movable_subtree_in_host(
        &self,
        host: &Rc<SlotsHost>,
        mut subtree: crate::slot::DetachedSubtree,
    ) -> Result<(), NodeError> {
        for nested in subtree.split_off_unowned_nested_movables() {
            self.handle_detached_movable_in_host(host, nested)?;
        }
        self.dispose_detached_subtree_in_host(host, subtree)
    }

    pub(super) fn settle_pass_movables(&self, slots: &Rc<SlotsHost>) -> Result<(), NodeError> {
        slots.flush_pending_drops();
        self.dispose_dropped_movable_owners()?;
        self.wake_sites_whose_movable_arrived();
        Ok(())
    }

    pub(super) fn dispose_dropped_movable_owners(&self) -> Result<(), NodeError> {
        if !self.core.shared_state.movable_owners.has_dropped() {
            return Ok(());
        }
        self.dispose_dropped_movable_owners_slow()
    }

    #[cold]
    #[inline(never)]
    fn dispose_dropped_movable_owners_slow(&self) -> Result<(), NodeError> {
        while let Some(dropped) = self.core.shared_state.movable_owners.pop_dropped() {
            if let Some((host, subtree)) = self
                .core
                .shared_state
                .take_retained_movable_in(dropped.id, dropped.owner_host)
            {
                self.dispose_movable_subtree_in_host(&host, subtree)?;
                host.flush_pending_drops();
            } else if self
                .core
                .shared_state
                .host_holding_movable_in(dropped.id, dropped.owner_host)
                .is_some()
            {
                self.core
                    .shared_state
                    .movable_owners
                    .note_disposed_attached(dropped.id);
            }
        }
        Ok(())
    }

    pub(super) fn resolve_group_entry(
        &self,
        host: &Rc<SlotsHost>,
        key: crate::slot::GroupKey,
        parent_scope_id: Option<ScopeId>,
        lookup: MovableLookup,
    ) -> GroupEntry {
        if lookup == MovableLookup::FreshUnshown {
            return GroupEntry {
                key,
                restored: None,
                placeholder_for: None,
            };
        }
        let retain_key = RetainKey::for_group(parent_scope_id, key);
        let restored = self
            .core
            .shared_state
            .take_retained(host, retain_key, |subtree| {
                host.with_write_session(|slots| slots.retained_restore_ready(key, subtree))
            })
            .or_else(|| {
                (lookup == MovableLookup::AnyHost)
                    .then(|| self.take_movable_from_another_table(host, retain_key, key))
                    .flatten()
            })
            .map(Box::new);
        if restored.is_some() || !key.is_movable() {
            return GroupEntry {
                key,
                restored,
                placeholder_for: None,
            };
        }
        let attached_elsewhere = self.movable_attached_elsewhere(host, key, lookup);
        if !attached_elsewhere {
            return GroupEntry {
                key,
                restored: None,
                placeholder_for: None,
            };
        }
        let id = key.explicit_key.unwrap_or_default();
        let placeholder = host.with_write_session(|slots| {
            slots.reserve_group_key(crate::slot::GroupKeySeed::movable_placeholder(id))
        });
        GroupEntry {
            key: placeholder,
            restored: None,
            placeholder_for: Some(key),
        }
    }

    // Movable content that was let go by a parent composed in another slot
    // table -- a `SubcomposeLayout`'s, or the one a subcomposition owns -- is
    // retained there, not here. Bring it over: the table that held it lets go
    // of its anchors, and this one issues its own.
    fn take_movable_from_another_table(
        &self,
        host: &Rc<SlotsHost>,
        retain_key: RetainKey,
        key: crate::slot::GroupKey,
    ) -> Option<crate::slot::DetachedSubtree> {
        if !key.is_movable() {
            return None;
        }
        let (source, mut subtree) = self
            .core
            .shared_state
            .take_retained_movable_elsewhere(host, retain_key)?;
        source
            .borrow_mut()
            .invalidate_detached_subtree_anchors(&subtree);
        if host.with_write_session(|slots| slots.retained_restore_ready(key, &mut subtree)) {
            return Some(subtree);
        }
        log::error!(
            "movable content {key:?} could not be taken over by the slot table that asked for it"
        );
        if let Err(error) = self.dispose_detached_subtree_in_host(host, subtree) {
            log::error!("disposing movable content that could not move failed: {error}");
        }
        None
    }

    // Whether the movable's content is attached to some parent that is not the
    // one composing, in this slot table or in another.
    fn movable_attached_elsewhere(
        &self,
        host: &Rc<SlotsHost>,
        key: crate::slot::GroupKey,
        lookup: MovableLookup,
    ) -> bool {
        match host.with_write_session(|slots| slots.movable_attachment(key)) {
            crate::slot::MovableAttachment::AttachedHere => false,
            crate::slot::MovableAttachment::AttachedElsewhereInHost => true,
            crate::slot::MovableAttachment::NotAttachedHere => {
                if lookup != MovableLookup::AnyHost {
                    return false;
                }
                let Some(id) = key.movable_id() else {
                    return false;
                };
                self.core
                    .shared_state
                    .host_holding_movable(id)
                    .is_some_and(|holder| !Rc::ptr_eq(&holder, host))
            }
        }
    }
}
