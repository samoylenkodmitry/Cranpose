//! What the headset's system reports of the tracking extensions that the app enabled.

// What the system properties say. A field is `None` where the extension is off, because a runtime
// answers a property structure only for an extension that is enabled. `Some(false)` means that the
// extension is there and the headset has no sensor for it, as a Quest 3 has no eye cameras.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SystemSupport {
    pub(crate) hand_tracking: Option<bool>,
    pub(crate) body_tracking: Option<bool>,
    pub(crate) full_body_tracking: Option<bool>,
    pub(crate) face_tracking2: Option<bool>,
    pub(crate) android_face_tracking: Option<bool>,
    pub(crate) eye_gaze_interaction: Option<bool>,
    pub(crate) eye_tracking_social: Option<bool>,
}

impl SystemSupport {
    // Whether the system said that it lacks the sensor of an extension. An extension that is off
    // answers nothing, and the runtime's list of extensions decides alone.
    pub(crate) fn lacks(reported: Option<bool>) -> bool {
        reported == Some(false)
    }
}
