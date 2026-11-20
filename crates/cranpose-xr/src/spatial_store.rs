//! The anchors and planes of a session, which the app reads and the backend fills.
#![cfg_attr(
    not(headset),
    expect(dead_code, reason = "the headset backend fills what the app reads")
)]

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use cranpose::{MutableState, mutableStateOf};

use crate::{
    Anchor, AnchorCapabilities, AnchorPersistence, AnchorResult, Plane, Pose, TrackingAvailability,
    TrackingState, Uuid, anchor::AnchorOrigin, plane::PlaneKey,
};
#[cfg(any(test, headset))]
use crate::{
    plane_filter::PlaneFilter,
    plane_snap::{Content, Surface, snap},
};

// What the app and the backend share about the room's anchors and planes. The app's handles
// (`Anchors`, `Planes`, `Anchor`, `Plane`) call the first group of methods in a composition or a
// callback. The backend calls the second group in each frame, and a frame allocates nothing.
//
// Everything runs on the thread of the composition, so the store needs no lock. One state counts
// the changes that a composition can observe: an anchor's tracking state and persistence, the list
// of persisted identifiers and the planes that a frame adds, removes or relabels. A pose that
// moves by a refinement is not such a change, so a moving pose writes no state.
pub(crate) struct SpatialStore {
    me: Weak<Self>,
    anchors: RefCell<Vec<Anchor>>,
    persisted: RefCell<Vec<Uuid>>,
    unpersisting: RefCell<Vec<Uuid>>,
    planes: RefCell<Vec<Plane>>,
    capabilities: Cell<AnchorCapabilities>,
    planes_availability: Cell<TrackingAvailability>,
    head_tracked: Cell<bool>,
    next_id: Cell<u64>,
    epoch: MutableState<u64>,
}

