//! What the headset's system reports of the anchor extensions that the app enabled.

// What the system properties say. A field is `None` where the extension is off, because a runtime
// answers a property structure only for an extension that is enabled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpatialSupport {
    pub(crate) meta_anchors: Option<bool>,
    pub(crate) android_anchors: Option<bool>,
    pub(crate) android_max_anchors: Option<u32>,
    pub(crate) android_persistence: Option<bool>,
}

impl SpatialSupport {
    // Whether the system said that it lacks the feature. An extension that is off answers
    // nothing, and the runtime's list of extensions decides alone.
    pub(crate) fn lacks(reported: Option<bool>) -> bool {
        reported == Some(false)
    }
}
