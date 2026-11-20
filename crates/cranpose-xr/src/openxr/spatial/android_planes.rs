//! Planes on Android XR: `XR_ANDROID_trackables` gives them.
#![expect(
    unsafe_code,
    reason = "XR_ANDROID_trackables, which the openxr binding wraps in no safe call"
)]

use std::{cell::Cell, ptr, time::Duration};

use openxr as xr;
use xr::sys::Handle;

use super::frame::Frame;
use crate::{
    Plane, PlaneLabel, PlaneState, PlaneType, TrackingState,
    openxr::{graphics::Api, session::pose_of},
    plane::PlaneKey,
    plane_classify::type_of_normal,
};

// How often the app asks the runtime for the list of its planes, which changes slowly.
const SCAN: Duration = Duration::from_secs(1);

// What a plane holds in the runtime: the time of its last update, which tells when its polygon
// changed.
struct Held {
    updated: Cell<i64>,
}

// What the runtime says of one plane.
struct Read {
    plane_type: PlaneType,
    state: PlaneState,
    subsumed_by: Option<PlaneKey>,
    updated: i64,
}

pub(crate) struct AndroidPlanes {
    instance: xr::Instance,
    session: xr::Session<Api>,
    tracker: Option<xr::sys::TrackableTrackerANDROID>,
    last_scan: Option<Duration>,
    // Reused at each scan and each read of a polygon
    trackables: Vec<xr::sys::TrackableANDROID>,
    vertices: Vec<xr::sys::Vector2f>,
}

impl AndroidPlanes {
    pub(crate) fn new(instance: &xr::Instance, session: &xr::Session<Api>) -> Self {
        Self {
            instance: instance.clone(),
            session: session.clone(),
            tracker: None,
            last_scan: None,
            trackables: Vec::new(),
            vertices: Vec::new(),
        }
    }

    // Makes the plane tracker. The runtime refuses it until the user grants the permission, and
    // answers with the result that says so.
    pub(crate) fn start(&mut self) -> Result<(), xr::sys::Result> {
        let fp = self
            .instance
            .exts()
            .android_trackables
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let info = xr::sys::TrackableTrackerCreateInfoANDROID {
            ty: xr::sys::TrackableTrackerCreateInfoANDROID::TYPE,
            next: ptr::null(),
            trackable_type: xr::sys::TrackableTypeANDROID::PLANE,
        };
        let mut handle = xr::sys::TrackableTrackerANDROID::NULL;
        // SAFETY: the session handle is live, and `info` is initialized with its type.
        let result =
            unsafe { (fp.create_trackable_tracker)(self.session.as_raw(), &info, &mut handle) };
        if result.into_raw() < 0 {
            return Err(result);
        }
        self.tracker = Some(handle);
        Ok(())
    }

    pub(crate) fn is_running(&self) -> bool {
        self.tracker.is_some()
    }

    // One frame: looks for new and lost planes once a second, and reads the pose and the state
    // of every plane that the app holds.
    pub(crate) fn sample(&mut self, frame: &Frame<'_>) {
        let Some(tracker) = self.tracker else {
            return;
        };
        let due = self
            .last_scan
            .is_none_or(|last| frame.now.saturating_sub(last) >= SCAN);
        if due {
            self.last_scan = Some(frame.now);
            self.scan(tracker, frame);
        }
        for index in 0..frame.store.plane_count() {
            let Some(plane) = frame.store.plane_at(index) else {
                break;
            };
            self.refresh(tracker, &plane, frame);
        }
    }

