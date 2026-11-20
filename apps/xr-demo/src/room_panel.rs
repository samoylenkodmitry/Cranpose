// A panel that stays where the user put it, also after the app restarts, and a list of the planes
// that the runtime reports for the room.
//
// The user carries the panel with a controller and lets it go. The panel snaps to a wall, a table
// or the floor when it is near one. The app then makes an anchor at the place where the panel
// stands and asks the runtime to keep it. At its next start the app loads the anchor that the runtime
// kept, and the panel stands there again. The build asks for the anchors and the planes, which the
// environment variable `XR_DEMO_ROOM` turns on: the planes need the user's permission, and a Quest
// that has the permission in a build shows its prompt before the app starts.
//
// `XR_DEMO_ROOM=script` also places the panel once with no user: the first start, which finds no kept
// anchor, behaves as if the user dropped the panel at a set place. A device with no controller to
// carry the panel shows the persistence that way.

use std::{cell::Cell, rc::Rc};

use cranpose::prelude::*;
use cranpose_ui::text::FontWeight;
use cranpose_xr::{
    Anchor, AnchorPersistence, Anchors, FollowMode, FollowTarget, FollowingSubspace, MovePolicy,
    Plane, PlaneLabel, PlaneType, Planes, Pose, SpatialFrameEffect, SpatialMoveEventType,
    SpatialPanel, Subspace, SubspaceModifier, TrackingAvailability, TrackingState, Uuid,
    local_anchors, local_planes,
};
use player_demo::sized;

// Where a scripted start puts the panel in the room: a place that the first panel position of a
// subspace does not hold, so that a screenshot tells the two apart.
const SCRIPTED_PLACE: [f32; 3] = [0.5, -0.1, -1.2];

// Whether the build places the panel by itself, for a device where no controller carries it.
pub(crate) fn scripted(build: Option<&str>) -> bool {
    build == Some("script")
}

// The room panel: standing where the runtime kept its anchor, or where a subspace puts it.
#[composable]
pub(crate) fn RoomDemo(script: bool) {
    let (Some(anchors), Some(planes)) = (local_anchors().current(), local_planes().current())
    else {
        return;
    };
    let placed = rememberMutableStateOf(|| None::<Anchor>);
    let capabilities = anchors.capabilities();
    let kept = anchors.persisted_uuids();
    let last_kept = kept.last().copied();
    // The runtime reads its storage some frames after the start. The app loads what it kept once, as
    // soon as the list names an anchor, and a scripted start places the panel when the list has come
    // and is empty.
    let loader = anchors.clone();
    DisposableEffect(
        (capabilities.persistence, last_kept, placed.get().is_some()),
        move |_| {
            if placed.get_non_reactive().is_none()
                && capabilities.persistence == TrackingAvailability::Active
            {
                match last_kept {
                    Some(uuid) => load(&loader, placed, uuid),
                    None if script => place(&loader, placed, Pose::at(SCRIPTED_PLACE)),
                    None => {}
                }
            }
            DisposableEffectResult::new(|| {})
        },
    );
    let on_anchors = anchors;
    match placed.get() {
        Some(anchor) => {
            AnchorLog(anchor.clone());
            FollowingSubspace(
                FollowTarget::Anchor(anchor.clone(), FollowMode::Tight),
                move || {
                    RoomPanel(
                        on_anchors.clone(),
                        placed,
                        planes.clone(),
                        Some(anchor.clone()),
                    );
                },
            );
        }
        None => Subspace(move || RoomPanel(on_anchors.clone(), placed, planes.clone(), None)),
    }
}

// Loads the anchor that the runtime kept under `uuid`, and holds the panel to it.
fn load(anchors: &Anchors, placed: MutableState<Option<Anchor>>, uuid: Uuid) {
    match anchors.load(uuid).into_anchor() {
        Some(anchor) => {
            log::info!("xr demo room: loads the anchor {uuid}");
            placed.set(Some(anchor));
        }
        None => log::warn!("xr demo room: the anchor {uuid} does not load"),
    }
}

// Makes an anchor at `pose` in the room, asks the runtime to keep it and holds the panel to it. The
// anchor that held the panel before is deleted from the runtime's storage, so that the app keeps one.
fn place(anchors: &Anchors, placed: MutableState<Option<Anchor>>, pose: Pose) {
    let Some(anchor) = anchors.create(pose).into_anchor() else {
        log::warn!("xr demo room: no anchor at {pose:?}");
        return;
    };
    log::info!("xr demo room: makes an anchor at {pose:?}");
    anchor.persist();
    if let Some(old) = placed.get_non_reactive() {
        if let Some(uuid) = old.uuid() {
            anchors.unpersist(uuid);
        }
        old.detach();
    }
    placed.set(Some(anchor));
}

