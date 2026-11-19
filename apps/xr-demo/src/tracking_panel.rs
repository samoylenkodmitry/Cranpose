// A panel that shows which tracking the runtime offers and how far each feature has come, and a map
// of the hands: a dot at each fingertip that the cameras track, seen from the front as the viewer
// looks at them. The map is part of the panel. A panel for each dot would add ten layers to the
// compositor's frame, and a hand's own pointing ray would stop at the dot at its fingertip.
//
// The app asks only for hand tracking, which needs no prompt on a Meta Quest. The build asks for the
// body, the face and the eyes as well when the environment variable `XR_DEMO_TRACKING` is `all`,
// and the runtime then shows its prompts for them.

use cranpose::prelude::*;
use cranpose_ui::text::FontWeight;
use cranpose_xr::{
    FaceBlendShapeType, FaceDataSource, FaceTrackingProvider, Hand, HandJointType,
    SpatialFrameEffect, SpatialPanel, SubspaceModifier, TrackingAvailability, TrackingCapabilities,
    TrackingFeature, TrackingState, XrFrame, local_tracking_capabilities,
};
use player_demo::sized;

// The diameter of a fingertip's dot, and the side of the map, in dp.
const DOT: f32 = 14.0;
const MAP: f32 = 280.0;
// The map shows this many dp for a meter of the room.
const DP_PER_METER: f32 = 350.0;
// The map's center lies this far below the head, where the hands rest, in meters.
const MAP_DROP: f32 = 0.3;
// The fingertips, from the thumb to the little finger.
const TIPS: [HandJointType; 5] = [
    HandJointType::ThumbTip,
    HandJointType::IndexTip,
    HandJointType::MiddleTip,
    HandJointType::RingTip,
    HandJointType::LittleTip,
];
// Where a fingertip is to the right of the head and above the map's center, in meters rounded to a
// millimeter, so that a hand at rest redraws nothing.
type Dot = Option<[f32; 2]>;

// What the panel shows of the frame, rounded so that a still headset redraws nothing.
#[derive(Clone, Copy, Default, PartialEq)]
struct TrackingReadout {
    tracking: TrackingCapabilities,
    hand_joints: [usize; 2],
    body_joints: usize,
    face_source: Option<FaceDataSource>,
    // Whole percent
    jaw_percent: f32,
    eyes_tracked: bool,
}

impl TrackingReadout {
    fn of(frame: &XrFrame) -> Self {
        Self {
            tracking: frame.tracking,
            hand_joints: [Hand::Left, Hand::Right].map(|hand| {
                let state = frame.hand(hand);
                if state.tracking_state == TrackingState::Tracking {
                    state.joints.iter().filter(|joint| joint.valid).count()
                } else {
                    0
                }
            }),
            body_joints: if frame.body.active {
                frame
                    .body
                    .tracked_joints()
                    .iter()
                    .filter(|joint| joint.valid)
                    .count()
            } else {
                0
            },
            face_source: frame.meta_face.data_source.filter(|_| frame.face.is_valid),
            jaw_percent: (frame
                .face
                .weight(FaceBlendShapeType::JawDrop)
                .clamp(0.0, 1.0)
                * 100.0)
                .round(),
            eyes_tracked: frame.eyes.left.tracking_state == TrackingState::Tracking
                || frame.eyes.right.tracking_state == TrackingState::Tracking
                || frame.eyes.gaze.is_some(),
        }
    }
}

// Where a fingertip is on the map: to the right of the head and above the map's center.
fn on_map(tip: [f32; 3], head: [f32; 3]) -> [f32; 2] {
    [tip[0] - head[0], tip[1] - head[1] + MAP_DROP].map(|meters| (meters * 1000.0).round() / 1000.0)
}

// The panel with the readout, and a dot for each fingertip of each hand.
#[composable]
pub(crate) fn TrackingPanel() {
    let readout = rememberMutableStateOf(TrackingReadout::default);
    let dots = remember(|| std::array::from_fn::<_, 10, _>(|_| mutableStateOf(None::<[f32; 2]>)))
        .with(|dots| *dots);
    SpatialFrameEffect(move |frame| {
        let head = frame.device_pose.map_or([0.0; 3], |pose| pose.position);
        let next = TrackingReadout::of(frame);
        if readout.get_non_reactive() != next {
            readout.set(next);
        }
        for (side, hand) in [Hand::Left, Hand::Right].into_iter().enumerate() {
            let state = frame.hand(hand);
            for (n, tip) in TIPS.into_iter().enumerate() {
                let joint = state.joint(tip);
                let place: Dot = (state.tracking_state == TrackingState::Tracking && joint.valid)
                    .then(|| on_map(joint.pose.position, head));
                let dot = dots[side * TIPS.len() + n];
                if dot.get_non_reactive() != place {
                    dot.set(place);
                }
            }
        }
    });
    SpatialPanel(
        SubspaceModifier::new()
            .size(1000.0, 330.0)
            .offset(0.0, -560.0, 0.0),
        move || {
            Row(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color(0.1, 0.12, 0.14, 0.94))
                    .rounded_corners(24.0)
                    .padding(24.0),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(24.0)),
                move || {
                    Column(
                        Modifier::empty(),
                        ColumnSpec::default()
                            .vertical_arrangement(LinearArrangement::spaced_by(8.0)),
                        move || {
                            Text(
                                "Tracking",
                                Modifier::empty(),
                                sized(30.0, Some(FontWeight::BOLD)),
                            );
                            ReadoutLines(readout);
                        },
                    );
                    HandMap(dots);
                },
            );
        },
    );
}

