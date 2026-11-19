use cranpose::capabilities::{Capabilities, Use};

use super::*;

// What a Meta Quest 3 lists: every extension but the Android ones.
fn quest_list() -> ExtensionOffer {
    ExtensionOffer {
        hand_tracking: true,
        hand_tracking_aim: true,
        body_tracking: true,
        full_body_tracking: true,
        face_tracking2: true,
        android_face_tracking: false,
        eye_gaze_interaction: true,
        eye_tracking_social: true,
    }
}

// What an Android XR headset lists.
fn android_list() -> ExtensionOffer {
    ExtensionOffer {
        hand_tracking: true,
        hand_tracking_aim: false,
        body_tracking: false,
        full_body_tracking: false,
        face_tracking2: false,
        android_face_tracking: true,
        eye_gaze_interaction: true,
        eye_tracking_social: false,
    }
}

fn asking(uses: &[Use]) -> TrackingRequest {
    TrackingRequest::of(&Capabilities {
        uses,
        demands: &[],
        opens: &[],
        links: &[],
    })
}

const NOTHING: ExtensionOffer = ExtensionOffer {
    hand_tracking: false,
    hand_tracking_aim: false,
    body_tracking: false,
    full_body_tracking: false,
    face_tracking2: false,
    android_face_tracking: false,
    eye_gaze_interaction: false,
    eye_tracking_social: false,
};

#[test]
fn an_app_that_asks_for_nothing_enables_no_extension_and_sees_what_the_runtime_lists() {
    let request = asking(&[]);
    assert!(!request.asks_any());
    assert_eq!(quest_list().enabled_for(request), NOTHING);
    let capabilities =
        TrackingCapabilities::from_runtime(request, quest_list(), SystemSupport::default());
    for feature in TrackingFeature::ALL {
        assert_eq!(
            capabilities.availability(feature),
            TrackingAvailability::Offered,
            "{feature:?}"
        );
    }
    assert!(capabilities.is_hand_aim_available());
    assert!(capabilities.is_full_body_available());
    assert_eq!(
        capabilities.face_tracking_provider(),
        Some(FaceTrackingProvider::Meta)
    );
}

#[test]
fn a_runtime_that_lists_nothing_offers_nothing_whatever_the_app_asks() {
    let request = asking(&[Use::hand_tracking(), Use::face_tracking()]);
    assert_eq!(NOTHING.enabled_for(request), NOTHING);
    let capabilities =
        TrackingCapabilities::from_runtime(request, NOTHING, SystemSupport::default());
    assert_eq!(capabilities, TrackingCapabilities::default());
    assert!(
        TrackingFeature::ALL
            .into_iter()
            .all(|feature| !capabilities.offers(feature))
    );
}

#[test]
fn only_the_asked_features_enable_their_extensions() {
    let request = asking(&[Use::hand_tracking(), Use::eye_tracking()]);
    let enabled = quest_list().enabled_for(request);
    assert_eq!(
        enabled,
        ExtensionOffer {
            hand_tracking: true,
            hand_tracking_aim: true,
            eye_gaze_interaction: true,
            eye_tracking_social: true,
            ..NOTHING
        }
    );
}

#[test]
fn a_dependent_extension_comes_only_with_the_one_it_extends() {
    let aim_alone = ExtensionOffer {
        hand_tracking_aim: true,
        full_body_tracking: true,
        ..NOTHING
    };
    let request = asking(&[Use::hand_tracking(), Use::body_tracking()]);
    assert_eq!(aim_alone.enabled_for(request), NOTHING);
    let capabilities =
        TrackingCapabilities::from_runtime(request, aim_alone, SystemSupport::default());
    assert!(!capabilities.is_hand_aim_available());
    assert!(!capabilities.is_full_body_available());
}

#[test]
fn a_face_comes_from_metas_extension_before_androids() {
    let both = ExtensionOffer {
        face_tracking2: true,
        android_face_tracking: true,
        ..NOTHING
    };
    let request = asking(&[Use::face_tracking()]);
    let enabled = both.enabled_for(request);
    assert!(enabled.face_tracking2);
    assert!(!enabled.android_face_tracking);
    let capabilities = TrackingCapabilities::from_runtime(request, both, SystemSupport::default());
    assert_eq!(
        capabilities.face_tracking_provider(),
        Some(FaceTrackingProvider::Meta)
    );
    let android = android_list().enabled_for(request);
    assert!(android.android_face_tracking && !android.face_tracking2);
}

