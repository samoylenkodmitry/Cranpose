use std::{fs, path::PathBuf, process::Command};

use cranpose_capabilities::{Capabilities, Use, apple_usage};

const PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\
    \t<key>CFBundleName</key>\n\t<string>App</string>\n\
    \t<key>CFBundleURLTypes</key>\n\t<array>\n\t\t<dict>\n\t\t\t<key>CFBundleURLSchemes</key>\n\
    \t\t\t<array>\n\t\t\t\t<string>old</string>\n\t\t\t</array>\n\t\t</dict>\n\t</array>\n\
    \t<key>UILaunchScreen</key>\n\t<dict/>\n\
    </dict>\n</plist>\n";

const CAMERA: [Use; 1] = [Use::camera("Reads receipts.")];

/// Merges what `capabilities` declares into [`PLIST`] with the
/// `cranpose-plist` binary, in a directory of the test's own.
fn merged(name: &str, capabilities: &Capabilities<'_>) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    fs::create_dir_all(&dir).expect("a scratch directory");
    let (usage, info) = (dir.join("usage.plist"), dir.join("Info.plist"));
    fs::write(&usage, apple_usage(capabilities)).expect("the usage file");
    fs::write(&info, PLIST).expect("the property list");
    let status = Command::new(env!("CARGO_BIN_EXE_cranpose-plist"))
        .arg("--usage")
        .arg(&usage)
        .arg("--into")
        .arg(&info)
        .status()
        .expect("cranpose-plist runs");
    assert!(status.success());
    fs::read_to_string(&info).expect("the merged list")
}

#[test]
fn link_schemes_replace_the_url_types_and_keep_the_rest() {
    let text = merged(
        "links",
        &Capabilities {
            uses: &CAMERA,
            demands: &[],
            opens: &[],
            links: &["cranpose-demo"],
        },
    );
    assert_eq!(
        text.matches("<key>CFBundleURLTypes</key>").count(),
        1,
        "{text}"
    );
    assert!(
        text.contains("\t\t\t\t<string>cranpose-demo</string>\n"),
        "{text}"
    );
    assert!(!text.contains("old"), "{text}");
    assert!(
        !text.contains("<key>CFBundleURLSchemes</key>\n\t<string>"),
        "a nested key never becomes a top-level string: {text}"
    );
    assert!(text.contains(
        "<key>NSCameraUsageDescription</key>\n\t<string>Reads receipts.</string>\n</dict>"
    ));
    assert!(text.contains("<key>CFBundleName</key>\n\t<string>App</string>"));
    assert!(text.contains("<key>UILaunchScreen</key>\n\t<dict/>"));
}

#[test]
fn a_declaration_without_links_leaves_the_url_types_alone() {
    let text = merged(
        "sentence",
        &Capabilities {
            uses: &CAMERA,
            demands: &[],
            opens: &[],
            links: &[],
        },
    );
    assert_eq!(text.matches("NSCameraUsageDescription").count(), 1);
    assert!(text.contains("<string>old</string>"), "{text}");
}