// Logs the anchor once its persistence is known and again when the runtime first tracks it: the
// identifier, the anchor's pose in the room and its pose in the stage, which the room's origin does
// not move, and the head where the app started. Two starts of the app show the same stage pose for the
// anchor that the first start persisted.
#[composable]
fn AnchorLog(anchor: Anchor) {
    let persistence = anchor.persistence();
    DisposableEffect(persistence, move |_| {
        if let AnchorPersistence::Persisted(uuid) = persistence {
            log::info!("xr demo room: the anchor is kept as {uuid}");
        }
        DisposableEffectResult::new(|| {})
    });
    let logged = remember(|| Rc::new(Cell::new(false))).with(Clone::clone);
    let watched = anchor;
    SpatialFrameEffect(move |frame| {
        let state = watched.state();
        if logged.get() || state.tracking_state != TrackingState::Tracking {
            return;
        }
        logged.set(true);
        let in_stage = frame
            .stage_pose
            .map(|stage| stage.inverse().then(&state.pose));
        log::info!(
            "xr demo room: the anchor {:?} is tracked at {:?} in the room, {:?} in the stage, with the head at {:?}",
            watched.uuid(),
            state.pose,
            in_stage,
            frame.device_pose,
        );
    });
}

// The panel: the anchor's state and the planes of the room. The user carries it, and it snaps to
// the planes that the runtime reports.
#[composable]
fn RoomPanel(
    anchors: Anchors,
    placed: MutableState<Option<Anchor>>,
    planes: Planes,
    anchor: Option<Anchor>,
) {
    let heard = anchors.clone();
    let policy = MovePolicy::anchor(&[], &[], move |event| {
        if event.event_type == SpatialMoveEventType::End {
            place(&heard, placed, event.room_pose);
        }
    });
    SpatialPanel(
        SubspaceModifier::new()
            .size(760.0, 700.0)
            .movable_with(policy),
        move || {
            let (anchors, planes, anchor) = (anchors.clone(), planes.clone(), anchor.clone());
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color(0.1, 0.13, 0.1, 0.95))
                    .rounded_corners(24.0)
                    .padding(32.0),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
                move || {
                    Text(
                        "Anchors and planes",
                        Modifier::empty(),
                        sized(34.0, Some(FontWeight::BOLD)),
                    );
                    AnchorLines(&anchors, anchor.as_ref());
                    PlaneLines(&planes);
                },
            );
        },
    );
}

#[composable]
fn AnchorLines(anchors: &Anchors, anchor: Option<&Anchor>) {
    let capabilities = anchors.capabilities();
    Text(
        format!(
            "Anchors: {}, storage: {}, kept: {}",
            availability(capabilities.anchors),
            availability(capabilities.persistence),
            anchors.persisted_uuids().len()
        ),
        Modifier::empty(),
        sized(20.0, None),
    );
    let line = match anchor {
        Some(anchor) => {
            let state = anchor.state();
            format!(
                "The panel's anchor is {:?}, {}",
                state.tracking_state,
                match anchor.persistence() {
                    AnchorPersistence::Persisted(uuid) => format!("kept as {uuid}"),
                    other => format!("{other:?}"),
                }
            )
        }
        None => "Carry the panel and let it go to keep it where it stands".to_owned(),
    };
    Text(line, Modifier::empty(), sized(18.0, None));
}

#[composable]
fn PlaneLines(planes: &Planes) {
    let listed = planes.all();
    Text(
        format!(
            "Planes: {}, {} found",
            availability(planes.availability()),
            listed.len()
        ),
        Modifier::empty(),
        sized(20.0, None),
    );
    for plane in listed.iter().take(8) {
        Text(plane_line(plane), Modifier::empty(), sized(18.0, None));
    }
}

// A plane in a line: what it is, which way it faces and how large it is.
fn plane_line(plane: &Plane) -> String {
    let state = plane.state();
    let [width, depth] = state.extents;
    format!(
        "{} ({}): {width:.1} by {depth:.1} m at height {:.2} m",
        match state.label {
            PlaneLabel::Ceiling => "ceiling",
            PlaneLabel::Floor => "floor",
            PlaneLabel::Table => "table",
            PlaneLabel::Wall => "wall",
            _ => "unlabeled",
        },
        match plane.plane_type() {
            PlaneType::HorizontalDownwardFacing => "faces down",
            PlaneType::HorizontalUpwardFacing => "faces up",
            PlaneType::Vertical => "vertical",
            _ => "other",
        },
        state.center_pose.position[1],
    )
}

fn availability(availability: TrackingAvailability) -> &'static str {
    match availability {
        TrackingAvailability::Unavailable => "not offered",
        TrackingAvailability::Offered => "offered, not asked for",
        TrackingAvailability::AwaitingPermission => "waiting for the permission",
        TrackingAvailability::PermissionRefused => "permission refused",
        TrackingAvailability::Active => "active",
        _ => "unknown",
    }
}
