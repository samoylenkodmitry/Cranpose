use cranpose::capabilities::{Capabilities, Use};

use super::*;

// What a Meta Quest lists once the manifest holds `USE_ANCHOR_API`: Meta's extensions and none of
// the Android ones.
fn quest_list() -> SpatialOffer {
    SpatialOffer {
        fb_spatial_entity: true,
        fb_spatial_entity_query: true,
        fb_spatial_entity_storage: true,
        fb_scene: true,
        android_trackables: false,
        android_device_anchor_persistence: false,
    }
}

// What an Android XR headset lists: the Android extensions and none of Meta's.
fn android_list() -> SpatialOffer {
    SpatialOffer {
        android_trackables: true,
        android_device_anchor_persistence: true,
        ..SpatialOffer::default()
    }
}

fn asking(uses: &[Use]) -> SpatialRequest {
    SpatialRequest::of(&Capabilities {
        uses,
        demands: &[],
        opens: &[],
        links: &[],
    })
}

const NOTHING: SpatialOffer = SpatialOffer {
    fb_spatial_entity: false,
    fb_spatial_entity_query: false,
    fb_spatial_entity_storage: false,
    fb_scene: false,
    android_trackables: false,
    android_device_anchor_persistence: false,
};

fn setup(request: SpatialRequest, list: SpatialOffer) -> SpatialSetup {
    SpatialSetup::from_runtime(request, list, SpatialSupport::default())
}

#[test]
fn an_app_that_asks_for_nothing_enables_no_extension_and_sees_what_the_runtime_lists() {
    let request = asking(&[]);
    assert!(!request.asks_any());
    for list in [quest_list(), android_list()] {
        let setup = setup(request, list);
        assert_eq!(setup.enabled, NOTHING);
        assert_eq!(setup.anchors.anchors, TrackingAvailability::Offered);
        assert_eq!(setup.anchors.persistence, TrackingAvailability::Offered);
        assert_eq!(setup.planes, TrackingAvailability::Offered);
    }
}

#[test]
fn a_runtime_that_lists_nothing_offers_nothing_whatever_the_app_asks() {
    let request = asking(&[Use::anchors(), Use::plane_tracking()]);
    let setup = setup(request, NOTHING);
    assert_eq!(setup.enabled, NOTHING);
    assert_eq!(setup.provider, None);
    assert_eq!(setup.anchors.anchors, TrackingAvailability::Unavailable);
    assert_eq!(setup.anchors.persistence, TrackingAvailability::Unavailable);
    assert_eq!(setup.planes, TrackingAvailability::Unavailable);
}

#[test]
fn a_quest_that_is_asked_for_anchors_enables_the_entities_their_storage_and_their_query() {
    let enabled = quest_list().enabled_for(asking(&[Use::anchors()]));
    assert_eq!(
        enabled,
        SpatialOffer {
            fb_spatial_entity: true,
            fb_spatial_entity_query: true,
            fb_spatial_entity_storage: true,
            ..NOTHING
        }
    );
}

#[test]
fn a_quest_that_is_asked_for_planes_enables_the_scene_and_not_the_storage() {
    let enabled = quest_list().enabled_for(asking(&[Use::plane_tracking()]));
    assert_eq!(
        enabled,
        SpatialOffer {
            fb_spatial_entity: true,
            fb_spatial_entity_query: true,
            fb_scene: true,
            ..NOTHING
        }
    );
}

#[test]
fn an_android_xr_headset_enables_trackables_and_adds_the_storage_for_anchors_only() {
    let anchors = android_list().enabled_for(asking(&[Use::anchors()]));
    assert_eq!(anchors, android_list());
    let planes = android_list().enabled_for(asking(&[Use::plane_tracking()]));
    assert_eq!(
        planes,
        SpatialOffer {
            android_trackables: true,
            ..NOTHING
        }
    );
}

