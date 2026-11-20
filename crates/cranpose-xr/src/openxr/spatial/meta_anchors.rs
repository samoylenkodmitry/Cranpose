//! Anchors on a Meta Quest: `XR_FB_spatial_entity` holds them, and `XR_FB_spatial_entity_storage`
//! with `XR_FB_spatial_entity_query` keeps them and loads them back.
//!
//! Every step that touches the runtime's storage is a request that an event answers: the creation of
//! an anchor, the enabling of a component, a save, a query and an erase. A table of pending requests
//! says what each answer finishes.

use std::cell::{Cell, OnceCell};

use openxr as xr;

use super::{
    events::SpatialEvent,
    frame::Frame,
    locate::locate_anchor,
    meta_calls::{self, Filter},
    uuid_wire::uuid_of,
};
use crate::{
    Anchor, TrackingAvailability, Uuid,
    anchor::AnchorOrigin,
    openxr::{graphics::Api, session::posef},
    spatial_offer::SpatialOffer,
};

// How many saved anchors a listing reads, and the most that a query returns at once.
const LIST_LIMIT: u32 = 64;

// Where an anchor stands in the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    // The runtime makes the space: the app asked for a new anchor, or for a saved one
    Making,
    // The space exists and the runtime locates it
    Ready,
    // The runtime enables the component that lets the anchor be saved
    Storing,
    // The runtime saves the anchor
    Saving,
}

// What an anchor of Meta's runtime holds: its space and where it stands.
struct AnchorSpace {
    space: OnceCell<xr::Space>,
    phase: Cell<Phase>,
}

// What a request waits for.
enum Pending {
    // The anchor with this id comes from the runtime
    Create(u64),
    // The anchor with this id enables the component of storage, and then saves
    Storable(u64),
    // The anchor with this id is saved
    Save(u64),
    // The listing of the storage
    List,
    // The anchor with this id comes from the storage
    Load(u64),
    // The anchor with this id enables its location component after it came from the storage
    Locatable(u64),
    // An entity that the app wants erased is found, and then erased
    FindToErase,
    // The entity is erased, and its space lives until the runtime says so
    Erase(xr::Space),
}

pub(crate) struct MetaAnchors {
    instance: xr::Instance,
    session: xr::Session<Api>,
    pending: Vec<(u64, Pending)>,
    // Reused for each query's results and each request to unpersist
    results: Vec<xr::sys::SpaceQueryResultFB>,
    unpersist: Vec<Uuid>,
    listed: Vec<Uuid>,
    // Whether the first listing has come
    ready: bool,
    // Whether the runtime refused storage: the app cannot persist
    refused: bool,
}

impl MetaAnchors {
    pub(crate) fn new(
        instance: &xr::Instance,
        session: &xr::Session<Api>,
        enabled: SpatialOffer,
    ) -> Self {
        Self {
            instance: instance.clone(),
            session: session.clone(),
            pending: Vec::new(),
            results: Vec::new(),
            unpersist: Vec::new(),
            listed: Vec::new(),
            ready: !enabled.fb_spatial_entity_storage,
            refused: !enabled.fb_spatial_entity_storage,
        }
    }

    // One frame: takes the runtime's answers, starts the first listing, takes the app's requests,
    // makes the anchors that the app asked for and reads their poses.
    pub(crate) fn sample(&mut self, frame: &Frame<'_>) {
        for event in frame.events {
            self.on_event(*event, frame);
        }
        if !self.ready && !self.refused && !self.is_pending_list() {
            self.start_list(frame);
        }
        self.erase(frame);
        for index in 0..frame.store.anchor_count() {
            let Some(anchor) = frame.store.anchor_at(index) else {
                break;
            };
            if !anchor.is_attached() {
                self.start(&anchor, frame);
                continue;
            }
            let located = anchor.with_resource(|held: &AnchorSpace| {
                held.space
                    .get()
                    .map(|space| locate_anchor(space, &frame.xr_session.local, frame.time))
            });
            if let Some(Some((pose, tracking))) = located {
                frame.store.publish_anchor(&anchor, pose, tracking);
            }
            self.persist(&anchor, frame);
        }
    }