impl SpatialStore {
    // A store for a session in which the runtime offers `capabilities` and `planes`. It needs a
    // runtime for its state, so a composition makes it.
    pub(crate) fn new(capabilities: AnchorCapabilities, planes: TrackingAvailability) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            anchors: RefCell::new(Vec::new()),
            persisted: RefCell::new(Vec::new()),
            unpersisting: RefCell::new(Vec::new()),
            planes: RefCell::new(Vec::new()),
            capabilities: Cell::new(capabilities),
            planes_availability: Cell::new(planes),
            head_tracked: Cell::new(true),
            next_id: Cell::new(1),
            epoch: mutableStateOf(0),
        })
    }

    // Makes the composition that reads a change of the store read it again.
    pub(crate) fn observe(&self) {
        self.epoch.get();
    }

    fn touch(&self) {
        self.epoch.set(self.epoch.get_non_reactive() + 1);
    }

    // --- The app's calls -------------------------------------------------------------------

    pub(crate) fn capabilities(&self) -> AnchorCapabilities {
        self.observe();
        self.capabilities.get()
    }

    pub(crate) fn planes_availability(&self) -> TrackingAvailability {
        self.observe();
        self.planes_availability.get()
    }

    pub(crate) fn persisted_uuids(&self) -> Vec<Uuid> {
        self.observe();
        self.persisted.borrow().clone()
    }

    pub(crate) fn create_anchor(&self, pose: Pose) -> AnchorResult {
        self.create(AnchorOrigin::Space, pose)
    }

    pub(crate) fn create_anchor_on(&self, plane: PlaneKey, pose: Pose) -> AnchorResult {
        self.create(AnchorOrigin::Plane(plane), pose)
    }

    fn create(&self, origin: AnchorOrigin, pose: Pose) -> AnchorResult {
        let capabilities = self.capabilities.get();
        if capabilities.anchors != TrackingAvailability::Active {
            return AnchorResult::Unavailable;
        }
        if !self.head_tracked.get() {
            return AnchorResult::TrackingUnavailable;
        }
        let held = self
            .anchors
            .borrow()
            .iter()
            .filter(|anchor| !anchor.is_detached())
            .count();
        if capabilities
            .max_anchors
            .is_some_and(|most| held >= most as usize)
        {
            return AnchorResult::ResourcesExhausted;
        }
        AnchorResult::Success(self.add_anchor(origin, pose, AnchorPersistence::NotPersisted))
    }

    pub(crate) fn load(&self, uuid: Uuid) -> AnchorResult {
        if self.capabilities.get().persistence != TrackingAvailability::Active {
            return AnchorResult::Unavailable;
        }
        if !self.persisted.borrow().contains(&uuid) {
            return AnchorResult::InvalidUuid;
        }
        AnchorResult::Success(self.add_anchor(
            AnchorOrigin::Persisted(uuid),
            Pose::IDENTITY,
            AnchorPersistence::Persisted(uuid),
        ))
    }

    fn add_anchor(
        &self,
        origin: AnchorOrigin,
        pose: Pose,
        persistence: AnchorPersistence,
    ) -> Anchor {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let anchor = Anchor::new(id, origin, pose, persistence, Weak::clone(&self.me));
        self.anchors.borrow_mut().push(anchor.clone());
        anchor
    }

    pub(crate) fn persist(&self, anchor: &Anchor) {
        if anchor.is_detached() {
            return;
        }
        match anchor.raw_persistence() {
            AnchorPersistence::Persisting | AnchorPersistence::Persisted(_) => {}
            AnchorPersistence::NotPersisted | AnchorPersistence::Failed => {
                let next = if self.capabilities.get().persistence == TrackingAvailability::Active {
                    AnchorPersistence::Persisting
                } else {
                    AnchorPersistence::Failed
                };
                anchor.set_persistence(next);
                self.touch();
            }
        }
    }

    pub(crate) fn detach(&self, anchor: &Anchor) {
        if !anchor.is_detached() {
            anchor.mark_detached();
            self.touch();
        }
    }

    pub(crate) fn unpersist(&self, uuid: Uuid) {
        let mut listed = self.persisted.borrow_mut();
        if let Some(at) = listed.iter().position(|known| *known == uuid) {
            listed.remove(at);
            self.unpersisting.borrow_mut().push(uuid);
            drop(listed);
            self.touch();
        }
    }

    pub(crate) fn plane_with_key(&self, key: PlaneKey) -> Option<Plane> {
        self.planes
            .borrow()
            .iter()
            .find(|plane| plane.key() == key)
            .cloned()
    }

    // The planes that the runtime tracks and that no other plane subsumes.
    pub(crate) fn tracked_planes(&self) -> Vec<Plane> {
        self.observe();
        self.planes
            .borrow()
            .iter()
            .filter(|plane| plane.is_tracking() && !plane.is_subsumed())
            .cloned()
            .collect()
    }

    // Where a movable composable of `size` meters (width, height, depth) at `pose` comes to
    // rest on a plane that `filter` admits, or `None` when no plane is near enough. A
    // frame's carry calls it and allocates nothing.
    #[cfg(any(test, headset))]
    pub(crate) fn snap(&self, filter: PlaneFilter, pose: &Pose, size: [f32; 3]) -> Option<Pose> {
        let content = Content { pose: *pose, size };
        let mut best: Option<(f32, Pose)> = None;
        for plane in self.planes.borrow().iter() {
            let state = plane.current();
            if !plane.is_tracking()
                || plane.is_subsumed()
                || !filter.admits(plane.plane_type(), state.label)
            {
                continue;
            }
            let vertices = plane.vertices();
            let surface = Surface {
                center: state.center_pose,
                extents: state.extents,
                vertices: &vertices,
                plane_type: plane.plane_type(),
            };
            if let Some((gap, snapped)) = snap(&surface, &content)
                && best.is_none_or(|(nearest, _)| gap < nearest)
            {
                best = Some((gap, snapped));
            }
        }
        best.map(|(_, snapped)| snapped)
    }

    // --- The backend's calls ---------------------------------------------------------------

    pub(crate) fn set_capabilities(&self, capabilities: AnchorCapabilities) {
        if self.capabilities.get() != capabilities {
            self.capabilities.set(capabilities);
            self.touch();
        }
    }

    // Changes one part of the capabilities.
    pub(crate) fn update_capabilities(&self, change: impl FnOnce(&mut AnchorCapabilities)) {
        let mut capabilities = self.capabilities.get();
        change(&mut capabilities);
        self.set_capabilities(capabilities);
    }

    pub(crate) fn set_planes_availability(&self, availability: TrackingAvailability) {
        if self.planes_availability.get() != availability {
            self.planes_availability.set(availability);
            self.touch();
        }
    }

    // Tells the store whether the runtime tracks the head. A runtime that does not cannot place
    // an anchor.
    pub(crate) fn set_head_tracked(&self, tracked: bool) {
        self.head_tracked.set(tracked);
    }

    // The anchors that the app holds and the runtime has not ended. The backend walks them by
    // index, because a visit by closure would hold the list while the backend calls the store.
    pub(crate) fn anchor_count(&self) -> usize {
        self.anchors.borrow().len()
    }

    pub(crate) fn anchor_at(&self, index: usize) -> Option<Anchor> {
        self.anchors.borrow().get(index).cloned()
    }

    // The anchor with this id, if the app still holds it.
    pub(crate) fn anchor_with_id(&self, id: u64) -> Option<Anchor> {
        self.anchors
            .borrow()
            .iter()
            .find(|anchor| anchor.id() == id)
            .cloned()
    }

    // Lets go of the anchors that the app detached and those that no handle holds. Their runtime
    // resources end with them.
    pub(crate) fn sweep(&self) {
        self.anchors
            .borrow_mut()
            .retain(|anchor| !anchor.is_detached() && !anchor.is_unreferenced());
    }

    // Moves the requests to unpersist into `out`, which keeps its storage from frame to frame.
    pub(crate) fn take_unpersists(&self, out: &mut Vec<Uuid>) {
        out.append(&mut self.unpersisting.borrow_mut());
    }

    // Replaces the list of the identifiers that the runtime keeps.
    pub(crate) fn set_persisted(&self, uuids: &[Uuid]) {
        let mut listed = self.persisted.borrow_mut();
        if listed.as_slice() != uuids {
            listed.clear();
            listed.extend_from_slice(uuids);
            drop(listed);
            self.touch();
        }
    }

    // The runtime answered a request to persist `anchor`: with the identifier it keeps the
    // anchor under, or with a refusal.
    pub(crate) fn finish_persist(&self, anchor: &Anchor, uuid: Option<Uuid>) {
        match uuid {
            Some(uuid) => {
                anchor.set_persistence(AnchorPersistence::Persisted(uuid));
                let mut listed = self.persisted.borrow_mut();
                if !listed.contains(&uuid) {
                    listed.push(uuid);
                }
            }
            None => anchor.set_persistence(AnchorPersistence::Failed),
        }
        self.touch();
    }

    // Takes what the runtime says of an anchor in a frame.
    pub(crate) fn publish_anchor(
        &self,
        anchor: &Anchor,
        pose: Option<Pose>,
        tracking: TrackingState,
    ) {
        if anchor.publish(pose, tracking) {
            self.touch();
        }
    }

    pub(crate) fn plane_count(&self) -> usize {
        self.planes.borrow().len()
    }

    pub(crate) fn plane_at(&self, index: usize) -> Option<Plane> {
        self.planes.borrow().get(index).cloned()
    }

    pub(crate) fn add_plane(&self, plane: Plane) {
        self.planes.borrow_mut().push(plane);
        self.touch();
    }

    // Removes the planes that `keep` refuses. Returns whether it removed any.
    pub(crate) fn retain_planes(&self, keep: impl FnMut(&Plane) -> bool) -> bool {
        let mut planes = self.planes.borrow_mut();
        let before = planes.len();
        planes.retain(keep);
        let removed = planes.len() != before;
        drop(planes);
        if removed {
            self.touch();
        }
        removed
    }

    // Records a change that a composition can observe on a plane: its label, its direction, its
    // tracking state or whether another plane subsumed it.
    pub(crate) fn plane_changed(&self) {
        self.touch();
    }

    pub(crate) fn weak(&self) -> Weak<Self> {
        Weak::clone(&self.me)
    }
}

#[cfg(test)]
#[path = "tests/spatial_store_tests.rs"]
mod tests;