#[test]
fn a_runtime_that_lists_both_families_uses_android_xrs() {
    let both = SpatialOffer {
        android_trackables: true,
        ..quest_list()
    };
    assert_eq!(both.provider(), Some(SpatialProvider::Android));
    let enabled = both.enabled_for(asking(&[Use::anchors()]));
    assert!(!enabled.fb_spatial_entity);
    assert!(enabled.android_trackables);
}

#[test]
fn a_quest_without_the_query_keeps_no_anchors_and_shows_no_planes() {
    let list = SpatialOffer {
        fb_spatial_entity_query: false,
        ..quest_list()
    };
    let setup = setup(asking(&[Use::anchors(), Use::plane_tracking()]), list);
    assert_eq!(setup.anchors.anchors, TrackingAvailability::Active);
    assert_eq!(setup.anchors.persistence, TrackingAvailability::Unavailable);
    assert_eq!(setup.planes, TrackingAvailability::Unavailable);
    assert!(!setup.enabled.fb_spatial_entity_storage);
    assert!(!setup.enabled.fb_scene);
}

#[test]
fn anchors_need_no_permission_but_planes_and_android_xrs_storage_wait_for_the_user() {
    let request = asking(&[Use::anchors(), Use::plane_tracking()]);
    let quest = setup(request, quest_list());
    assert_eq!(quest.anchors.anchors, TrackingAvailability::Active);
    assert_eq!(quest.anchors.persistence, TrackingAvailability::Active);
    assert_eq!(quest.planes, TrackingAvailability::AwaitingPermission);
    let android = setup(request, android_list());
    assert_eq!(android.anchors.anchors, TrackingAvailability::Active);
    assert_eq!(
        android.anchors.persistence,
        TrackingAvailability::AwaitingPermission
    );
    assert_eq!(android.planes, TrackingAvailability::AwaitingPermission);
}

#[test]
fn an_app_that_asks_for_planes_only_asks_for_no_anchors() {
    let setup = setup(asking(&[Use::plane_tracking()]), quest_list());
    assert_eq!(setup.anchors.anchors, TrackingAvailability::Offered);
    assert_eq!(setup.anchors.persistence, TrackingAvailability::Offered);
    assert_eq!(setup.planes, TrackingAvailability::AwaitingPermission);
}

#[test]
fn what_the_system_reports_of_its_support_overrules_the_list() {
    let request = asking(&[Use::anchors()]);
    let no_anchors = SpatialSupport {
        android_anchors: Some(false),
        ..SpatialSupport::default()
    };
    let setup_without = SpatialSetup::from_runtime(request, android_list(), no_anchors);
    assert_eq!(
        setup_without.anchors.anchors,
        TrackingAvailability::Unavailable
    );
    assert_eq!(
        setup_without.anchors.persistence,
        TrackingAvailability::Unavailable
    );
    let no_storage = SpatialSupport {
        android_anchors: Some(true),
        android_persistence: Some(false),
        android_max_anchors: Some(16),
        ..SpatialSupport::default()
    };
    let setup_with = SpatialSetup::from_runtime(request, android_list(), no_storage);
    assert_eq!(setup_with.anchors.anchors, TrackingAvailability::Active);
    assert_eq!(
        setup_with.anchors.persistence,
        TrackingAvailability::Unavailable
    );
    assert_eq!(setup_with.anchors.max_anchors, Some(16));
    let meta_no = SpatialSupport {
        meta_anchors: Some(false),
        ..SpatialSupport::default()
    };
    let quest_without = SpatialSetup::from_runtime(request, quest_list(), meta_no);
    assert_eq!(
        quest_without.anchors.anchors,
        TrackingAvailability::Unavailable
    );
    assert_eq!(quest_without.planes, TrackingAvailability::Unavailable);
}

#[test]
fn the_requests_come_from_the_declared_services() {
    assert_eq!(
        asking(&[Use::anchors()]),
        SpatialRequest {
            anchors: true,
            planes: false
        }
    );
    assert_eq!(
        asking(&[Use::plane_tracking(), Use::hand_tracking()]),
        SpatialRequest {
            anchors: false,
            planes: true
        }
    );
    assert_eq!(asking(&[Use::hand_tracking()]), SpatialRequest::default());
}