    fn is_pending_list(&self) -> bool {
        self.pending
            .iter()
            .any(|(_, pending)| matches!(pending, Pending::List))
    }

    fn start_list(&mut self, frame: &Frame<'_>) {
        // The storage may also hold the entities of the room's scene, which have semantic labels
        // and are no anchors of the app
        match meta_calls::query(
            &self.instance,
            &self.session,
            Filter::Stored,
            Some(xr::sys::SpaceComponentTypeFB::SEMANTIC_LABELS),
            LIST_LIMIT,
        ) {
            Ok(request) => self.pending.push((request, Pending::List)),
            Err(error) => self.refuse(error, frame),
        }
    }

    // The runtime does not keep anchors for the app: it says so in the capabilities.
    fn refuse(&mut self, error: xr::sys::Result, frame: &Frame<'_>) {
        log::warn!("cranpose xr: anchor storage is not available: {error:?}");
        self.refused = true;
        frame.store.update_capabilities(|capabilities| {
            capabilities.persistence = TrackingAvailability::Unavailable;
        });
    }

    // Starts what the runtime needs to hold `anchor`: a new anchor, or the saved one.
    fn start(&mut self, anchor: &Anchor, frame: &Frame<'_>) {
        let started = match anchor.origin() {
            AnchorOrigin::Space | AnchorOrigin::Plane(_) => meta_calls::create_anchor(
                &self.instance,
                &self.session,
                &frame.xr_session.local,
                frame.time,
                posef(&anchor.pose()),
            )
            .map(|request| (request, Pending::Create(anchor.id()))),
            AnchorOrigin::Persisted(uuid) => {
                if !self.ready {
                    return;
                }
                meta_calls::query(&self.instance, &self.session, Filter::Uuid(uuid), None, 1)
                    .map(|request| (request, Pending::Load(anchor.id())))
            }
        };
        match started {
            Ok((request, pending)) => {
                anchor.attach(AnchorSpace {
                    space: OnceCell::new(),
                    phase: Cell::new(Phase::Making),
                });
                self.pending.push((request, pending));
            }
            Err(error) => {
                log::warn!("cranpose xr: anchor {} is not made: {error:?}", anchor.id());
                frame.store.detach(anchor);
            }
        }
    }

    // Passes the app's request to keep `anchor` on to the runtime: the anchor first enables its
    // storage component, and the answer starts the save.
    fn persist(&mut self, anchor: &Anchor, frame: &Frame<'_>) {
        if !anchor.needs_persist() {
            return;
        }
        let ready = anchor
            .with_resource(|held: &AnchorSpace| {
                (held.phase.get() == Phase::Ready)
                    .then(|| held.space.get().map(xr::Space::as_raw))
                    .flatten()
            })
            .flatten();
        let Some(space) = ready else {
            return;
        };
        anchor.mark_persist_sent();
        match meta_calls::enable_component(
            &self.instance,
            space,
            xr::sys::SpaceComponentTypeFB::STORABLE,
        ) {
            Ok(request) => {
                anchor.with_resource(|held: &AnchorSpace| held.phase.set(Phase::Storing));
                self.pending.push((request, Pending::Storable(anchor.id())));
            }
            Err(error) => {
                log::warn!(
                    "cranpose xr: anchor {} cannot be saved: {error:?}",
                    anchor.id()
                );
                frame.store.finish_persist(anchor, None);
            }
        }
    }

    // Takes the app's requests to delete saved anchors: finds each entity, then erases it.
    fn erase(&mut self, frame: &Frame<'_>) {
        if !self.ready {
            return;
        }
        frame.store.take_unpersists(&mut self.unpersist);
        for uuid in std::mem::take(&mut self.unpersist) {
            match meta_calls::query(&self.instance, &self.session, Filter::Uuid(uuid), None, 1) {
                Ok(request) => self.pending.push((request, Pending::FindToErase)),
                Err(error) => log::warn!("cranpose xr: unpersist {uuid}: {error:?}"),
            }
        }
    }

