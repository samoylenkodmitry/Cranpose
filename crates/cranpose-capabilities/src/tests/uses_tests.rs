use crate::{
    Capabilities, Service, Use, android_features, android_manifest, android_permissions,
    android_permissions_up_to, apple_usage, json, rust_source,
};

const NONE: Capabilities<'static> = Capabilities {
    uses: &[],
    demands: &[],
    opens: &[],
    links: &[],
};

const UNLOCK: [Use; 1] = [Use::device_auth("Opens your history with your face.")];

#[test]
fn device_auth_asks_for_biometrics_and_gives_face_id_its_sentence() {
    let unlock = Capabilities {
        uses: &UNLOCK,
        demands: &[],
        opens: &[],
        links: &[],
    };
    assert_eq!(
        android_permissions(&unlock),
        vec!["android.permission.USE_BIOMETRIC"]
    );
    assert!(android_permissions_up_to(&unlock).is_empty());
    let usage = apple_usage(&unlock);
    assert!(
        usage.contains(
            "\t<key>NSFaceIDUsageDescription</key>\n\t<string>Opens your history with your face.</string>"
        ),
        "{usage}"
    );
    assert!(json(&unlock).contains("\"device-auth\""));
    assert!(rust_source(&unlock).contains("Use::device_auth(\"Opens your history"));
}

#[test]
fn an_application_that_does_not_declare_device_auth_names_no_biometric() {
    assert!(!android_manifest(&NONE).contains("USE_BIOMETRIC"));
    assert!(!apple_usage(&NONE).contains("NSFaceID"));
}

const MOTION: [Use; 1] = [Use::device_motion()];

#[test]
fn device_motion_asks_for_the_fast_sensor_permission_and_no_sentence() {
    let motion = Capabilities {
        uses: &MOTION,
        demands: &[],
        opens: &[],
        links: &[],
    };
    assert_eq!(
        android_permissions(&motion),
        vec!["android.permission.HIGH_SAMPLING_RATE_SENSORS"]
    );
    assert!(android_permissions_up_to(&motion).is_empty());
    assert!(json(&motion).contains("{ \"name\": \"device-motion\", \"reason\": null }"));
    assert!(rust_source(&motion).contains("Use::device_motion(),"));
    let usage = apple_usage(&motion);
    assert!(!usage.contains("NSMotionUsageDescription"), "{usage}");
}

#[test]
fn an_application_that_does_not_declare_device_motion_asks_for_no_sensor_rate() {
    assert!(!android_manifest(&NONE).contains("HIGH_SAMPLING_RATE_SENSORS"));
}

const FACE: [Use; 1] = [Use::face_tracking()];
const ALL_TRACKING: [Use; 4] = [
    Use::hand_tracking(),
    Use::body_tracking(),
    Use::face_tracking(),
    Use::eye_tracking(),
];

#[test]
fn face_tracking_names_both_headset_permissions_and_the_microphone_for_audio() {
    let face = Capabilities {
        uses: &FACE,
        demands: &[],
        opens: &[],
        links: &[],
    };
    assert_eq!(
        android_permissions(&face),
        vec![
            "android.permission.FACE_TRACKING",
            "android.permission.RECORD_AUDIO",
            "com.oculus.permission.FACE_TRACKING",
            "horizonos.permission.FACE_TRACKING",
        ]
    );
    let manifest = android_manifest(&face);
    assert!(
        manifest.contains(
            "<uses-feature android:name=\"oculus.software.face_tracking\" android:required=\"false\" />"
        ),
        "{manifest}"
    );
    assert!(json(&face).contains("{ \"name\": \"face-tracking\", \"reason\": null }"));
    assert!(
        json(&face).contains("\"features\": [\n    \"oculus.software.face_tracking\"\n  ]"),
        "{}",
        json(&face)
    );
    assert!(rust_source(&face).contains("Use::face_tracking(),"));
}

#[test]
fn every_tracking_permission_group_is_a_permission_the_manifest_declares() {
    let all = Capabilities {
        uses: &ALL_TRACKING,
        demands: &[],
        opens: &[],
        links: &[],
    };
    let declared = android_permissions(&all);
    for entry in ALL_TRACKING {
        let groups = entry.service().android_permission_groups();
        assert!(!groups.is_empty(), "{:?}", entry.service());
        for permission in groups.iter().flat_map(|group| group.iter()) {
            assert!(
                declared.contains(permission),
                "{permission} of {:?}",
                entry.service()
            );
        }
    }
    for permission in &declared {
        assert!(
            ALL_TRACKING.iter().any(|entry| entry
                .service()
                .android_permission_groups()
                .iter()
                .flat_map(|group| group.iter())
                .any(|named| named == permission)),
            "{permission} belongs to no group"
        );
    }
}

