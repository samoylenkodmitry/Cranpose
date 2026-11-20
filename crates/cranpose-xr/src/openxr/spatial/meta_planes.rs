//! Planes on a Meta Quest: `XR_FB_scene` describes the room that the user set up.
//!
//! The runtime lists the scene's entities through a query. Each entity that has a rectangle becomes
//! a plane: the runtime gives its label and its polygon once, and its pose in every frame. A room
//! that the user has not set up has none, and the app never starts a scan.

use std::time::Duration;

use openxr as xr;

use super::{
    events::SpatialEvent,
    frame::Frame,
    locate::locate_anchor,
    meta_calls::{self, Filter},
    uuid_wire::uuid_of,
};
use crate::{
    Plane, PlaneState, TrackingState, openxr::graphics::Api, plane::PlaneKey,
    plane_classify::label_of_meta, plane_frame::PlaneFrame,
};

// How often the app asks the runtime for the scene's entities again. The scene changes only when
// the user sets up the room again.
const SCAN: Duration = Duration::from_secs(10);

// The most entities that a scan returns.
const SCAN_LIMIT: u32 = 128;

// What a plane holds in the runtime: the space of its entity, and how the entity's frame maps to the
// plane's.
struct Held {
    space: xr::Space,
    frame: PlaneFrame,
}

// An entity that the scan found and that is not located yet.
struct Candidate {
    key: PlaneKey,
    space: xr::Space,
}

pub(crate) struct MetaPlanes {
    instance: xr::Instance,
    session: xr::Session<Api>,
    running: bool,
    last_scan: Option<Duration>,
    // The request of the scan that runs, and the planes that it found so far
    scanning: Option<u64>,
    seen: Vec<PlaneKey>,
    // The requests that enable an entity's location, with the entity
    locating: Vec<(u64, Candidate)>,
    // Reused for the scan's results and for each entity's labels and polygon
    results: Vec<xr::sys::SpaceQueryResultFB>,
    labels: Vec<u8>,
    boundary: Vec<xr::sys::Vector2f>,
}

impl MetaPlanes {
    pub(crate) fn new(instance: &xr::Instance, session: &xr::Session<Api>) -> Self {
        Self {
            instance: instance.clone(),
            session: session.clone(),
            running: false,
            last_scan: None,
            scanning: None,
            seen: Vec::new(),
            locating: Vec::new(),
            results: Vec::new(),
            labels: Vec::new(),
            boundary: Vec::new(),
        }
    }

    // Starts to read the scene, once the user has allowed it.
    pub(crate) fn start(&mut self) {
        self.running = true;
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running
    }

    // One frame: takes the runtime's answers, starts a scan when one is due and reads the pose of
    // every plane.
    pub(crate) fn sample(&mut self, frame: &Frame<'_>) {
        if !self.running {
            return;
        }
        for event in frame.events {
            self.on_event(*event, frame);
        }
        let due = self
            .last_scan
            .is_none_or(|last| frame.now.saturating_sub(last) >= SCAN);
        if due && self.scanning.is_none() {
            self.last_scan = Some(frame.now);
            self.seen.clear();
            match meta_calls::query(
                &self.instance,
                &self.session,
                Filter::Component(xr::sys::SpaceComponentTypeFB::BOUNDED_2D),
                None,
                SCAN_LIMIT,
            ) {
                Ok(request) => self.scanning = Some(request),
                Err(error) => log::warn!("cranpose xr: the scene query is refused: {error:?}"),
            }
        }
        for index in 0..frame.store.plane_count() {
            let Some(plane) = frame.store.plane_at(index) else {
                break;
            };
            Self::refresh(&plane, frame);
        }
    }

    fn on_event(&mut self, event: SpatialEvent, frame: &Frame<'_>) {
        match event {
            SpatialEvent::QueryResults { request } if self.scanning == Some(request) => {
                self.on_results(request, frame);
            }
            SpatialEvent::QueryDone { request, result } if self.scanning == Some(request) => {
                self.scanning = None;
                // A scan that failed tells nothing of which planes are gone
                if result.into_raw() >= 0 {
                    let seen = &self.seen;
                    frame
                        .store
                        .retain_planes(|plane| seen.contains(&plane.key()));
                }
            }
            SpatialEvent::StatusSet { request, result } => {
                if let Some(at) = self.locating.iter().position(|(id, _)| *id == request) {
                    let (_, candidate) = self.locating.remove(at);
                    if result.into_raw() >= 0 {
                        self.build(candidate, frame);
                    }
                }
            }
            _ => {}
        }
    }