    // What `request` was started for, taken out of the table.
    fn take(&mut self, request: u64) -> Option<Pending> {
        let at = self.pending.iter().position(|(id, _)| *id == request)?;
        Some(self.pending.remove(at).1)
    }

    fn on_event(&mut self, event: SpatialEvent, frame: &Frame<'_>) {
        match event {
            SpatialEvent::AnchorCreated {
                request,
                result,
                space,
                uuid,
            } => {
                let Some(Pending::Create(id)) = self.take(request) else {
                    return;
                };
                let Some(anchor) = frame.store.anchor_with_id(id) else {
                    return;
                };
                if result.into_raw() < 0 {
                    log::warn!("cranpose xr: anchor {id} is not made: {result:?}");
                    frame.store.detach(&anchor);
                    return;
                }
                self.hold(&anchor, space);
                log::info!("cranpose xr: anchor {id} made, the runtime names it {uuid}");
            }
            SpatialEvent::StatusSet { request, result } => match self.take(request) {
                Some(Pending::Storable(id)) => self.on_storable(id, result, frame),
                Some(Pending::Locatable(id)) => self.on_locatable(id, result, frame),
                _ => {}
            },
            SpatialEvent::Saved {
                request,
                result,
                uuid,
            } => {
                let Some(Pending::Save(id)) = self.take(request) else {
                    return;
                };
                let Some(anchor) = frame.store.anchor_with_id(id) else {
                    return;
                };
                anchor.with_resource(|held: &AnchorSpace| held.phase.set(Phase::Ready));
                if result.into_raw() < 0 {
                    log::warn!("cranpose xr: anchor {id} is not saved: {result:?}");
                    frame.store.finish_persist(&anchor, None);
                } else {
                    frame.store.finish_persist(&anchor, Some(uuid));
                    log::info!(
                        "cranpose xr: anchor {id} saved as {uuid} at {:?}",
                        anchor.pose()
                    );
                    self.start_list(frame);
                }
            }
            SpatialEvent::QueryResults { request } => self.on_results(request, frame),
            SpatialEvent::QueryDone { request, result } => {
                if result.into_raw() < 0 {
                    log::warn!("cranpose xr: a query of the storage failed: {result:?}");
                }
                match self.take(request) {
                    // A query that ends with no results answers a load with nothing
                    Some(Pending::Load(id)) => {
                        if let Some(anchor) = frame.store.anchor_with_id(id) {
                            log::warn!("cranpose xr: anchor {id} is not in the storage");
                            frame.store.detach(&anchor);
                        }
                    }
                    // A listing that ends with no results lists nothing
                    Some(Pending::List) if !self.ready => {
                        self.ready = true;
                        log::info!("cranpose xr: anchor storage ready, it keeps no anchors");
                        frame.store.set_persisted(&[]);
                    }
                    _ => {}
                }
            }
            SpatialEvent::Erased {
                request,
                result,
                uuid,
            } => {
                if let Some(Pending::Erase(space)) = self.take(request) {
                    drop(space);
                    log::info!("cranpose xr: erased {uuid} from the storage: {result:?}");
                    self.start_list(frame);
                }
            }
        }
    }

    // Wraps the space that the runtime made for the anchor, which ends with the anchor.
    fn hold(&self, anchor: &Anchor, space: xr::sys::Space) {
        let space = meta_calls::adopt(&self.session, space);
        anchor.with_resource(|held: &AnchorSpace| {
            let _ = held.space.set(space);
            held.phase.set(Phase::Ready);
        });
    }