#[test]
fn an_application_that_declares_no_tracking_names_no_tracking_permission_or_feature() {
    let manifest = android_manifest(&NONE);
    for name in [
        "TRACKING",
        "handtracking",
        "eye_tracking",
        "face_tracking",
        "body_tracking",
    ] {
        assert!(!manifest.contains(name), "{name}");
    }
    assert!(android_features(&NONE).is_empty());
    assert!(
        Service::Camera.android_permission_groups().is_empty(),
        "services that ask nothing at run time have no group"
    );
}

const ROOM: [Use; 2] = [Use::anchors(), Use::plane_tracking()];

#[test]
fn anchors_name_the_meta_permission_that_lists_the_extensions_and_the_android_xr_one_for_storage() {
    let anchors = Capabilities {
        uses: &ROOM[..1],
        demands: &[],
        opens: &[],
        links: &[],
    };
    assert_eq!(
        android_permissions(&anchors),
        vec![
            "android.permission.SCENE_UNDERSTANDING_COARSE",
            "com.oculus.permission.USE_ANCHOR_API",
            "horizonos.permission.USE_ANCHOR_API",
        ]
    );
    assert_eq!(
        Service::Anchors.android_permission_groups(),
        &[&["android.permission.SCENE_UNDERSTANDING_COARSE"][..]]
    );
    assert!(json(&anchors).contains("{ \"name\": \"anchors\", \"reason\": null }"));
    assert!(rust_source(&anchors).contains("Use::anchors(),"));
}

#[test]
fn plane_tracking_asks_the_user_for_scene_data_and_lists_the_scene_extension_with_the_anchor_permission()
 {
    let planes = Capabilities {
        uses: &ROOM[1..],
        demands: &[],
        opens: &[],
        links: &[],
    };
    let permissions = android_permissions(&planes);
    for wanted in [
        "android.permission.SCENE_UNDERSTANDING_COARSE",
        "com.oculus.permission.USE_SCENE",
        "horizonos.permission.USE_SCENE",
        "com.oculus.permission.USE_ANCHOR_API",
        "horizonos.permission.USE_ANCHOR_API",
    ] {
        assert!(permissions.contains(&wanted), "{wanted} in {permissions:?}");
    }
    assert_eq!(
        Service::PlaneTracking.android_permission_groups(),
        &[&[
            "com.oculus.permission.USE_SCENE",
            "horizonos.permission.USE_SCENE",
            "android.permission.SCENE_UNDERSTANDING_COARSE",
        ][..]]
    );
    assert!(json(&planes).contains("{ \"name\": \"plane-tracking\", \"reason\": null }"));
}

#[test]
fn an_application_that_declares_neither_asks_for_no_room_permission() {
    let manifest = android_manifest(&NONE);
    assert!(!manifest.contains("USE_ANCHOR_API"), "{manifest}");
    assert!(!manifest.contains("USE_SCENE"), "{manifest}");
    assert!(!manifest.contains("SCENE_UNDERSTANDING"), "{manifest}");
}

const MODELS: [Use; 1] = [Use::controller_models()];

#[test]
fn controller_models_name_the_quest_permission_and_feature_and_ask_nobody() {
    let models = Capabilities {
        uses: &MODELS,
        demands: &[],
        opens: &[],
        links: &[],
    };
    assert_eq!(
        android_permissions(&models),
        vec!["com.oculus.permission.RENDER_MODEL"]
    );
    assert!(
        Service::ControllerModels
            .android_permission_groups()
            .is_empty()
    );
    let manifest = android_manifest(&models);
    assert!(
        manifest.contains(
            "<uses-feature android:name=\"com.oculus.feature.RENDER_MODEL\" android:required=\"false\" />"
        ),
        "{manifest}"
    );
    assert!(json(&models).contains("{ \"name\": \"controller-models\", \"reason\": null }"));
    assert!(rust_source(&models).contains("Use::controller_models(),"));
}

#[test]
fn an_application_that_declares_no_controller_models_names_no_render_model_feature() {
    let manifest = android_manifest(&NONE);
    assert!(!manifest.contains("RENDER_MODEL"), "{manifest}");
}
