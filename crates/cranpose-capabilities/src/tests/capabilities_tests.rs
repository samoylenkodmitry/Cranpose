use super::*;

const READS: &str = "Reads a receipt with the camera.";

const SCANNER: Capabilities<'static> = Capabilities {
    uses: &[Use::camera(READS), Use::notifications(), Use::billing()],
    demands: &[],
    opens: &[],
};

#[test]
fn a_service_with_a_sentence_carries_it() {
    assert_eq!(SCANNER.reason_for(Service::Camera), Some(READS));
    assert_eq!(SCANNER.reason_for(Service::Notifications), None);
    assert!(SCANNER.has(Service::Billing));
    assert!(!SCANNER.has(Service::Haptics));
}

const BOTH_SERVICES: [Use; 2] = [Use::background(), Use::media()];
const ONE_HAPTIC: [Use; 1] = [Use::haptics()];
const WATCH: [Demand; 1] = [Demand::Watch];
const ONE_CAMERA: [Use; 1] = [Use::camera(READS)];
const CAMERA_HARDWARE: [Demand; 1] = [Demand::Camera];

#[test]
fn android_permissions_come_from_the_services_without_repeats() {
    let both = Capabilities {
        uses: &BOTH_SERVICES,
        demands: &[],
        opens: &[],
    };
    assert_eq!(
        android_permissions(&both),
        vec![
            "android.permission.FOREGROUND_SERVICE",
            "android.permission.FOREGROUND_SERVICE_DATA_SYNC",
            "android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK",
        ]
    );
}

#[test]
fn the_manifest_names_the_permissions_and_the_demanded_hardware() {
    let watch = Capabilities {
        uses: &ONE_HAPTIC,
        demands: &WATCH,
        opens: &[],
    };
    let text = android_manifest(&watch);
    assert!(text.contains("<uses-permission android:name=\"android.permission.VIBRATE\" />"));
    assert!(text.contains(
        "<uses-feature android:name=\"android.hardware.type.watch\" android:required=\"true\" />"
    ));
}

#[test]
fn the_build_names_where_the_declaration_goes() {
    let named = shared_dir(
        Path::new("/checkout/crates/app"),
        Some(OsString::from("/checkout/target/cranpose")),
    );
    assert_eq!(named, PathBuf::from("/checkout/target/cranpose"));
}

#[test]
fn an_empty_name_is_no_name() {
    let crate_dir = Path::new("/checkout/crates/app");
    assert_eq!(
        shared_dir(crate_dir, Some(OsString::new())),
        shared_dir(crate_dir, None)
    );
}

#[test]
fn apple_takes_only_the_services_it_shows_a_sentence_for() {
    let text = apple_usage(&SCANNER);
    assert!(text.starts_with("<?xml"));
    assert!(text.contains("<key>NSCameraUsageDescription</key>"));
    assert!(text.contains(READS));
    assert!(text.ends_with("</dict>\n</plist>\n"));
    assert!(!text.contains("Notification"));
}

#[test]
fn the_json_lists_services_permissions_and_demands() {
    let text = json(&Capabilities {
        uses: &ONE_CAMERA,
        demands: &CAMERA_HARDWARE,
        opens: &[],
    });
    assert!(text.contains("\"name\": \"camera\""));
    assert!(text.contains("\"android.permission.CAMERA\""));
    assert!(text.contains("\"android.hardware.camera\""));
}

#[test]
fn the_generated_rust_rebuilds_the_same_declaration() {
    let text = rust_source(&SCANNER);
    assert!(text.contains("Use::camera(\"Reads a receipt with the camera.\")"));
    assert!(text.contains("Use::notifications()"));
    assert!(text.contains("demands: &[\n        ],"));
}

#[test]
fn a_quote_in_a_sentence_survives_the_generated_rust() {
    let text = constructor(&Use::camera("Reads a \"receipt\"."));
    assert_eq!(text, "camera(\"Reads a \\\"receipt\\\".\")");
}

#[test]
fn the_workspace_is_the_tree_the_platform_builds_know() {
    let crate_dir = Path::new("/w/app");
    let holds = |path: &Path| path == Path::new("/w");
    assert_eq!(found_workspace(crate_dir, &holds), Path::new("/w"));
}

