use crate::{
    Capabilities, Use, android_manifest, android_permissions, android_permissions_up_to,
    apple_usage, json, rust_source,
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
