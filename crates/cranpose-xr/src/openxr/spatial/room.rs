//! The room of a session: its anchors and planes, made when the user's permissions allow and read
//! in every frame.

use std::time::Duration;

use cranpose::capabilities::Service;
use openxr as xr;

use super::{
    super::{host::Host, session::XrSession, tracking::step_of_service},
    frame::Frame,
    meta_anchors::MetaAnchors,
    meta_planes::MetaPlanes,
};
#[cfg(target_os = "android")]
use super::{android_anchors::AndroidAnchors, android_planes::AndroidPlanes};
use crate::{
    TrackingAvailability, spatial_offer::SpatialProvider, spatial_setup::SpatialSetup,
    spatial_store::SpatialStore, tracking_permission::PermissionStep,
};

// How long the backend waits between two looks at a pending feature: its permission, and a new try
// to make its handle. A frame that falls between does nothing for it.
const POLL: Duration = Duration::from_secs(1);

// The runtime's way to hold the room, as the extensions of its family give it.
enum Backend {
    // The app asked for nothing, or the runtime offers nothing
    None,
    Meta {
        anchors: Box<MetaAnchors>,
        planes: Box<MetaPlanes>,
    },
    #[cfg(target_os = "android")]
    Android {
        anchors: Box<AndroidAnchors>,
        planes: Box<AndroidPlanes>,
    },
}

impl Backend {
    fn sample(&mut self, frame: &Frame<'_>) {
        match self {
            Self::None => {}
            Self::Meta { anchors, planes } => {
                anchors.sample(frame);
                planes.sample(frame);
            }
            #[cfg(target_os = "android")]
            Self::Android { anchors, planes } => {
                anchors.sample(frame);
                planes.sample(frame);
            }
        }
    }

    // Whether the plane tracking runs.
    fn planes_run(&self) -> bool {
        match self {
            Self::None => false,
            Self::Meta { planes, .. } => planes.is_running(),
            #[cfg(target_os = "android")]
            Self::Android { planes, .. } => planes.is_running(),
        }
    }

    // Starts the plane tracking, which the runtime refuses until the user grants the permission.
    fn start_planes(&mut self) -> Result<(), xr::sys::Result> {
        match self {
            Self::None => Err(xr::sys::Result::ERROR_FEATURE_UNSUPPORTED),
            Self::Meta { planes, .. } => {
                planes.start();
                Ok(())
            }
            #[cfg(target_os = "android")]
            Self::Android { planes, .. } => planes.start(),
        }
    }
}

// The anchors and planes of a session. An app that asked for none has no backend, and `sample`
// only keeps the store's bookkeeping.
pub(crate) struct Room {
    setup: SpatialSetup,
    backend: Backend,
    last_poll: Option<Duration>,
}

impl Room {
    pub(crate) fn new(setup: SpatialSetup, xr_session: &XrSession) -> Self {
        let (instance, session) = (xr_session.instance(), &xr_session.session);
        let backend = match setup.provider.filter(|_| setup.request.asks_any()) {
            Some(SpatialProvider::Meta) => Backend::Meta {
                anchors: Box::new(MetaAnchors::new(instance, session, setup.enabled)),
                planes: Box::new(MetaPlanes::new(instance, session)),
            },
            #[cfg(target_os = "android")]
            Some(SpatialProvider::Android) => Backend::Android {
                anchors: Box::new(AndroidAnchors::new(instance, session)),
                planes: Box::new(AndroidPlanes::new(instance, session)),
            },
            #[cfg(not(target_os = "android"))]
            Some(SpatialProvider::Android) => Backend::None,
            None => Backend::None,
        };
        Self {
            setup,
            backend,
            last_poll: None,
        }
    }

    // One frame: tells the store whether the head is tracked, lets go of the anchors that the app
    // dropped, looks at the pending permissions once a second and reads what the runtime holds.
    pub(crate) fn sample(&mut self, frame: &Frame<'_>) {
        frame.store.set_head_tracked(frame.head_tracked);
        frame.store.sweep();
        if matches!(self.backend, Backend::None) {
            return;
        }
        let due = self
            .last_poll
            .is_none_or(|last| frame.now.saturating_sub(last) >= POLL);
        if due {
            self.last_poll = Some(frame.now);
            self.advance(frame.host, frame.store);
        }
        self.backend.sample(frame);
    }

    // One look at the features that wait for a permission: makes the handle of each whose
    // permission is granted, and stops waiting for each that the user refused.
    fn advance(&mut self, host: &Host, store: &SpatialStore) {
        #[cfg(target_os = "android")]
        self.advance_persistence(host, store);
        self.advance_planes(host, store);
    }

    // Android XR's storage of anchors waits for the permission of scene understanding.
    #[cfg(target_os = "android")]
    fn advance_persistence(&mut self, host: &Host, store: &SpatialStore) {
        let Backend::Android { anchors, .. } = &mut self.backend else {
            return;
        };
        if self.setup.anchors.persistence != TrackingAvailability::AwaitingPermission
            || anchors.has_persistence()
        {
            return;
        }
        let level = match step_of_service(host, Service::Anchors) {
            PermissionStep::Wait => return,
            PermissionStep::Refused => {
                log::info!("cranpose xr: anchor persistence: the user refused the permission");
                TrackingAvailability::PermissionRefused
            }
            PermissionStep::Ready => match anchors.start_persistence() {
                Ok(()) => {
                    log::info!("cranpose xr: anchor persistence runs");
                    return;
                }
                // The permission is granted and the runtime still refuses: a new try would only
                // repeat the refusal
                Err(error) if error != xr::sys::Result::ERROR_PERMISSION_INSUFFICIENT => {
                    log::warn!("cranpose xr: anchor persistence is not available: {error}");
                    TrackingAvailability::Unavailable
                }
                Err(error) => {
                    log::debug!("cranpose xr: anchor persistence: {error}");
                    return;
                }
            },
        };
        self.setup.anchors.persistence = level;
        store.update_capabilities(|capabilities| capabilities.persistence = level);
    }

    // The planes of both runtimes wait for the user's permission to read the room.
    fn advance_planes(&mut self, host: &Host, store: &SpatialStore) {
        if self.setup.planes != TrackingAvailability::AwaitingPermission
            || self.backend.planes_run()
        {
            return;
        }
        let level = match step_of_service(host, Service::PlaneTracking) {
            PermissionStep::Wait => return,
            PermissionStep::Refused => {
                log::info!("cranpose xr: plane tracking: the user refused the permission");
                TrackingAvailability::PermissionRefused
            }
            PermissionStep::Ready => match self.backend.start_planes() {
                Ok(()) => {
                    log::info!("cranpose xr: plane tracking runs");
                    TrackingAvailability::Active
                }
                Err(error) if error != xr::sys::Result::ERROR_PERMISSION_INSUFFICIENT => {
                    log::warn!("cranpose xr: plane tracking is not available: {error}");
                    TrackingAvailability::Unavailable
                }
                Err(error) => {
                    log::debug!("cranpose xr: plane tracking: {error}");
                    return;
                }
            },
        };
        self.setup.planes = level;
        store.set_planes_availability(level);
    }
}
