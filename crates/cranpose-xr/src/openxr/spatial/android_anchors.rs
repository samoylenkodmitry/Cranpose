//! Anchors on Android XR: `XR_ANDROID_trackables` holds them and `XR_ANDROID_device_anchor_persistence`
//! keeps them.
#![expect(
    unsafe_code,
    reason = "XR_ANDROID_trackables and XR_ANDROID_device_anchor_persistence, which the openxr binding wraps in no safe call"
)]

use std::{cell::Cell, ptr};

use openxr as xr;
use xr::sys::Handle;

use super::{
    frame::Frame,
    locate::locate_anchor,
    uuid_wire::{uuid_of, wire_of},
};
use crate::{
    Anchor, TrackingAvailability, Uuid,
    anchor::AnchorOrigin,
    openxr::{graphics::Api, session::posef},
};

// What an anchor of Android XR holds in the runtime: its space, and the identifier under which the
// runtime was asked to keep it, until the runtime says it did.
struct AnchorSpace {
    space: xr::Space,
    persisting: Cell<Option<Uuid>>,
}

// The runtime's handle for the app's persisted anchors, and whether it has read its storage.
struct Persistence {
    handle: xr::sys::DeviceAnchorPersistenceANDROID,
    ready: bool,
}

pub(crate) struct AndroidAnchors {
    instance: xr::Instance,
    session: xr::Session<Api>,
    persistence: Option<Persistence>,
    // Reused each time the runtime lists its storage
    wire: Vec<xr::sys::Uuid>,
    listed: Vec<Uuid>,
    unpersist: Vec<Uuid>,
}

impl AndroidAnchors {
    pub(crate) fn new(instance: &xr::Instance, session: &xr::Session<Api>) -> Self {
        Self {
            instance: instance.clone(),
            session: session.clone(),
            persistence: None,
            wire: Vec::new(),
            listed: Vec::new(),
            unpersist: Vec::new(),
        }
    }

    // Makes the runtime's persistence handle, which starts to read the app's storage. The runtime
    // refuses it until the user grants the permission. Returns the runtime's answer.
    pub(crate) fn start_persistence(&mut self) -> Result<(), xr::sys::Result> {
        let Some(fp) = self
            .instance
            .exts()
            .android_device_anchor_persistence
            .as_ref()
        else {
            return Err(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT);
        };
        let info = xr::sys::DeviceAnchorPersistenceCreateInfoANDROID {
            ty: xr::sys::DeviceAnchorPersistenceCreateInfoANDROID::TYPE,
            next: ptr::null(),
        };
        let mut handle = xr::sys::DeviceAnchorPersistenceANDROID::NULL;
        // SAFETY: the session handle is live, and `info` is initialized with its type.
        let result = unsafe {
            (fp.create_device_anchor_persistence)(self.session.as_raw(), &info, &mut handle)
        };
        if result.into_raw() < 0 {
            return Err(result);
        }
        self.persistence = Some(Persistence {
            handle,
            ready: false,
        });
        Ok(())
    }

    pub(crate) fn has_persistence(&self) -> bool {
        self.persistence.is_some()
    }

    // One frame: reads the storage once it is ready, takes the app's requests, makes the
    // anchors that the app asked for and reads their poses.
    pub(crate) fn sample(&mut self, frame: &Frame<'_>) {
        self.read_storage(frame);
        self.erase(frame);
        for index in 0..frame.store.anchor_count() {
            let Some(anchor) = frame.store.anchor_at(index) else {
                break;
            };
            if !anchor.is_attached() && !self.make(&anchor, frame) {
                continue;
            }
            let located = anchor.with_resource(|held: &AnchorSpace| {
                locate_anchor(&held.space, &frame.xr_session.local, frame.time)
            });
            if let Some((pose, tracking)) = located {
                frame.store.publish_anchor(&anchor, pose, tracking);
            }
            self.persist(&anchor, frame);
        }
    }