// The hands seen from the front: a dot for each fingertip, blue for the left hand and orange for the
// right.
#[composable]
fn HandMap(dots: [MutableState<Dot>; 10]) {
    Box(
        Modifier::empty()
            .size_points(MAP, MAP)
            .background(Color(0.05, 0.06, 0.08, 1.0))
            .rounded_corners(16.0),
        BoxSpec::default(),
        move || {
            for (side, hand) in [Hand::Left, Hand::Right].into_iter().enumerate() {
                for n in 0..TIPS.len() {
                    FingertipDot(hand, dots[side * TIPS.len() + n]);
                }
            }
        },
    );
}

// The line for each feature: how far it has come, and what the runtime gives with it.
#[composable]
fn ReadoutLines(readout: MutableState<TrackingReadout>) {
    let local = local_tracking_capabilities().current();
    let readout = readout.get();
    // The frame's capabilities follow the permissions. Until the first frame they are the start's.
    let tracking = if readout.tracking == TrackingCapabilities::default() {
        local
    } else {
        readout.tracking
    };
    let [left, right] = readout.hand_joints;
    let hands = format!(
        "{}{}; joints {left} left, {right} right",
        label(tracking.availability(TrackingFeature::Hands)),
        if tracking.is_hand_aim_available() {
            ", aim"
        } else {
            ""
        },
    );
    let body = format!(
        "{}{}; joints {}",
        label(tracking.availability(TrackingFeature::Body)),
        if tracking.is_full_body_available() {
            ", full body"
        } else {
            ""
        },
        readout.body_joints,
    );
    let face = format!(
        "{}{}; {}, jaw {}%",
        label(tracking.availability(TrackingFeature::Face)),
        match tracking.face_tracking_provider() {
            Some(FaceTrackingProvider::Meta) => ", Meta",
            Some(FaceTrackingProvider::Android) => ", Android",
            _ => "",
        },
        match readout.face_source {
            Some(FaceDataSource::Audio) => "from audio",
            Some(FaceDataSource::Visual) => "from cameras",
            _ => "no weights",
        },
        readout.jaw_percent,
    );
    let eyes = format!(
        "{}{}{}; {}",
        label(tracking.availability(TrackingFeature::Eyes)),
        if tracking.is_eye_tracking_social_available() {
            ", both eyes"
        } else {
            ""
        },
        if tracking.is_eye_gaze_interaction_available() {
            ", gaze"
        } else {
            ""
        },
        if readout.eyes_tracked {
            "tracked"
        } else {
            "no gaze"
        },
    );
    for (name, line) in [
        ("Hands", hands),
        ("Body", body),
        ("Face", face),
        ("Eyes", eyes),
    ] {
        Text(
            format!("{name}: {line}"),
            Modifier::empty(),
            sized(20.0, None),
        );
    }
}

// What a feature's availability says in a line.
fn label(availability: TrackingAvailability) -> &'static str {
    match availability {
        TrackingAvailability::Unavailable => "not offered",
        TrackingAvailability::Offered => "offered, not asked for",
        TrackingAvailability::AwaitingPermission => "waiting for the permission",
        TrackingAvailability::PermissionRefused => "permission refused",
        TrackingAvailability::Active => "active",
        _ => "unknown",
    }
}

// A dot at a fingertip. It shows only while the cameras track the fingertip.
#[composable]
fn FingertipDot(hand: Hand, place: MutableState<Dot>) {
    let Some([right, up]) = place.get() else {
        return;
    };
    let color = match hand {
        Hand::Left => Color(0.3, 0.55, 1.0, 1.0),
        Hand::Right => Color(1.0, 0.6, 0.2, 1.0),
    };
    Box(
        Modifier::empty()
            .size_points(DOT, DOT)
            .offset(
                MAP / 2.0 + right * DP_PER_METER - DOT / 2.0,
                MAP / 2.0 - up * DP_PER_METER - DOT / 2.0,
            )
            .background(color)
            .rounded_corners(DOT / 2.0),
        BoxSpec::default(),
        || {},
    );
}