    // Asks the runtime for its planes: adds the new ones and drops the lost ones.
    fn scan(&mut self, tracker: xr::sys::TrackableTrackerANDROID, frame: &Frame<'_>) {
        let Some(fp) = self.instance.exts().android_trackables.as_ref() else {
            return;
        };
        let mut count = 0_u32;
        // SAFETY: the tracker is live. A capacity of zero asks for the count and reads no array.
        let result = unsafe { (fp.get_all_trackables)(tracker, 0, &mut count, ptr::null_mut()) };
        if result.into_raw() < 0 {
            return;
        }
        self.trackables.clear();
        self.trackables
            .resize(count as usize, xr::sys::TrackableANDROID::from_raw(0));
        let capacity = count;
        // SAFETY: `trackables` holds `capacity` atoms, which the runtime fills.
        let result = unsafe {
            (fp.get_all_trackables)(tracker, capacity, &mut count, self.trackables.as_mut_ptr())
        };
        if result.into_raw() < 0 {
            return;
        }
        self.trackables.truncate(count as usize);
        let listed = |key: PlaneKey| {
            self.trackables
                .iter()
                .any(|trackable| u128::from(trackable.into_raw()) == key.0)
        };
        frame.store.retain_planes(|plane| listed(plane.key()));
        for index in 0..self.trackables.len() {
            let key = PlaneKey(u128::from(self.trackables[index].into_raw()));
            if frame.store.plane_with_key(key).is_some() {
                continue;
            }
            let Ok(read) = self.read(tracker, key, frame, true) else {
                continue;
            };
            log::info!(
                "cranpose xr: plane {:?} {:?}, {:?} by {:?} m, at {:?}",
                read.plane_type,
                read.state.label,
                read.state.extents[0],
                read.state.extents[1],
                read.state.center_pose.position
            );
            let plane = Plane::new(key, read.plane_type, read.state, frame.store.weak());
            plane.set_subsumed_by(read.subsumed_by);
            plane.set_vertices(self.vertices.iter().map(|v| [v.x, v.y]));
            plane.attach(Held {
                updated: Cell::new(read.updated),
            });
            frame.store.add_plane(plane);
        }
    }

    // Reads the plane's pose and state for the frame, and its polygon when the runtime changed
    // the plane since the last read.
    fn refresh(
        &mut self,
        tracker: xr::sys::TrackableTrackerANDROID,
        plane: &Plane,
        frame: &Frame<'_>,
    ) {
        let Ok(read) = self.read(tracker, plane.key(), frame, false) else {
            return;
        };
        let known = plane
            .with_resource(|held: &Held| Some(held.updated.get()))
            .unwrap_or_default();
        if read.updated != known
            && let Ok(full) = self.read(tracker, plane.key(), frame, true)
        {
            plane.set_vertices(self.vertices.iter().map(|v| [v.x, v.y]));
            plane.with_resource(|held: &Held| {
                held.updated.set(full.updated);
                Some(())
            });
        }
        let before = plane.current();
        let changed = before.label != read.state.label
            || before.tracking_state != read.state.tracking_state
            || plane.plane_type() != read.plane_type
            || plane.is_subsumed() != read.subsumed_by.is_some();
        plane.publish(read.state);
        plane.set_plane_type(read.plane_type);
        plane.set_subsumed_by(read.subsumed_by);
        if changed {
            frame.store.plane_changed();
        }
    }

    // Asks the runtime for one plane at the frame's time, in the app's room. With `polygon` it reads
    // the plane's vertices into `self.vertices` as well.
    fn read(
        &mut self,
        tracker: xr::sys::TrackableTrackerANDROID,
        key: PlaneKey,
        frame: &Frame<'_>,
        polygon: bool,
    ) -> Result<Read, xr::sys::Result> {
        let fp = self
            .instance
            .exts()
            .android_trackables
            .as_ref()
            .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
        let info = xr::sys::TrackableGetInfoANDROID {
            ty: xr::sys::TrackableGetInfoANDROID::TYPE,
            next: ptr::null(),
            trackable: xr::sys::TrackableANDROID::from_raw(key.0 as u64),
            base_space: frame.xr_session.local.as_raw(),
            time: frame.time,
        };
        let mut count = 0_u32;
        let mut plane = blank_plane(&mut count);
        // SAFETY: the tracker is live. `info` and `plane` are initialized with their types, and
        // a capacity of zero asks for the vertex count and reads no array.
        let result = unsafe { (fp.get_trackable_plane)(tracker, &info, &mut plane) };
        if result.into_raw() < 0 {
            return Err(result);
        }
        if polygon {
            self.vertices.clear();
            self.vertices
                .resize(count as usize, xr::sys::Vector2f { x: 0.0, y: 0.0 });
            plane.vertex_capacity_input = count;
            plane.vertices = self.vertices.as_mut_ptr();
            // SAFETY: `vertices` holds `count` points, which `vertex_capacity_input` names.
            let result = unsafe { (fp.get_trackable_plane)(tracker, &info, &mut plane) };
            if result.into_raw() < 0 {
                self.vertices.clear();
                return Err(result);
            }
            let written = (unsafe { *plane.vertex_count_output } as usize).min(self.vertices.len());
            self.vertices.truncate(written);
        }
        let center = pose_of(&plane.center_pose);
        Ok(Read {
            plane_type: plane_type_of(plane.plane_type, &center),
            state: PlaneState {
                center_pose: center,
                extents: [plane.extents.width, plane.extents.height],
                label: label_of(plane.plane_label),
                tracking_state: tracking_of(plane.tracking_state),
            },
            subsumed_by: (plane.subsumed_by_plane.into_raw() != 0)
                .then(|| PlaneKey(u128::from(plane.subsumed_by_plane.into_raw()))),
            updated: plane.last_updated_time.as_nanos(),
        })
    }
}