    // Lists the storage when the runtime has read it, until the first list comes.
    fn read_storage(&mut self, frame: &Frame<'_>) {
        let Some(persistence) = &mut self.persistence else {
            return;
        };
        if persistence.ready {
            return;
        }
        if list(&self.instance, persistence.handle, &mut self.wire).is_err() {
            return;
        }
        persistence.ready = true;
        self.publish_list(frame);
        frame.store.update_capabilities(|capabilities| {
            capabilities.persistence = TrackingAvailability::Active;
        });
        log::info!(
            "cranpose xr: anchor persistence ready, the runtime keeps {} anchors: {:?}",
            self.listed.len(),
            self.listed
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
    }

    // Lists the storage again and gives the list to the app.
    fn refresh(&mut self, frame: &Frame<'_>) {
        let Some(persistence) = self.persistence.as_ref().filter(|p| p.ready) else {
            return;
        };
        if list(&self.instance, persistence.handle, &mut self.wire).is_ok() {
            self.publish_list(frame);
        }
    }

    fn publish_list(&mut self, frame: &Frame<'_>) {
        self.listed.clear();
        self.listed.extend(self.wire.iter().map(uuid_of));
        frame.store.set_persisted(&self.listed);
    }

    // Deletes the anchors that the app asked to unpersist.
    fn erase(&mut self, frame: &Frame<'_>) {
        let Some(persistence) = self.persistence.as_ref().filter(|p| p.ready) else {
            return;
        };
        frame.store.take_unpersists(&mut self.unpersist);
        if self.unpersist.is_empty() {
            return;
        }
        let handle = persistence.handle;
        let Some(fp) = self
            .instance
            .exts()
            .android_device_anchor_persistence
            .as_ref()
        else {
            return;
        };
        for uuid in self.unpersist.drain(..) {
            let wire = wire_of(uuid);
            // SAFETY: the handle is live, and `wire` is a value that outlives the call.
            let result = unsafe { (fp.unpersist_anchor)(handle, &wire) };
            log::info!("cranpose xr: unpersist {uuid}: {result:?}");
        }
        self.refresh(frame);
    }

    // Makes the runtime's anchor for `anchor`. Returns whether the anchor has its space now.
    fn make(&mut self, anchor: &Anchor, frame: &Frame<'_>) -> bool {
        let made = match anchor.origin() {
            AnchorOrigin::Space => self.create(anchor, frame, 0),
            AnchorOrigin::Plane(key) => self.create(anchor, frame, key.0 as u64),
            AnchorOrigin::Persisted(uuid) => {
                if !self.persistence.as_ref().is_some_and(|p| p.ready) {
                    return false;
                }
                self.load(uuid)
            }
        };
        match made {
            Ok(space) => {
                // SAFETY: the runtime made the space for this session, and nothing else owns it.
                let space = unsafe { xr::Space::reference_from_raw(self.session.clone(), space) };
                anchor.attach(AnchorSpace {
                    space,
                    persisting: Cell::new(None),
                });
                true
            }
            Err(result) => {
                log::warn!(
                    "cranpose xr: anchor {} is not made: {result:?}",
                    anchor.id()
                );
                frame.store.detach(anchor);
                false
            }
        }
    }

    fn create(
        &self,
        anchor: &Anchor,
        frame: &Frame<'_>,
        trackable: u64,
    ) -> Result<xr::sys::Space, xr::sys::Result> {
        let fp = self
            .instance
            .exts()
            .android_trackables
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let info = xr::sys::AnchorSpaceCreateInfoANDROID {
            ty: xr::sys::AnchorSpaceCreateInfoANDROID::TYPE,
            next: ptr::null(),
            space: frame.xr_session.local.as_raw(),
            time: frame.time,
            pose: posef(&anchor.pose()),
            trackable: xr::sys::TrackableANDROID::from_raw(trackable),
        };
        let mut space = xr::sys::Space::NULL;
        // SAFETY: the session and the base space are live, and `info` is initialized with its type.
        let result = unsafe { (fp.create_anchor_space)(self.session.as_raw(), &info, &mut space) };
        if result.into_raw() < 0 {
            Err(result)
        } else {
            Ok(space)
        }
    }

    fn load(&self, uuid: Uuid) -> Result<xr::sys::Space, xr::sys::Result> {
        let persistence = self
            .persistence
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_FEATURE_UNSUPPORTED)?;
        let fp = self
            .instance
            .exts()
            .android_device_anchor_persistence
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let info = xr::sys::PersistedAnchorSpaceCreateInfoANDROID {
            ty: xr::sys::PersistedAnchorSpaceCreateInfoANDROID::TYPE,
            next: ptr::null(),
            anchor_id: wire_of(uuid),
        };
        let mut space = xr::sys::Space::NULL;
        // SAFETY: the persistence handle is live, and `info` is initialized with its type.
        let result =
            unsafe { (fp.create_persisted_anchor_space)(persistence.handle, &info, &mut space) };
        if result.into_raw() < 0 {
            Err(result)
        } else {
            Ok(space)
        }
    }