    // Takes each entity of the scan: a known plane keeps its place, and a new one waits for its
    // location before it joins.
    fn on_results(&mut self, request: u64, frame: &Frame<'_>) {
        if meta_calls::results(&self.instance, &self.session, request, &mut self.results).is_err() {
            return;
        }
        let found = std::mem::take(&mut self.results);
        for entity in &found {
            let key = PlaneKey(u128::from_be_bytes(*uuid_of(&entity.uuid).as_bytes()));
            let space = meta_calls::adopt(&self.session, entity.space);
            self.seen.push(key);
            if frame.store.plane_with_key(key).is_some() {
                continue;
            }
            let candidate = Candidate { key, space };
            match meta_calls::component_enabled(
                &self.instance,
                entity.space,
                xr::sys::SpaceComponentTypeFB::LOCATABLE,
            ) {
                Some(true) => self.build(candidate, frame),
                _ => match meta_calls::enable_component(
                    &self.instance,
                    entity.space,
                    xr::sys::SpaceComponentTypeFB::LOCATABLE,
                ) {
                    Ok(request) => self.locating.push((request, candidate)),
                    Err(error) => {
                        log::debug!("cranpose xr: a scene entity cannot be located: {error:?}");
                    }
                },
            }
        }
        self.results = found;
        self.results.clear();
    }

    // Makes the plane of an entity that can be located: its rectangle, label and polygon come from
    // the runtime once.
    fn build(&mut self, candidate: Candidate, frame: &Frame<'_>) {
        let Candidate { key, space } = candidate;
        let raw = space.as_raw();
        let Ok((offset, extent)) = meta_calls::bounding_box(&self.instance, &self.session, raw)
        else {
            return;
        };
        let (Some(pose), _) = locate_anchor(&space, &frame.xr_session.local, frame.time) else {
            return;
        };
        let label = match meta_calls::labels(&self.instance, &self.session, raw, &mut self.labels) {
            Ok(()) => label_of_meta(&String::from_utf8_lossy(&self.labels)),
            Err(_) => crate::PlaneLabel::Unknown,
        };
        let plane_frame = PlaneFrame::of(&pose, offset, extent, label);
        let state = PlaneState {
            center_pose: plane_frame.center_pose(&pose),
            extents: plane_frame.extents,
            label,
            tracking_state: TrackingState::Tracking,
        };
        let plane = Plane::new(key, plane_frame.plane_type, state, frame.store.weak());
        if meta_calls::boundary(&self.instance, &self.session, raw, &mut self.boundary).is_ok() {
            plane.set_vertices(self.boundary.iter().map(|v| plane_frame.vertex([v.x, v.y])));
        }
        plane.attach(Held {
            space,
            frame: plane_frame,
        });
        log::info!(
            "cranpose xr: plane {:?} {:?} {:?} extents {:?} at {:?}",
            plane.plane_type(),
            label,
            key,
            extent,
            state.center_pose
        );
        frame.store.add_plane(plane);
    }

    // Reads the plane's pose for the frame. The scene's entities are static, so a pose that the
    // runtime gives counts as tracked.
    fn refresh(plane: &Plane, frame: &Frame<'_>) {
        let read = plane.with_resource(|held: &Held| {
            let (pose, _) = locate_anchor(&held.space, &frame.xr_session.local, frame.time);
            Some(pose.map(|pose| held.frame.center_pose(&pose)))
        });
        let Some(center) = read else {
            return;
        };
        let before = plane.current();
        let tracking = if center.is_some() {
            TrackingState::Tracking
        } else {
            TrackingState::Paused
        };
        plane.publish(PlaneState {
            center_pose: center.unwrap_or(before.center_pose),
            tracking_state: tracking,
            ..before
        });
        if before.tracking_state != tracking {
            frame.store.plane_changed();
        }
    }
}