    fn on_storable(&mut self, id: u64, result: xr::sys::Result, frame: &Frame<'_>) {
        let Some(anchor) = frame.store.anchor_with_id(id) else {
            return;
        };
        let space = anchor
            .with_resource(|held: &AnchorSpace| held.space.get().map(xr::Space::as_raw))
            .flatten();
        let saved = match space {
            Some(space) if result.into_raw() >= 0 => {
                meta_calls::save(&self.instance, &self.session, space)
            }
            _ => Err(result),
        };
        match saved {
            Ok(request) => {
                anchor.with_resource(|held: &AnchorSpace| held.phase.set(Phase::Saving));
                self.pending.push((request, Pending::Save(id)));
            }
            Err(error) => {
                log::warn!("cranpose xr: anchor {id} cannot be saved: {error:?}");
                anchor.with_resource(|held: &AnchorSpace| held.phase.set(Phase::Ready));
                frame.store.finish_persist(&anchor, None);
            }
        }
    }

    // The anchor from the storage can be located now.
    fn on_locatable(&mut self, id: u64, result: xr::sys::Result, frame: &Frame<'_>) {
        let Some(anchor) = frame.store.anchor_with_id(id) else {
            return;
        };
        if result.into_raw() < 0 {
            log::warn!("cranpose xr: anchor {id} cannot be located: {result:?}");
            frame.store.detach(&anchor);
        } else {
            anchor.with_resource(|held: &AnchorSpace| held.phase.set(Phase::Ready));
        }
    }

    fn on_results(&mut self, request: u64, frame: &Frame<'_>) {
        let Some(at) = self.pending.iter().position(|(id, _)| *id == request) else {
            return;
        };
        if meta_calls::results(&self.instance, &self.session, request, &mut self.results).is_err() {
            return;
        }
        match &self.pending[at].1 {
            Pending::List => {
                self.listed.clear();
                self.listed
                    .extend(self.results.iter().map(|found| uuid_of(&found.uuid)));
                self.release_results();
                if !self.ready {
                    self.ready = true;
                    log::info!(
                        "cranpose xr: anchor storage ready, it keeps {} anchors: {:?}",
                        self.listed.len(),
                        self.listed
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                    );
                }
                frame.store.set_persisted(&self.listed);
            }
            Pending::Load(id) => {
                let id = *id;
                self.pending.remove(at);
                self.on_loaded(id, frame);
            }
            Pending::FindToErase => {
                self.pending.remove(at);
                self.on_found_to_erase();
            }
            _ => {}
        }
    }

    // Gives the anchor the saved entity that the query found, and asks the runtime to enable its
    // location.
    fn on_loaded(&mut self, id: u64, frame: &Frame<'_>) {
        let Some(anchor) = frame.store.anchor_with_id(id) else {
            self.release_results();
            return;
        };
        let Some(found) = self.results.first().copied() else {
            frame.store.detach(&anchor);
            return;
        };
        self.hold(&anchor, found.space);
        // Any further result is not the anchor: its space ends here
        for extra in self.results.drain(1..) {
            drop(meta_calls::adopt(&self.session, extra.space));
        }
        self.results.clear();
        anchor.with_resource(|held: &AnchorSpace| held.phase.set(Phase::Making));
        match meta_calls::enable_component(
            &self.instance,
            found.space,
            xr::sys::SpaceComponentTypeFB::LOCATABLE,
        ) {
            Ok(request) => self.pending.push((request, Pending::Locatable(id))),
            Err(error) => {
                log::warn!("cranpose xr: anchor {id} cannot be located: {error:?}");
                frame.store.detach(&anchor);
            }
        }
    }

    // Erases the entities that a query found for an unpersist request.
    fn on_found_to_erase(&mut self) {
        for found in std::mem::take(&mut self.results) {
            let space = meta_calls::adopt(&self.session, found.space);
            match meta_calls::erase(&self.instance, &self.session, found.space) {
                Ok(request) => self.pending.push((request, Pending::Erase(space))),
                Err(error) => log::warn!("cranpose xr: erase: {error:?}"),
            }
        }
    }

    // Ends the spaces of the results that the app does not keep.
    fn release_results(&mut self) {
        for found in self.results.drain(..) {
            drop(meta_calls::adopt(&self.session, found.space));
        }
    }
}
