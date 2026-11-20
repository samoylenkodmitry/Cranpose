//! The services that track the user's body in a headset.

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
        _ => &NONE,
    }
}