#[test]
fn the_system_removes_a_feature_whose_sensor_the_headset_lacks_when_the_app_asked() {
    // A Quest 3 lists the eye extensions and has no eye cameras.
    let request = asking(&[Use::eye_tracking(), Use::face_tracking()]);
    let system = SystemSupport {
        eye_gaze_interaction: Some(false),
        eye_tracking_social: Some(false),
        face_tracking2: Some(true),
        ..SystemSupport::default()
    };
    let capabilities = TrackingCapabilities::from_runtime(request, quest_list(), system);
    assert_eq!(
        capabilities.availability(TrackingFeature::Eyes),
        TrackingAvailability::Unavailable
    );
    assert_eq!(
        capabilities.availability(TrackingFeature::Face),
        TrackingAvailability::AwaitingPermission
    );
    assert!(!capabilities.is_eye_tracking_social_available());
    // The same headset reports the eyes as offered while the app does not ask
    let unasked =
        TrackingCapabilities::from_runtime(asking(&[]), quest_list(), SystemSupport::default());
    assert_eq!(
        unasked.availability(TrackingFeature::Eyes),
        TrackingAvailability::Offered
    );
}

#[test]
fn full_body_needs_the_body_to_be_supported_and_the_system_to_report_it() {
    let request = asking(&[Use::body_tracking()]);
    let upper_only = SystemSupport {
        body_tracking: Some(true),
        full_body_tracking: Some(false),
        ..SystemSupport::default()
    };
    let capabilities = TrackingCapabilities::from_runtime(request, quest_list(), upper_only);
    assert_eq!(
        capabilities.availability(TrackingFeature::Body),
        TrackingAvailability::AwaitingPermission
    );
    assert!(!capabilities.is_full_body_available());
    let no_body = SystemSupport {
        body_tracking: Some(false),
        full_body_tracking: Some(true),
        ..SystemSupport::default()
    };
    let capabilities = TrackingCapabilities::from_runtime(request, quest_list(), no_body);
    assert!(!capabilities.offers(TrackingFeature::Body));
    assert!(!capabilities.is_full_body_available());
}

#[test]
fn an_android_xr_headset_reports_its_own_face_and_eye_extensions() {
    let request = asking(&[Use::face_tracking(), Use::eye_tracking()]);
    let capabilities =
        TrackingCapabilities::from_runtime(request, android_list(), SystemSupport::default());
    assert_eq!(
        capabilities.face_tracking_provider(),
        Some(FaceTrackingProvider::Android)
    );
    assert!(capabilities.is_eye_gaze_interaction_available());
    assert!(!capabilities.is_eye_tracking_social_available());
    assert!(!capabilities.offers(TrackingFeature::Body));
}

#[test]
fn availability_follows_the_permission_of_one_feature_and_leaves_the_others() {
    let request = asking(&[Use::hand_tracking(), Use::face_tracking()]);
    let start = TrackingCapabilities::from_runtime(request, quest_list(), SystemSupport::default());
    let live = start.with_availability(TrackingFeature::Hands, TrackingAvailability::Active);
    assert_eq!(
        live.availability(TrackingFeature::Hands),
        TrackingAvailability::Active
    );
    assert_eq!(
        live.availability(TrackingFeature::Face),
        start.availability(TrackingFeature::Face)
    );
}

#[test]
fn a_feature_is_offered_by_any_extension_that_supplies_it_and_not_by_an_extension_that_extends_it()
{
    let offers = |offer: ExtensionOffer, feature| offer.offers(feature);
    let android_face = ExtensionOffer {
        android_face_tracking: true,
        ..NOTHING
    };
    assert!(offers(android_face, TrackingFeature::Face));
    let social = ExtensionOffer {
        eye_tracking_social: true,
        ..NOTHING
    };
    assert!(offers(social, TrackingFeature::Eyes));
    // The aim and the full body extend hand tracking and body tracking and supply nothing alone
    let extensions_only = ExtensionOffer {
        hand_tracking_aim: true,
        full_body_tracking: true,
        ..NOTHING
    };
    assert!(!offers(extensions_only, TrackingFeature::Hands));
    assert!(!offers(extensions_only, TrackingFeature::Body));
    assert!(
        TrackingFeature::ALL
            .into_iter()
            .all(|feature| !offers(NOTHING, feature))
    );
    assert!(
        TrackingFeature::ALL
            .into_iter()
            .all(|feature| offers(quest_list(), feature))
    );
}
