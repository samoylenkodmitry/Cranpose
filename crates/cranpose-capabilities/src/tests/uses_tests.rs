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
