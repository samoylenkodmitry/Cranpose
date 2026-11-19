// Movable content coordination across slot tables during a composition pass.

use std::rc::{Rc, Weak};

use super::ComposerRuntimeState;
use crate::{
    AnchorId, Composer, Key, NodeError, RecomposeScope, RecomposeScopeInner, ScopeId, SlotsHost,
    retention::RetainKey,
};

pub(super) struct GroupEntry {
    pub(super) key: crate::slot::GroupKey,
    // Boxed: a restore is rare, and the entry moves on every call.
    pub(super) restored: Option<Box<crate::slot::DetachedSubtree>>,
    pub(super) placeholder_for: Option<crate::slot::GroupKey>,
}

pub(crate) struct PendingMovable {
    pub(crate) key: crate::slot::GroupKey,
    pub(crate) placeholder: AnchorId,
    pub(crate) parent_scope: Option<Weak<RecomposeScopeInner>>,
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl Composer {
    pub(crate) fn shared_state_weak(&self) -> Weak<ComposerRuntimeState> {
        Rc::downgrade(&self.core.shared_state)
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
        let _pass = self.core.shared_state.enter_compose_pass();
        for id in ids {
            let Some((host, subtree)) = self.core.shared_state.take_retained_movable(*id) else {
                continue;
            };
            self.dispose_movable_subtree_in_host(&host, subtree)?;
            host.flush_pending_drops();
        }
        self.dispose_dropped_movable_owners()
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
            .is_some_and(|id| self.core.shared_state.take_disposed_attached_movable(id))
    }

    fn dispose_movable_subtree_in_host(
        &self,
        host: &Rc<SlotsHost>,
        mut subtree: crate::slot::DetachedSubtree,
    ) -> Result<(), NodeError> {
        for nested in subtree.split_off_nested_movables() {
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
        if !self.core.shared_state.has_dropped_movable_owners() {
            return Ok(());
        }
        self.dispose_dropped_movable_owners_slow()
    }

    #[cold]
    #[inline(never)]
    fn dispose_dropped_movable_owners_slow(&self) -> Result<(), NodeError> {
        loop {
            let ids = self.core.shared_state.take_dropped_movable_owners();
            if ids.is_empty() {
                return Ok(());
            }
            for id in ids {
                if let Some((host, subtree)) = self.core.shared_state.take_retained_movable(id) {
                    self.dispose_movable_subtree_in_host(&host, subtree)?;
                    host.flush_pending_drops();
                } else if self.core.shared_state.host_holding_movable(id).is_some() {
                    self.core.shared_state.note_disposed_attached_movable(id);
                }
            }
        }
    }

    pub(super) fn resolve_group_entry(
        &self,
        host: &Rc<SlotsHost>,
        seed: crate::slot::GroupKeySeed,
        parent_scope_id: Option<ScopeId>,
    ) -> GroupEntry {
        let key = host.with_write_session(|slots| slots.reserve_group_key(seed));
        let retain_key = RetainKey::for_group(parent_scope_id, key);
        let restored = self
            .core
            .shared_state
            .take_retained(host, retain_key, |subtree| {
                host.with_write_session(|slots| slots.retained_restore_ready(key, subtree))
            })
            .or_else(|| self.take_movable_from_another_table(host, retain_key, key))
            .map(Box::new);
        if restored.is_some() || !key.is_movable() {
            return GroupEntry {
                key,
                restored,
                placeholder_for: None,
            };
        }
        let attached_elsewhere = self.movable_attached_elsewhere(host, key);
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
    fn movable_attached_elsewhere(&self, host: &Rc<SlotsHost>, key: crate::slot::GroupKey) -> bool {
        if host.with_write_session(|slots| slots.movable_attached_elsewhere(key)) {
            return true;
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