impl Drop for AndroidPlanes {
    fn drop(&mut self) {
        let Some(tracker) = self.tracker.take() else {
            return;
        };
        if let Some(fp) = self.instance.exts().android_trackables.as_ref() {
            // SAFETY: the tracker is live and nothing uses it after this call.
            unsafe { (fp.destroy_trackable_tracker)(tracker) };
        }
    }
}

// A plane for the runtime to fill: it asks for no vertices and names `count` for their number.
fn blank_plane(count: &mut u32) -> xr::sys::TrackablePlaneANDROID {
    xr::sys::TrackablePlaneANDROID {
        ty: xr::sys::TrackablePlaneANDROID::TYPE,
        next: ptr::null_mut(),
        tracking_state: xr::sys::TrackingStateANDROID::STOPPED,
        center_pose: xr::Posef::IDENTITY,
        extents: xr::Extent2Df {
            width: 0.0,
            height: 0.0,
        },
        plane_type: xr::sys::PlaneTypeANDROID::VERTICAL,
        plane_label: xr::sys::PlaneLabelANDROID::UNKNOWN,
        subsumed_by_plane: xr::sys::TrackableANDROID::from_raw(0),
        last_updated_time: xr::Time::from_nanos(0),
        vertex_capacity_input: 0,
        vertex_count_output: count,
        vertices: ptr::null_mut(),
    }
}

// The plane's direction. A plane with an arbitrary direction takes the one that its normal is
// nearest to.
fn plane_type_of(raw: xr::sys::PlaneTypeANDROID, center: &crate::Pose) -> PlaneType {
    if raw == xr::sys::PlaneTypeANDROID::HORIZONTAL_DOWNWARD_FACING {
        PlaneType::HorizontalDownwardFacing
    } else if raw == xr::sys::PlaneTypeANDROID::HORIZONTAL_UPWARD_FACING {
        PlaneType::HorizontalUpwardFacing
    } else if raw == xr::sys::PlaneTypeANDROID::VERTICAL {
        PlaneType::Vertical
    } else {
        type_of_normal(center.rotate([0.0, 1.0, 0.0]))
    }
}

fn label_of(raw: xr::sys::PlaneLabelANDROID) -> PlaneLabel {
    if raw == xr::sys::PlaneLabelANDROID::WALL {
        PlaneLabel::Wall
    } else if raw == xr::sys::PlaneLabelANDROID::FLOOR {
        PlaneLabel::Floor
    } else if raw == xr::sys::PlaneLabelANDROID::CEILING {
        PlaneLabel::Ceiling
    } else if raw == xr::sys::PlaneLabelANDROID::TABLE {
        PlaneLabel::Table
    } else {
        PlaneLabel::Unknown
    }
}

fn tracking_of(raw: xr::sys::TrackingStateANDROID) -> TrackingState {
    if raw == xr::sys::TrackingStateANDROID::TRACKING {
        TrackingState::Tracking
    } else if raw == xr::sys::TrackingStateANDROID::PAUSED {
        TrackingState::Paused
    } else {
        TrackingState::Stopped
    }
}
