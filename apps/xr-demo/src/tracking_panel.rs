// A panel that shows which tracking the runtime offers and how far each feature has come, and a dot
// at each fingertip of the hands that the cameras track. The dots are small panels. Each one stands
// at its fingertip in the room, facing the direction that the headset looked at when the app
// started, so a viewer who looks that way sees them on the fingers.
//
// The app asks only for hand tracking, which needs no prompt on a Meta Quest. The build asks for the
// body, the face and the eyes as well when the environment variable `XR_DEMO_TRACKING` is `all`,
// and the runtime then shows its prompts for them.

use cranpose::prelude::*;
use cranpose_ui::text::FontWeight;
use cranpose_xr::{
    FaceBlendShapeType, FaceDataSource, FaceTrackingProvider, Hand, HandJointType, Pose,
    SpatialFrameEffect, SpatialPanel, SubspaceModifier, TrackingAvailability, TrackingCapabilities,
    TrackingFeature, TrackingState, XrFrame, local_tracking_capabilities,
};
use player_demo::sized;

// The diameter of a fingertip's dot in dp, which are millimeters.
const DOT: f32 = 14.0;
// The fingertips, from the thumb to the little finger.
const TIPS: [HandJointType; 5] = [
    HandJointType::ThumbTip,
    HandJointType::IndexTip,
    HandJointType::MiddleTip,
    HandJointType::RingTip,
    HandJointType::LittleTip,
];
// A subspace starts a meter in front of where the head was when the app started.
const SUBSPACE_DEPTH: f32 = 1.0;

// Where a fingertip is in the room, rounded to a millimeter so that a hand at rest redraws nothing.
type Dot = Option<[f32; 3]>;

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

// Rounds a place to a millimeter.
fn millimeters(position: [f32; 3]) -> [f32; 3] {
    position.map(|meters| (meters * 1000.0).round() / 1000.0)
}

// The panel with the readout, and a dot for each fingertip of each hand.
#[composable]
pub(crate) fn TrackingPanel() {
    let readout = rememberMutableStateOf(TrackingReadout::default);
    let dots = remember(|| std::array::from_fn::<_, 10, _>(|_| mutableStateOf(None::<[f32; 3]>)))
        .with(|dots| *dots);
    SpatialFrameEffect(move |frame| {
        let next = TrackingReadout::of(frame);
        if readout.get_non_reactive() != next {
            readout.set(next);
        }
        for (side, hand) in [Hand::Left, Hand::Right].into_iter().enumerate() {
            let state = frame.hand(hand);
            for (n, tip) in TIPS.into_iter().enumerate() {
                let joint = state.joint(tip);
                let place: Dot = (state.tracking_state == TrackingState::Tracking && joint.valid)
                    .then(|| millimeters(joint.pose.position));
                let dot = dots[side * TIPS.len() + n];
                if dot.get_non_reactive() != place {
                    dot.set(place);
                }
            }
        }
    });
    SpatialPanel(
        SubspaceModifier::new()
            .size(800.0, 330.0)
            .offset(0.0, -560.0, 0.0),
        move || {
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color(0.1, 0.12, 0.14, 0.94))
                    .rounded_corners(24.0)
                    .padding(28.0),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
                move || {
                    Text(
                        "Tracking",
                        Modifier::empty(),
                        sized(30.0, Some(FontWeight::BOLD)),
                    );
                    ReadoutLines(readout);
                },
            );
        },
    );
    for (side, hand) in [Hand::Left, Hand::Right].into_iter().enumerate() {
        for n in 0..TIPS.len() {
            FingertipDot(hand, dots[side * TIPS.len() + n]);
        }
    }
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

// A dot at a fingertip: blue for the left hand, orange for the right. It shows only while the
// cameras track the fingertip.
#[composable]
fn FingertipDot(hand: Hand, place: MutableState<Dot>) {
    let Some([x, y, z]) = place.get() else {
        return;
    };
    let color = match hand {
        Hand::Left => Color(0.3, 0.55, 1.0, 1.0),
        Hand::Right => Color(1.0, 0.6, 0.2, 1.0),
    };
    SpatialPanel(
        SubspaceModifier::new()
            .size(DOT, DOT)
            .with_pose(Pose::at([x, y, z + SUBSPACE_DEPTH])),
        move || {
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .background(color)
                    .rounded_corners(DOT / 2.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}
