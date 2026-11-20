//! A place in the room that the runtime holds still.
#![cfg_attr(
    not(headset),
    expect(dead_code, reason = "the headset backend fills what the app reads")
)]

use std::{
    any::Any,
    cell::{Cell, RefCell},
    fmt,
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

use crate::{
    AnchorPersistence, AnchorState, Pose, TrackingState, Uuid, plane::PlaneKey,
    spatial_store::SpatialStore,
};

// How the runtime makes an anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnchorOrigin {
    // In space, at the pose that the app gave
    Space,
    // On a plane, at the pose that the app gave
    Plane(PlaneKey),
    // From the runtime's storage, at the place where it was persisted
    Persisted(Uuid),
}

// What the app's `Anchor` handle shares with the backend.
pub(crate) struct AnchorCell {
    id: u64,
    origin: AnchorOrigin,
    store: Weak<SpatialStore>,
    pose: Cell<Pose>,
    // Whether `pose` is where the anchor is: the app gave it, or the runtime placed the anchor
    located: Cell<bool>,
    tracking: Cell<TrackingState>,
    persistence: Cell<AnchorPersistence>,
    // Whether the backend asked the runtime to keep the anchor
    persist_sent: Cell<bool>,
    detached: Cell<bool>,
    // What the backend holds for the anchor in the runtime, which ends with the anchor
    resource: RefCell<Option<Box<dyn Any>>>,
}

/// A place and a turn in the room that the runtime holds still while it learns more of the room.
/// Jetpack XR's `Anchor`. A handle that the app clones and keeps. Its last clone ends the anchor,
/// or [`detach`](Self::detach) does.
///
/// An app creates one with [`Anchors::create`](crate::Anchors::create) or
/// [`Plane::create_anchor`](crate::Plane::create_anchor), keeps it for its next start with
/// [`persist`](Self::persist), loads it back with [`Anchors::load`](crate::Anchors::load), and
/// holds content to it with `FollowTarget::Anchor`. The backend reads the anchor's pose from the
/// runtime in every frame and allocates nothing for it.
#[derive(Clone)]
pub struct Anchor {
    cell: Rc<AnchorCell>,
}

impl Anchor {
    // A persisted anchor has no place until the runtime gives one. Every other anchor stands at the
    // pose that the app gave.
    pub(crate) fn new(
        id: u64,
        origin: AnchorOrigin,
        pose: Pose,
        persistence: AnchorPersistence,
        store: Weak<SpatialStore>,
    ) -> Self {
        Self {
            cell: Rc::new(AnchorCell {
                id,
                origin,
                store,
                pose: Cell::new(pose),
                located: Cell::new(!matches!(origin, AnchorOrigin::Persisted(_))),
                tracking: Cell::new(TrackingState::Paused),
                persistence: Cell::new(persistence),
                persist_sent: Cell::new(false),
                detached: Cell::new(false),
                resource: RefCell::new(None),
            }),
        }
    }

    /// Where the anchor is and whether the runtime tracks it, as the last frame read it. A
    /// composition that reads it composes again when the tracking state changes. It does not
    /// compose again when the pose moves by a refinement of the runtime: a frame effect reads the
    /// pose in every frame. Jetpack XR's `Anchor.state`.
    pub fn state(&self) -> AnchorState {
        if let Some(store) = self.cell.store.upgrade() {
            store.observe();
        }
        AnchorState {
            pose: self.cell.pose.get(),
            tracking_state: self.cell.tracking.get(),
        }
    }

    /// How far the anchor has come in being kept for the app's next start. A composition that
    /// reads it composes again when the answer changes.
    pub fn persistence(&self) -> AnchorPersistence {
        if let Some(store) = self.cell.store.upgrade() {
            store.observe();
        }
        self.cell.persistence.get()
    }

    /// The identifier under which the runtime keeps the anchor, or `None` until it does.
    pub fn uuid(&self) -> Option<Uuid> {
        match self.persistence() {
            AnchorPersistence::Persisted(uuid) => Some(uuid),
            _ => None,
        }
    }