    // Passes the app's request to keep `anchor` on to the runtime, and reads the runtime's answer.
    fn persist(&mut self, anchor: &Anchor, frame: &Frame<'_>) {
        let Some(persistence) = self.persistence.as_ref().filter(|p| p.ready) else {
            return;
        };
        let Some(fp) = self
            .instance
            .exts()
            .android_device_anchor_persistence
            .as_ref()
        else {
            return;
        };
        let handle = persistence.handle;
        if anchor.needs_persist() {
            let asked = anchor.with_resource(|held: &AnchorSpace| {
                let info = xr::sys::PersistedAnchorSpaceInfoANDROID {
                    ty: xr::sys::PersistedAnchorSpaceInfoANDROID::TYPE,
                    next: ptr::null(),
                    anchor: held.space.as_raw(),
                };
                let mut wire = xr::sys::Uuid::default();
                // SAFETY: the handle and the anchor's space are live, and `info` is initialized
                // with its type.
                let result = unsafe { (fp.persist_anchor)(handle, &info, &mut wire) };
                (result, uuid_of(&wire))
            });
            match asked {
                // The runtime wants a moment more: the data is not ready or the anchor does not
                // track yet. The next frame asks again.
                Some((result, _))
                    if result == xr::sys::Result::ERROR_PERSISTED_DATA_NOT_READY_ANDROID
                        || result == xr::sys::Result::ERROR_ANCHOR_NOT_TRACKING_ANDROID => {}
                Some((result, uuid)) if result.into_raw() >= 0 => {
                    anchor.mark_persist_sent();
                    anchor.with_resource(|held: &AnchorSpace| held.persisting.set(Some(uuid)));
                }
                Some((result, _)) => {
                    log::warn!(
                        "cranpose xr: anchor {} persists no: {result:?}",
                        anchor.id()
                    );
                    anchor.mark_persist_sent();
                    frame.store.finish_persist(anchor, None);
                }
                None => {}
            }
        }
        let pending = anchor
            .with_resource(|held: &AnchorSpace| held.persisting.get())
            .flatten();
        if let Some(uuid) = pending {
            self.poll_persisted(anchor, uuid, frame);
        }
    }

    fn poll_persisted(&mut self, anchor: &Anchor, uuid: Uuid, frame: &Frame<'_>) {
        let Some(persistence) = self.persistence.as_ref() else {
            return;
        };
        let Some(fp) = self
            .instance
            .exts()
            .android_device_anchor_persistence
            .as_ref()
        else {
            return;
        };
        let wire = wire_of(uuid);
        let mut state = xr::sys::AnchorPersistStateANDROID::PERSIST_NOT_REQUESTED;
        // SAFETY: the handle is live, and the pointers name locals that outlive the call.
        let result =
            unsafe { (fp.get_anchor_persist_state)(persistence.handle, &wire, &mut state) };
        if result.into_raw() < 0 {
            return;
        }
        if state == xr::sys::AnchorPersistStateANDROID::PERSISTED {
            anchor.with_resource(|held: &AnchorSpace| held.persisting.set(None));
            frame.store.finish_persist(anchor, Some(uuid));
            log::info!(
                "cranpose xr: anchor {} persisted as {uuid} at {:?}",
                anchor.id(),
                anchor.pose()
            );
            self.refresh(frame);
        }
    }
}

impl Drop for AndroidAnchors {
    fn drop(&mut self) {
        let Some(persistence) = self.persistence.take() else {
            return;
        };
        if let Some(fp) = self
            .instance
            .exts()
            .android_device_anchor_persistence
            .as_ref()
        {
            // SAFETY: the handle is live and nothing uses it after this call.
            unsafe { (fp.destroy_device_anchor_persistence)(persistence.handle) };
        }
    }
}

// The identifiers that the runtime keeps for the app: the two-call idiom into `out`, which keeps its
// storage. The runtime answers PERSISTED_DATA_NOT_READY while it reads the storage.
fn list(
    instance: &xr::Instance,
    handle: xr::sys::DeviceAnchorPersistenceANDROID,
    out: &mut Vec<xr::sys::Uuid>,
) -> Result<(), xr::sys::Result> {
    let fp = instance
        .exts()
        .android_device_anchor_persistence
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let mut count = 0_u32;
    // SAFETY: the handle is live. A capacity of zero asks for the count and reads no array.
    let result =
        unsafe { (fp.enumerate_persisted_anchors)(handle, 0, &mut count, ptr::null_mut()) };
    if result.into_raw() < 0 {
        return Err(result);
    }
    out.clear();
    out.resize(count as usize, xr::sys::Uuid::default());
    let capacity = count;
    // SAFETY: `out` holds `capacity` identifiers, which the runtime fills.
    let result =
        unsafe { (fp.enumerate_persisted_anchors)(handle, capacity, &mut count, out.as_mut_ptr()) };
    if result.into_raw() < 0 {
        out.clear();
        return Err(result);
    }
    out.truncate(count as usize);
    Ok(())
}
