// Where the children of a record lie in the window, which the records above
// a dirty record pass to its update. Only a node that publishes window
// origins reads it, a text field or a scroll viewport, so an update works it
// out where such a node is.

use cranpose_core::RecordId;

use super::AbsOrigin;
use crate::scene_store::SceneStore;

#[derive(Clone, Copy)]
pub(super) enum ParentOrigin {
    Known(AbsOrigin),
    // Where the children of this record lie, worked out by a climb to the
    // root when an update reads it.
    Beneath(RecordId),
    // No node beneath reads it in this update.
    Unread,
}

impl ParentOrigin {
    pub(super) fn resolve(self, store: &SceneStore) -> Option<AbsOrigin> {
        match self {
            Self::Known(origin) => Some(origin),
            Self::Beneath(record) => Some(children_origin(store, record)),
            Self::Unread => None,
        }
    }
}

// Where the children of `record` lie: each record above places its children.
pub(super) fn children_origin(store: &SceneStore, record: RecordId) -> AbsOrigin {
    let above = store
        .parent(record)
        .map_or(AbsOrigin::ROOT, |parent| children_origin(store, parent));
    above.children_of(store.layer(record))
}
