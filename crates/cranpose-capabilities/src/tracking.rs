//! The services that track the user's body, read the room or read the runtime's models in a headset.

use crate::Service;

// What a tracking service asks of an Android device.
pub(crate) struct Tracking {
    // The name in the build's own files
    pub(crate) name: &'static str,
    // Every permission the manifest declares
    pub(crate) permissions: &'static [&'static str],
    // The permissions that the person answers at run time, as groups of alternatives
    pub(crate) groups: &'static [&'static [&'static str]],
    // The optional features that the manifest names
    pub(crate) features: &'static [&'static str],
}

// One APK serves every headset: Horizon OS on a Meta Quest defines the `com.oculus.permission` names
// and, for hands, face and eyes, the same permissions as `horizonos.permission` names. Android XR
// defines the `android.permission` ones. Each ignores the other's. The Quest asks nobody for the
// hand and body permissions and shows a prompt for the face and the eyes. A Quest 3 estimates the
// face from the microphone, so the face adds RECORD_AUDIO.
const HANDS: Tracking = Tracking {
    name: "hand-tracking",
    permissions: &[
        "android.permission.HAND_TRACKING",
        "com.oculus.permission.HAND_TRACKING",
        "horizonos.permission.HAND_TRACKING",
    ],
    groups: &[&[
        "com.oculus.permission.HAND_TRACKING",
        "horizonos.permission.HAND_TRACKING",
        "android.permission.HAND_TRACKING",
    ]],
    features: &["oculus.software.handtracking"],
};

const BODY: Tracking = Tracking {
    name: "body-tracking",
    permissions: &[
        "android.permission.BODY_TRACKING",
        "com.oculus.permission.BODY_TRACKING",
    ],
    groups: &[&[
        "com.oculus.permission.BODY_TRACKING",
        "android.permission.BODY_TRACKING",
    ]],
    features: &["com.oculus.software.body_tracking"],
};

const FACE: Tracking = Tracking {
    name: "face-tracking",
    permissions: &[
        "android.permission.FACE_TRACKING",
        "android.permission.RECORD_AUDIO",
        "com.oculus.permission.FACE_TRACKING",
        "horizonos.permission.FACE_TRACKING",
    ],
    groups: &[
        &[
            "com.oculus.permission.FACE_TRACKING",
            "horizonos.permission.FACE_TRACKING",
            "android.permission.FACE_TRACKING",
        ],
        &["android.permission.RECORD_AUDIO"],
    ],
    features: &["oculus.software.face_tracking"],
};

const EYES: Tracking = Tracking {
    name: "eye-tracking",
    permissions: &[
        "android.permission.EYE_TRACKING_FINE",
        "com.oculus.permission.EYE_TRACKING",
        "horizonos.permission.EYE_TRACKING",
    ],
    groups: &[&[
        "com.oculus.permission.EYE_TRACKING",
        "horizonos.permission.EYE_TRACKING",
        "android.permission.EYE_TRACKING_FINE",
    ]],
    features: &["oculus.software.eye_tracking"],
};

// The anchors of Meta's spatial entities need `USE_ANCHOR_API`, a normal permission that the system
// grants at install. The Horizon OS runtime lists `XR_FB_spatial_entity`, its storage and query and
// `XR_FB_scene` to an app only while the manifest holds it, under the `horizonos` name on a current
// system and the `com.oculus` name on an older one. Android XR creates anchors with no permission,
// and its storage of anchors asks for `SCENE_UNDERSTANDING_COARSE`, because it exposes the geometry
// of the room.
const ANCHORS: Tracking = Tracking {
    name: "anchors",
    permissions: &[
        "android.permission.SCENE_UNDERSTANDING_COARSE",
        "com.oculus.permission.USE_ANCHOR_API",
        "horizonos.permission.USE_ANCHOR_API",
    ],
    groups: &[&["android.permission.SCENE_UNDERSTANDING_COARSE"]],
    features: &[],
};

// The planes of the room come from Meta's scene data, which a Quest user allows with the "spatial
// data" prompt (`USE_SCENE`), and from Android XR's scene understanding, which asks as
// `SCENE_UNDERSTANDING_COARSE`. Meta's scene extension is listed only while the manifest holds
// `USE_ANCHOR_API` as well, because the scene's planes are spatial entities.
const PLANES: Tracking = Tracking {
    name: "plane-tracking",
    permissions: &[
        "android.permission.SCENE_UNDERSTANDING_COARSE",
        "com.oculus.permission.USE_ANCHOR_API",
        "com.oculus.permission.USE_SCENE",
        "horizonos.permission.USE_ANCHOR_API",
        "horizonos.permission.USE_SCENE",
    ],
    groups: &[&[
        "com.oculus.permission.USE_SCENE",
        "horizonos.permission.USE_SCENE",
        "android.permission.SCENE_UNDERSTANDING_COARSE",
    ]],
    features: &[],
};

// The runtime's models of the controllers come through `XR_FB_render_model`, which the Horizon OS
// runtime lists to an app only while the manifest holds the permission and the feature. Both are
// normal: the system grants the permission at install and asks nobody. Android XR names no such
// permission. An app on a runtime that offers no model draws Cranpose's own.
const CONTROLLER_MODELS: Tracking = Tracking {
    name: "controller-models",
    permissions: &["com.oculus.permission.RENDER_MODEL"],
    groups: &[],
    features: &["com.oculus.feature.RENDER_MODEL"],
};

// A service that asks nothing of the kind
const NONE: Tracking = Tracking {
    name: "",
    permissions: &[],
    groups: &[],
    features: &[],
};

pub(crate) const fn of(service: Service) -> &'static Tracking {
    match service {
        Service::HandTracking => &HANDS,
        Service::BodyTracking => &BODY,
        Service::FaceTracking => &FACE,
        Service::EyeTracking => &EYES,
        Service::Anchors => &ANCHORS,
        Service::PlaneTracking => &PLANES,
        Service::ControllerModels => &CONTROLLER_MODELS,
        _ => &NONE,
    }
}
