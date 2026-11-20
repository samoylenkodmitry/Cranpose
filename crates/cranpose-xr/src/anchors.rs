//! The app's way to create, keep and load anchors.

use std::{fmt, rc::Rc};

use cranpose_core::CompositionLocal;

use crate::{
    AnchorCapabilities, AnchorResult, Pose, Uuid, locals::with_locals, spatial_store::SpatialStore,
};

/// What an app does with anchors: create them, keep them for its next start and load them back.
/// An app reads it through [`local_anchors`]. In Jetpack XR these are the companion functions of
/// `Anchor`, which take the `Session`, and the session's `Config.anchorPersistence`.
///
/// An app declares `Use::anchors()` of `cranpose::capabilities` in its build. Until it does, the
/// backend enables no extension and every call here answers [`AnchorResult::Unavailable`].
/// [`capabilities`](Self::capabilities) says what the runtime offers.
///
/// A Meta Quest holds anchors through `XR_FB_spatial_entity` and keeps them with
/// `XR_FB_spatial_entity_storage`. Android XR holds them with `XR_ANDROID_trackables` and keeps them
/// with `XR_ANDROID_device_anchor_persistence`.
#[derive(Clone)]
pub struct Anchors {
    store: Rc<SpatialStore>,
}

impl fmt::Debug for Anchors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anchors")
            .field("capabilities", &self.store.capabilities())
            .finish()
    }
}

impl PartialEq for Anchors {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.store, &other.store)
    }
}

impl Anchors {
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend provides it")
    )]
    pub(crate) fn new(store: Rc<SpatialStore>) -> Self {
        Self { store }
    }

    /// Creates an anchor at `pose` in the app's room, the OpenXR LOCAL space that
    /// [`XrFrame`](crate::XrFrame) describes. The runtime holds the anchor from the next frame
    /// on. Create it from an event handler or an effect, not while a composable runs. Jetpack XR's
    /// `Anchor.create`.
    pub fn create(&self, pose: Pose) -> AnchorResult {
        self.store.create_anchor(pose)
    }

    /// Loads the anchor that the runtime kept under `uuid`. It stands where it was when the app
    /// persisted it, relative to the room, and the runtime places it once it recognizes the room,
    /// which takes some frames. Until then its state is [`Paused`](crate::TrackingState::Paused)
    /// and content that follows it stays hidden. Jetpack XR's `Anchor.load`.
    pub fn load(&self, uuid: Uuid) -> AnchorResult {
        self.store.load(uuid)
    }

    /// The identifiers of the anchors that the runtime keeps for this app. The list is empty until
    /// the runtime has read its storage, which takes some frames after the start.
    /// [`capabilities`](Self::capabilities) shows when
    /// [`persistence`](AnchorCapabilities::persistence) is
    /// [`Active`](crate::TrackingAvailability::Active). A composition that reads it composes again
    /// when the list changes. Jetpack XR's `Anchor.getPersistedAnchorUuids`.
    pub fn persisted_uuids(&self) -> Vec<Uuid> {
        self.store.persisted_uuids()
    }

    /// Deletes the anchor that the runtime kept under `uuid`. An anchor that the app loaded from
    /// it keeps tracking, as in Jetpack XR. A `uuid` that is not in the list changes nothing.
    /// Jetpack XR's `Anchor.unpersist`.
    pub fn unpersist(&self, uuid: Uuid) {
        self.store.unpersist(uuid);
    }

    /// What the runtime offers of anchors, and how far each part has come. A composition that reads
    /// it composes again when a part changes.
    pub fn capabilities(&self) -> AnchorCapabilities {
        self.store.capabilities()
    }
}

/// The composition local that holds the app's [`Anchors`], or `None` outside a headset.
///
/// ```ignore
/// if let Some(anchors) = local_anchors().current() {
///     // Put an anchor a meter ahead of where the app started and keep it
///     if let Some(anchor) = anchors.create(Pose::at([0.0, 0.0, -1.0])).into_anchor() {
///         anchor.persist();
///     }
/// }
/// ```
pub fn local_anchors() -> CompositionLocal<Option<Anchors>> {
    with_locals(|locals| locals.anchors.clone())
}

#[cfg(test)]
#[path = "tests/anchors_tests.rs"]
mod tests;
