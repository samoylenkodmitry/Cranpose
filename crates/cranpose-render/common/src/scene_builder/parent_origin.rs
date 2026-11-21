// Where the children of a record lie in the window, which the records above
// a dirty record pass to its update. Only a node that publishes window
// origins reads it, a text field or a scroll viewport, so an update works it
// out where such a node is. The context holds it by reference, so a step
// that passes it on copies a few words.

use std::cell::OnceCell;

use cranpose_core::RecordId;

use super::AbsOrigin;
use crate::scene_store::SceneStore;

#[derive(Clone, Copy)]
pub(super) enum ParentOrigin<'a> {
    // Worked out by the step that passes it.
    Known(&'a AbsOrigin),
    // Where the children of a record lie, worked out at the first read.
    Beneath(&'a ChildrenOrigin),
    // No node beneath reads it in this update.
    Unread,
}

impl<'a> ParentOrigin<'a> {
    pub(super) fn resolve(self, store: &SceneStore) -> Option<&'a AbsOrigin> {
        match self {
            Self::Known(origin) => Some(origin),
            Self::Beneath(children) => Some(children.read(store)),
            Self::Unread => None,
        }
    }
}

// Where the children of one record lie in the window: worked out by a climb
// to the root at the first read, then kept for the record's other dirty
// children.
pub(super) struct ChildrenOrigin {
    record: RecordId,
    origin: OnceCell<AbsOrigin>,
}

impl ChildrenOrigin {
    pub(super) fn of(record: RecordId) -> Self {
        Self {
            record,
            origin: OnceCell::new(),
        }
    }

    pub(super) fn record(&self) -> RecordId {
        self.record
    }

    pub(super) fn read(&self, store: &SceneStore) -> &AbsOrigin {
        self.origin
            .get_or_init(|| children_origin(store, self.record))
    }
}

// Where the children of `record` lie: each record above places its children.
fn children_origin(store: &SceneStore, record: RecordId) -> AbsOrigin {
    let above = store
        .parent(record)
        .map_or(AbsOrigin::ROOT, |parent| children_origin(store, parent));
    above.children_of(store.layer(record))
}
