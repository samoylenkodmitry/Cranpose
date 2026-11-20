//! The planes of the room that the app can read.

use std::{fmt, rc::Rc};

use cranpose_core::CompositionLocal;

use crate::{Plane, TrackingAvailability, locals::with_locals, spatial_store::SpatialStore};

/// The planes of the room: its walls, floor, ceiling and tables. An app reads it through
/// [`local_planes`]. In Jetpack XR this is `Plane.subscribe`, which takes the `Session`, and the
/// session's `Config.planeTracking`.
///
/// An app declares `Use::plane_tracking()` of `cranpose::capabilities` in its build, and the user
/// allows it with the system's prompt. A Meta Quest gives the planes that the user's room setup
/// scanned, through `XR_FB_scene`, and never starts a scan. Android XR gives the planes that
/// its cameras found, through `XR_ANDROID_trackables`.
#[derive(Clone)]
pub struct Planes {
    store: Rc<SpatialStore>,
}

impl fmt::Debug for Planes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Planes")
            .field("availability", &self.store.planes_availability())
            .finish()
    }
}

impl PartialEq for Planes {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.store, &other.store)
    }
}

impl Planes {
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend provides it")
    )]
    pub(crate) fn new(store: Rc<SpatialStore>) -> Self {
        Self { store }
    }

    /// The planes that the runtime tracks now, except the ones that another plane subsumed. The
    /// list is empty until the user allows the access and the runtime has found planes. A room that
    /// the user has not set up gives none on a Meta Quest. A composition that reads it composes
    /// again when a plane is added or removed, or when it changes its label or its tracking state.
    /// Jetpack XR's `Plane.subscribe`, whose flow holds the same planes.
    pub fn all(&self) -> Vec<Plane> {
        self.store.tracked_planes()
    }

    /// How far the plane tracking has come: [`Active`](TrackingAvailability::Active) once the
    /// runtime gives planes, [`AwaitingPermission`](TrackingAvailability::AwaitingPermission) while the
    /// user has not allowed it. A composition that reads it composes again when it changes.
    pub fn availability(&self) -> TrackingAvailability {
        self.store.planes_availability()
    }
}

/// The composition local that holds the app's [`Planes`], or `None` outside a headset.
///
/// ```ignore
/// if let Some(planes) = local_planes().current() {
///     for plane in planes.all() {
///         Text(format!("{:?}", plane.state().label), Modifier::empty(), TextStyle::default());
///     }
/// }
/// ```
pub fn local_planes() -> CompositionLocal<Option<Planes>> {
    with_locals(|locals| locals.planes.clone())
}
