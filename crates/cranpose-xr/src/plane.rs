//! A flat surface of the room.
#![cfg_attr(
    not(headset),
    expect(dead_code, reason = "the headset backend fills what the app reads")
)]

use std::{
    any::Any,
    cell::{Cell, Ref, RefCell},
    fmt,
    rc::{Rc, Weak},
};

use crate::{
    AnchorResult, PlaneState, PlaneType, Pose, spatial_store::SpatialStore,
    tracking_state::TrackingState,
};

// The runtime's name for a plane: a trackable of Android XR, or the UUID of one of Meta's spatial
// entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PlaneKey(pub(crate) u128);

// What the app's `Plane` handle shares with the backend.
pub(crate) struct PlaneCell {
    key: PlaneKey,
    plane_type: Cell<PlaneType>,
    state: Cell<PlaneState>,
    subsumed_by: Cell<Option<PlaneKey>>,
    vertices: RefCell<Vec<[f32; 2]>>,
    store: Weak<SpatialStore>,
    // What the backend holds for the plane in the runtime, which ends with the plane
    resource: RefCell<Option<Box<dyn Any>>>,
}

/// A flat surface of the room, such as a wall, the floor or a table. Jetpack XR's `Plane`. A
/// handle that the app clones and keeps. The backend updates it in place, so a plane that stays
/// tracked is the same plane in every read of [`Planes::all`](crate::Planes::all).
///
/// An app declares `Use::plane_tracking()` in its build and the user allows it. A Meta Quest shows
/// the planes that the user's room setup scanned. Android XR shows the planes that its cameras
/// found.
#[derive(Clone)]
pub struct Plane {
    cell: Rc<PlaneCell>,
}

impl Plane {
    pub(crate) fn new(
        key: PlaneKey,
        plane_type: PlaneType,
        state: PlaneState,
        store: Weak<SpatialStore>,
    ) -> Self {
        Self {
            cell: Rc::new(PlaneCell {
                key,
                plane_type: Cell::new(plane_type),
                state: Cell::new(state),
                subsumed_by: Cell::new(None),
                vertices: RefCell::new(Vec::new()),
                store,
                resource: RefCell::new(None),
            }),
        }
    }

    /// Which way the plane faces. Jetpack XR's `Plane.type`.
    pub fn plane_type(&self) -> PlaneType {
        self.cell.plane_type.get()
    }

    /// Where the plane is and what it is, as the last frame read it. A composition that reads it
    /// composes again when the plane is added, removed or relabeled, and when the runtime stops or
    /// starts tracking it. It does not compose again when the plane's pose or size changes by a
    /// refinement of the runtime. Jetpack XR's `Plane.state`.
    pub fn state(&self) -> PlaneState {
        if let Some(store) = self.cell.store.upgrade() {
            store.observe();
        }
        self.cell.state.get()
    }

    /// Calls `read` with the vertices of the convex polygon that approximates the plane, as pairs
    /// of meters along the X and the Z axes of the plane's
    /// [`center_pose`](PlaneState::center_pose), in counter-clockwise order. A plane with no
    /// polygon gives an empty slice, and the rectangle of its
    /// [`extents`](PlaneState::extents) stands for it. Jetpack XR's `Plane.State.vertices`.
    pub fn with_vertices<R>(&self, read: impl FnOnce(&[[f32; 2]]) -> R) -> R {
        read(&self.cell.vertices.borrow())
    }

    /// The plane that this plane was merged into, or `None` while no plane subsumes it. Both
    /// planes then describe the same surface, and [`Planes::all`](crate::Planes::all) lists only
    /// the one that subsumes. Jetpack XR's `Plane.State.subsumedBy`.
    pub fn subsumed_by(&self) -> Option<Self> {
        let key = self.cell.subsumed_by.get()?;
        self.cell.store.upgrade()?.plane_with_key(key)
    }

    /// Creates an anchor on this plane at `pose`, given in the app's room. Android XR attaches the
    /// anchor to the plane, so the anchor follows the runtime's refinement of the plane. Meta's
    /// runtime has anchors in space only, so it places the anchor at the pose. Jetpack XR's
    /// `Plane.createAnchor`.
    pub fn create_anchor(&self, pose: Pose) -> AnchorResult {
        match self.cell.store.upgrade() {
            Some(store) => store.create_anchor_on(self.cell.key, pose),
            None => AnchorResult::Unavailable,
        }
    }

    // The runtime's name for the plane.
    pub(crate) fn key(&self) -> PlaneKey {
        self.cell.key
    }

    // The part of the state that the backend gets from the runtime in every frame. No state
    // read changes, and nothing allocates.
    pub(crate) fn publish(&self, state: PlaneState) {
        self.cell.state.set(state);
    }

    // The plane as the backend last wrote it, for the backend's own decisions.
    pub(crate) fn current(&self) -> PlaneState {
        self.cell.state.get()
    }

    #[cfg(any(test, target_os = "android"))]
    pub(crate) fn set_plane_type(&self, plane_type: PlaneType) {
        self.cell.plane_type.set(plane_type);
    }

    #[cfg(any(test, target_os = "android"))]
    pub(crate) fn set_subsumed_by(&self, key: Option<PlaneKey>) {
        self.cell.subsumed_by.set(key);
    }

    pub(crate) fn is_subsumed(&self) -> bool {
        self.cell.subsumed_by.get().is_some()
    }

    // Replaces the polygon. The vertices keep their storage while the polygon has no more of
    // them than before.
    pub(crate) fn set_vertices(&self, vertices: impl IntoIterator<Item = [f32; 2]>) {
        let mut stored = self.cell.vertices.borrow_mut();
        stored.clear();
        stored.extend(vertices);
    }

    pub(crate) fn vertices(&self) -> Ref<'_, Vec<[f32; 2]>> {
        self.cell.vertices.borrow()
    }

    pub(crate) fn attach<T: Any>(&self, resource: T) {
        *self.cell.resource.borrow_mut() = Some(Box::new(resource));
    }

    pub(crate) fn with_resource<T: Any, R>(&self, read: impl FnOnce(&T) -> Option<R>) -> Option<R> {
        read(self.cell.resource.borrow().as_ref()?.downcast_ref::<T>()?)
    }

    pub(crate) fn is_tracking(&self) -> bool {
        self.cell.state.get().tracking_state == TrackingState::Tracking
    }
}

impl PartialEq for Plane {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.cell, &other.cell)
    }
}

impl Eq for Plane {}

impl fmt::Debug for Plane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plane")
            .field("type", &self.cell.plane_type.get())
            .field("state", &self.cell.state.get())
            .finish()
    }
}
