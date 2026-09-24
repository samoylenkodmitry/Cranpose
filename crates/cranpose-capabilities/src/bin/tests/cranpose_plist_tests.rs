use super::*;

const PLIST: &str = "<plist version=\"1.0\">\n<dict>\n\t<key>CFBundleName</key>\n\t<string>App</string>\n</dict>\n</plist>\n";

#[test]
fn a_usage_file_reads_as_key_and_value() {
    let text = "\t<key>NSCameraUsageDescription</key>\n\t<string>Reads receipts.</string>\n";
    assert_eq!(
        pairs(text),
        vec![(
            String::from("NSCameraUsageDescription"),
            String::from("Reads receipts.")
        )]
    );
}

#[test]
fn a_new_key_lands_before_the_closing_tag() {
    let merged = merge(
        PLIST,
        &[(
            String::from("NSCameraUsageDescription"),
            String::from("Reads receipts."),
        )],
    );
    assert!(merged.contains(
        "<key>NSCameraUsageDescription</key>\n\t<string>Reads receipts.</string>\n</dict>"
    ));
    assert!(merged.contains("<key>CFBundleName</key>"));
}

#[test]
fn a_key_the_list_holds_takes_the_new_sentence() {
    let once = merge(
        PLIST,
        &[(
            String::from("NSCameraUsageDescription"),
            String::from("First."),
        )],
    );
    let twice = merge(
        &once,
        &[(
            String::from("NSCameraUsageDescription"),
            String::from("Second."),
        )],
    );
    assert_eq!(twice.matches("NSCameraUsageDescription").count(), 1);
    assert!(twice.contains("<string>Second.</string>"));
    assert!(!twice.contains("First."));
}
