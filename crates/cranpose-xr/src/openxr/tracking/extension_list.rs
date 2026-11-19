//! The tracking extensions of the instance, as plain flags.

use openxr as xr;

use crate::extension_offer::ExtensionOffer;

// The tracking extensions that the runtime lists.
pub(crate) fn listed(available: &xr::ExtensionSet) -> ExtensionOffer {
    ExtensionOffer {
        hand_tracking: available.ext_hand_tracking,
        hand_tracking_aim: available.fb_hand_tracking_aim,
        body_tracking: available.fb_body_tracking,
        full_body_tracking: available.meta_body_tracking_full_body,
        face_tracking2: available.fb_face_tracking2,
        android_face_tracking: android_face_tracking(available),
        eye_gaze_interaction: available.ext_eye_gaze_interaction,
        eye_tracking_social: available.fb_eye_tracking_social,
    }
}

// Turns on the tracking extensions of `enabled`. It leaves the others as they are, so an app that
// asked for nothing enables none.
pub(crate) fn enable(extensions: &mut xr::ExtensionSet, enabled: ExtensionOffer) {
    extensions.ext_hand_tracking = enabled.hand_tracking;
    extensions.fb_hand_tracking_aim = enabled.hand_tracking_aim;
    extensions.fb_body_tracking = enabled.body_tracking;
    extensions.meta_body_tracking_full_body = enabled.full_body_tracking;
    extensions.fb_face_tracking2 = enabled.face_tracking2;
    enable_android_face_tracking(extensions, enabled.android_face_tracking);
    extensions.ext_eye_gaze_interaction = enabled.eye_gaze_interaction;
    extensions.fb_eye_tracking_social = enabled.eye_tracking_social;
}

// Only an Android runtime has the Android extension, and the binding has the field only there.
#[cfg(target_os = "android")]
fn android_face_tracking(available: &xr::ExtensionSet) -> bool {
    available.android_face_tracking
}

#[cfg(not(target_os = "android"))]
fn android_face_tracking(_available: &xr::ExtensionSet) -> bool {
    false
}

#[cfg(target_os = "android")]
fn enable_android_face_tracking(extensions: &mut xr::ExtensionSet, on: bool) {
    extensions.android_face_tracking = on;
}

#[cfg(not(target_os = "android"))]
fn enable_android_face_tracking(_extensions: &mut xr::ExtensionSet, _on: bool) {}
