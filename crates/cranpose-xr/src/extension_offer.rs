//! The tracking extensions that a runtime lists and the app enables.

use crate::{TrackingFeature, tracking_request::TrackingRequest};

// The tracking extensions of a set, as plain flags. The runtime's list of extensions fills one,
// and the extensions to enable make another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExtensionOffer {
    pub(crate) hand_tracking: bool,
    pub(crate) hand_tracking_aim: bool,
    pub(crate) body_tracking: bool,
    pub(crate) full_body_tracking: bool,
    pub(crate) face_tracking2: bool,
    pub(crate) android_face_tracking: bool,
    pub(crate) eye_gaze_interaction: bool,
    pub(crate) eye_tracking_social: bool,
}

impl ExtensionOffer {
    // The part of this list that the request needs, which the instance enables and nothing else.
    // An extension that depends on another comes only with it: the aim extension needs hand
    // tracking, and the full body needs body tracking. A face comes from one extension, Meta's
    // before Android's.
    pub(crate) fn enabled_for(self, request: TrackingRequest) -> Self {
        let hands = request.asks(TrackingFeature::Hands) && self.hand_tracking;
        let body = request.asks(TrackingFeature::Body) && self.body_tracking;
        let face = request.asks(TrackingFeature::Face);
        let eyes = request.asks(TrackingFeature::Eyes);
        let meta_face = face && self.face_tracking2;
        Self {
            hand_tracking: hands,
            hand_tracking_aim: hands && self.hand_tracking_aim,
            body_tracking: body,
            full_body_tracking: body && self.full_body_tracking,
            face_tracking2: meta_face,
            android_face_tracking: face && !meta_face && self.android_face_tracking,
            eye_gaze_interaction: eyes && self.eye_gaze_interaction,
            eye_tracking_social: eyes && self.eye_tracking_social,
        }
    }
}
