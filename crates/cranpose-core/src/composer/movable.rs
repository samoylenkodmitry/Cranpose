// Movable content coordination across slot tables during a composition pass.

use std::rc::{Rc, Weak};

use crate::{
    AnchorId, Composer, Key, NodeError, RecomposeScope, RecomposeScopeInner, SlotsHost,
    retention::RetainKey,
};

pub(crate) struct PendingMovable {
    pub(crate) key: crate::slot::GroupKey,
    pub(crate) placeholder: AnchorId,
    pub(crate) parent_scope: Option<Weak<RecomposeScopeInner>>,
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl Composer {
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
        for id in ids {
            let Some((host, subtree)) = self.core.shared_state.take_retained_movable(*id) else {
                continue;
            };
            self.dispose_detached_subtree_in_host(&host, subtree)?;
            host.flush_pending_drops();
        }
        Ok(())
    }

    // Movable content that was let go by a parent composed in another slot
    // table -- a `SubcomposeLayout`'s, or the one a subcomposition owns -- is
    // retained there, not here. Bring it over: the table that held it lets go
    // of its anchors, and this one issues its own.
    pub(super) fn take_movable_from_another_table(
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
    pub(super) fn movable_attached_elsewhere(
        &self,
        host: &Rc<SlotsHost>,
        key: crate::slot::GroupKey,
    ) -> bool {
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