    /// Asks the runtime to keep the anchor for the app's next start. The runtime answers within
    /// some frames: [`persistence`](Self::persistence) goes from
    /// [`Persisting`](AnchorPersistence::Persisting) to [`Persisted`](AnchorPersistence::Persisted)
    /// with the identifier, or to [`Failed`](AnchorPersistence::Failed). A call on an anchor that is
    /// persisting or persisted changes nothing, and a call after a failure asks again. Jetpack XR's
    /// suspending `Anchor.persist`.
    pub fn persist(&self) {
        if let Some(store) = self.cell.store.upgrade() {
            store.persist(self);
        }
    }

    /// Ends the anchor: the runtime stops tracking it, and its state becomes
    /// [`Stopped`](TrackingState::Stopped). Content that follows it stays where it was. A persisted
    /// anchor stays in the runtime's storage until [`Anchors::unpersist`](crate::Anchors::unpersist).
    /// Jetpack XR's `Anchor.detach`.
    pub fn detach(&self) {
        if let Some(store) = self.cell.store.upgrade() {
            store.detach(self);
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.cell.id
    }

    pub(crate) fn origin(&self) -> AnchorOrigin {
        self.cell.origin
    }

    pub(crate) fn is_detached(&self) -> bool {
        self.cell.detached.get()
    }

    pub(crate) fn mark_detached(&self) {
        self.cell.detached.set(true);
        self.cell.tracking.set(TrackingState::Stopped);
    }

    // Whether the anchor stands where the pose says. A persisted anchor has no place until the
    // runtime gives one.
    pub(crate) fn is_located(&self) -> bool {
        self.cell.located.get()
    }

    // The pose as the app and the backend last knew it, with no subscription.
    pub(crate) fn pose(&self) -> Pose {
        self.cell.pose.get()
    }

    // Takes what the runtime says of the anchor in a frame: its pose when it has one, and whether
    // it tracks the anchor. Returns whether the tracking state changed, which reaches the compositions
    // that read it.
    pub(crate) fn publish(&self, pose: Option<Pose>, tracking: TrackingState) -> bool {
        if self.cell.detached.get() {
            return false;
        }
        if let Some(pose) = pose {
            self.cell.pose.set(pose);
            self.cell.located.set(true);
        }
        let changed = self.cell.tracking.get() != tracking;
        self.cell.tracking.set(tracking);
        changed
    }

    pub(crate) fn raw_persistence(&self) -> AnchorPersistence {
        self.cell.persistence.get()
    }

    pub(crate) fn set_persistence(&self, persistence: AnchorPersistence) {
        self.cell.persistence.set(persistence);
    }

    // Whether the app asked for persistence and the backend has not passed the ask on
    pub(crate) fn needs_persist(&self) -> bool {
        self.cell.persistence.get() == AnchorPersistence::Persisting
            && !self.cell.persist_sent.get()
    }

    pub(crate) fn mark_persist_sent(&self) {
        self.cell.persist_sent.set(true);
    }

    // Whether only the store holds the anchor: the app dropped every handle
    pub(crate) fn is_unreferenced(&self) -> bool {
        Rc::strong_count(&self.cell) == 1
    }

    pub(crate) fn attach<T: Any>(&self, resource: T) {
        *self.cell.resource.borrow_mut() = Some(Box::new(resource));
    }

    pub(crate) fn is_attached(&self) -> bool {
        self.cell.resource.borrow().is_some()
    }

    pub(crate) fn with_resource<T: Any, R>(&self, read: impl FnOnce(&T) -> R) -> Option<R> {
        self.cell
            .resource
            .borrow()
            .as_ref()
            .and_then(|resource| resource.downcast_ref::<T>())
            .map(read)
    }
}

impl PartialEq for Anchor {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.cell, &other.cell)
    }
}

impl Eq for Anchor {}

impl Hash for Anchor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.cell.id.hash(state);
    }
}

impl fmt::Debug for Anchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anchor")
            .field("id", &self.cell.id)
            .field("pose", &self.cell.pose.get())
            .field("tracking", &self.cell.tracking.get())
            .field("persistence", &self.cell.persistence.get())
            .finish()
    }
}