#[test]
fn a_workspace_inside_a_workspace_answers_with_the_outer_one() {
    let crate_dir = Path::new("/w/vendor/thing/crates/one");
    let holds = |path: &Path| path == Path::new("/w") || path == Path::new("/w/vendor/thing");
    assert_eq!(found_workspace(crate_dir, &holds), Path::new("/w"));
}

#[test]
fn a_crate_in_no_workspace_answers_with_itself() {
    let crate_dir = Path::new("/w/single");
    assert_eq!(
        found_workspace(crate_dir, &|_| false),
        Path::new("/w/single")
    );
}

#[test]
fn the_declaration_is_three_files_beside_the_generated_source() {
    let outputs = shared_outputs(Path::new("/w/target/cranpose"), "desktop-app-platform");
    let names: Vec<String> = outputs
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "desktop-app-platform-capabilities.json",
            "desktop-app-platform-permissions.xml",
            "desktop-app-platform-usage.plist",
        ]
    );
}

#[test]
fn every_file_written_outside_out_dir_is_named_to_cargo() {
    let outputs = shared_outputs(Path::new("/w/target/cranpose"), "example");
    let directives = rerun_directives(&outputs);
    assert_eq!(
        directives.len(),
        outputs.len(),
        "a file cargo is not told about is a file nothing will rewrite once it is gone"
    );
    for path in &outputs {
        let expected = format!("cargo::rerun-if-changed={}", path.display());
        assert!(
            directives.contains(&expected),
            "{} is written but never named to cargo: {directives:?}",
            path.display()
        );
    }
}

/// The coupling is the guard: a file written outside [`shared_outputs`]
/// would not appear in [`rerun_directives`] either, and deleting it would
/// break the Android manifest check permanently in that workspace.
#[test]
fn emit_writes_the_shared_files_only_through_the_named_list() {
    let source = include_str!("../lib.rs");
    let emit = source
        .split_once("pub fn emit(self)")
        .expect("emit is still a function")
        .1
        .split_once("\n}")
        .expect("emit still ends")
        .0;
    for name in ["-capabilities.json", "-permissions.xml", "-usage.plist"] {
        assert!(
            !emit.contains(name),
            "emit names {name} directly instead of going through shared_outputs, \
             so cargo is never told the file exists"
        );
    }
    assert!(
        emit.contains("shared_outputs(&shared, &package)")
            && emit.contains("rerun_directives(&outputs)"),
        "emit must write and announce the same list of files"
    );
}

const AUDIO: [&str; 2] = ["audio/*", "application/ogg"];

const PLAYER: Capabilities<'static> = Capabilities {
    uses: &[],
    demands: &[],
    opens: &AUDIO,
};

#[test]
fn an_application_that_opens_files_offers_the_activity_for_them() {
    let text = android_manifest(&PLAYER);
    assert!(text.contains("<activity android:name=\"dev.cranpose.android.CranposeActivity\">"));
    for action in [
        "android.intent.action.SEND",
        "android.intent.action.SEND_MULTIPLE",
        "android.intent.action.VIEW",
    ] {
        assert!(
            text.contains(&format!("<action android:name=\"{action}\" />")),
            "{action}: {text}"
        );
    }
    assert_eq!(
        text.matches("<data android:mimeType=\"audio/*\" />")
            .count(),
        2
    );
    assert_eq!(
        text.matches("<category android:name=\"android.intent.category.DEFAULT\" />")
            .count(),
        2
    );
}

#[test]
fn an_application_that_opens_nothing_adds_no_activity_entry() {
    assert!(!android_manifest(&SCANNER).contains("<activity"));
}

#[test]
fn the_json_and_the_generated_rust_carry_what_the_application_opens() {
    let text = json(&PLAYER);
    assert!(text.contains("\"opens\": [\n    \"audio/*\",\n    \"application/ogg\"\n  ]"));
    let rust = rust_source(&PLAYER);
    assert!(rust.contains(
        "opens: &[\n            \"audio/*\",\n            \"application/ogg\",\n        ],"
    ));
}

#[test]
fn opening_keeps_what_the_declaration_already_used_and_demanded() {
    let declaration = declare(&ONE_HAPTIC).demanding(&WATCH).opening(&AUDIO);
    assert_eq!(declaration.capabilities.uses, &ONE_HAPTIC);
    assert_eq!(declaration.capabilities.demands, &WATCH);
    assert_eq!(declaration.capabilities.opens, &AUDIO);
}
