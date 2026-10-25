use std::{
    fs,
    path::{Path, PathBuf},
};

const PARSER_TEST: &str = "apps/android-demo/android/app/src/androidTest/java/com/compose_rs/demo/CranposeAccessibilityParserTest.java";
const APPLICATION_MANIFEST: &str = "apps/android-demo/android/app/src/main/AndroidManifest.xml";

fn copy_files(real: &Path, directory: &str, extensions: &[&str], out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(real.join(directory)).expect("a directory the checks read") {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if path.is_dir() && !matches!(name, "build" | ".gradle") {
            let relative = path.strip_prefix(real).expect("a path under the workspace");
            copy_files(real, &relative.to_string_lossy(), extensions, out);
        } else if extensions.iter().any(|extension| name.ends_with(extension)) {
            out.push(path);
        }
    }
}

// A copy of the files the checks read, with one edit made after the first
// occurrence of `after`; the problems `cargo xtask platform-contracts` names.
fn problems_after_edit(file: &str, after: &str, from: &str, to: &str) -> String {
    let real = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("the workspace root");
    let copy = tempfile::tempdir().expect("a temporary workspace");
    let mut files = Vec::new();
    copy_files(&real, ".github/workflows", &[".yml"], &mut files);
    copy_files(
        &real,
        "crates/cranpose/android",
        &[".java", ".xml", ".kt"],
        &mut files,
    );
    files.extend(
        [
            "scripts/ci/android_env.sh",
            "crates/cranpose/src/tests/android_accessibility_wire.rs",
            PARSER_TEST,
            APPLICATION_MANIFEST,
            "apps/isolated-demo/android/app/src/main/AndroidManifest.xml",
        ]
        .map(|relative| real.join(relative)),
    );
    for path in files {
        let target = copy.path().join(
            path.strip_prefix(&real)
                .expect("a file under the workspace"),
        );
        fs::create_dir_all(target.parent().expect("a parent directory"))
            .expect("the copy's directory");
        fs::copy(&path, &target).expect("copy a file the checks read");
    }

    let path = copy.path().join(file);
    let text = fs::read_to_string(&path).expect("the file to edit");
    let start = text
        .find(after)
        .unwrap_or_else(|| panic!("{file} has no {after:?}"));
    let at = start
        + text[start..]
            .find(from)
            .unwrap_or_else(|| panic!("{file} has no {from:?} after {after:?}; pick another edit"));
    let edited = format!("{}{to}{}", &text[..at], &text[at + from.len()..]);
    fs::write(&path, edited).expect("write the edited file");
    crate::platform_contracts(copy.path()).expect_err("the edit breaks a contract")
}

// Each edit breaks one rule: the file, the text the edit comes after, the text
// it replaces, its replacement, and a part of the problem that names the rule.
const EDITS: [(&str, &str, &str, &str, &str); 19] = [
    (
        ".github/workflows/rust.yml",
        "concurrency:",
        "cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}",
        "cancel-in-progress: true",
        "rust.yml: cancel-in-progress",
    ),
    (
        ".github/workflows/nightly.yml",
        "env:",
        "GIT_CONFIG_KEY_0: core.fsmonitor",
        "GIT_CONFIG_KEY_0: core.editor",
        "nightly.yml runs on a self-hosted machine",
    ),
    (
        ".github/workflows/publish.yml",
        "  bump_isolated_demo:",
        "ref: ${{ github.event.repository.default_branch }}",
        "ref: ${{ github.ref_name }}",
        "bump_isolated_demo commits on the default branch",
    ),
    (
        ".github/workflows/release.yml",
        "  build-windows:",
        "fail_on_unmatched_files: true",
        "fail_on_unmatched_files: false",
        "must set fail_on_unmatched_files: true",
    ),
    (
        ".github/workflows/release.yml",
        "  build-windows:",
        "files: cranpose-demo-windows-x86_64.zip",
        "files: \"*.zip\"",
        "no longer uploads `cranpose-demo-windows-x86_64.zip`",
    ),
    (
        ".github/workflows/release.yml",
        "  build-android:",
        "ref: ${{ needs.create-release.outputs.tag }}",
        "ref: ${{ github.event.workflow_run.head_branch }}",
        "build-android must take the tag from create-release",
    ),
    (
        ".github/workflows/release.yml",
        "  build:",
        "needs: create-release",
        "needs: []",
        "build must need create-release",
    ),
    (
        ".github/workflows/nightly.yml",
        "  msrv:",
        "if: needs.decide.outputs.run == 'true'",
        "if: always()",
        "msrv must need decide",
    ),
    (
        ".github/workflows/rust.yml",
        "jobs:",
        "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "uses: actions/checkout@v7",
        "`actions/checkout@v7` must be pinned",
    ),
    (
        ".github/workflows/heavy-selfhosted.yml",
        "  android-apk:",
        "if: github.event_name != 'pull_request'",
        "if: always()",
        "android-apk must not run on a pull request",
    ),
    (
        ".github/workflows/nightly.yml",
        "  robot-suite:",
        "run: just robot-linux serial",
        "run: just robot-linux",
        "the whole robot suite runs again",
    ),
    (
        "scripts/ci/android_env.sh",
        "ANDROID_NDK_HOME",
        "/ndk/",
        "/ndk/1",
        "must install that version",
    ),
    (
        PARSER_TEST,
        "String KINDS",
        "\"NNNN",
        "\"NNNF",
        "the accessibility record layout differs",
    ),
    (
        "crates/cranpose/android/java/dev/cranpose/android/CranposeActivity.java",
        "private void askForCranposeBackgroundService()",
        "if (!cranposeEverResumed)",
        "if (false)",
        "the ask must wait for a lifetime that reached the foreground",
    ),
    (
        "crates/cranpose/android/java/dev/cranpose/android/CranposeBackgroundService.java",
        "static void stop(Context context)",
        "if (obligationArmed)",
        "if (true)",
        "stop must wait for startForeground",
    ),
    (
        "crates/cranpose/android/java-billing/dev/cranpose/android/CranposeBilling.java",
        "private int apply(",
        "orders.clear()",
        "orders.size()",
        "replace ownership and order ids together",
    ),
    (
        "crates/cranpose/android/manifests/base.xml",
        "<receiver",
        "android:exported=\"false\"",
        "android:exported=\"true\"",
        "base.xml declares a receiver other applications can reach",
    ),
    (
        APPLICATION_MANIFEST,
        "<application",
        ">",
        "><activity android:name=\"dev.cranpose.android.CranposeActivity\" />",
        "declares the framework activity",
    ),
    (
        "crates/cranpose/android/java/dev/cranpose/android/CranposeActivity.java",
        "protected void onResume()",
        "{",
        "{ registerReceiver(receiver, filter);",
        "registers a receiver at run time",
    ),
];

#[test]
fn breaking_a_rule_names_it() {
    for (file, after, from, to, expected) in EDITS {
        let found = problems_after_edit(file, after, from, to);
        assert!(
            found.contains(expected),
            "editing {from:?} to {to:?} in {file} should report {expected:?}, reported {found}"
        );
    }
}
