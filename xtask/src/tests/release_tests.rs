use std::{collections::BTreeSet, fs, path::Path};

use super::{
    bump_release_version_at, check_root_lock_versions, resolve_publish_order, unique_temp_dir,
};

fn write_member(root: &Path, dir: &str, name: &str, version_line: &str) {
    let member = root.join(dir);
    fs::create_dir_all(&member).expect("create member");
    fs::write(
        member.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\n{version_line}\n"),
    )
    .expect("write member manifest");
}

fn write_workspace(root: &Path, version: &str, lock_version: &str) {
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\n\
             members = [\"crates/coroflow\", \"apps/demo\"]\n\
             \n\
             [workspace.package]\n\
             version = \"{version}\"\n\
             \n\
             [workspace.dependencies]\n\
             log = \"0.4\"\n"
        ),
    )
    .expect("write root manifest");
    write_member(
        root,
        "crates/coroflow",
        "coroflow",
        "version.workspace = true",
    );
    write_member(
        root,
        "apps/demo",
        "demo",
        &format!("version = \"{version}\""),
    );
    fs::write(
        root.join("Cargo.lock"),
        format!(
            "version = 4\n\n\
             [[package]]\nname = \"coroflow\"\nversion = \"{lock_version}\"\n\n\
             [[package]]\nname = \"demo\"\nversion = \"{version}\"\n\n\
             [[package]]\nname = \"log\"\nversion = \"0.4.0\"\n"
        ),
    )
    .expect("write root lock");
}

#[test]
fn a_release_bumps_every_member_that_inherits_the_workspace_version() {
    let root = unique_temp_dir();
    write_workspace(&root, "0.1.104", "0.1.104");

    bump_release_version_at(&root, "v0.1.105").expect("bump must succeed");

    let lock = fs::read_to_string(root.join("Cargo.lock")).expect("read lock");
    assert!(
        lock.contains("name = \"coroflow\"\nversion = \"0.1.105\""),
        "a member inheriting the workspace version follows it: {lock}"
    );
    assert!(
        lock.contains("name = \"demo\"\nversion = \"0.1.104\""),
        "a member with a version of its own keeps it: {lock}"
    );
    assert!(
        lock.contains("name = \"log\"\nversion = \"0.4.0\""),
        "a dependency is left alone: {lock}"
    );
}

#[test]
fn a_release_bumps_every_release_crate_in_workspace_dependencies() {
    let root = unique_temp_dir();
    write_workspace(&root, "0.1.104", "0.1.104");
    let manifest = root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).expect("read root manifest");
    fs::write(
        &manifest,
        text.replace(
            "log = \"0.4\"\n",
            "log = \"0.4\"\n\
             coroflow = { path = \"crates/coroflow\", version = \"0.1.104\" }\n\
             cranpose-coroflow = { path = \"crates/cranpose-coroflow\", version = \"0.1.104\" }\n",
        ),
    )
    .expect("write root manifest");

    bump_release_version_at(&root, "v0.1.105").expect("bump must succeed");

    let manifest = fs::read_to_string(&manifest).expect("read root manifest");
    for entry in [
        r#"coroflow = { path = "crates/coroflow", version = "0.1.105" }"#,
        r#"cranpose-coroflow = { path = "crates/cranpose-coroflow", version = "0.1.105" }"#,
    ] {
        assert!(
            manifest.contains(entry),
            "missing `{entry}` in:\n{manifest}"
        );
    }
    assert!(
        manifest.contains("log = \"0.4\""),
        "a third-party dependency is left alone: {manifest}"
    );
}

#[test]
fn the_root_lock_check_reports_a_member_left_at_an_old_version() {
    let root = unique_temp_dir();
    write_workspace(&root, "0.1.105", "0.1.104");
    let mut failures = Vec::new();

    check_root_lock_versions(&root, &BTreeSet::new(), "0.1.105", &mut failures)
        .expect("the lock parses");

    assert_eq!(
        failures,
        vec!["Cargo.lock package coroflow has 0.1.104, expected 0.1.105".to_owned()]
    );
}

#[test]
fn the_publish_order_leaves_out_crates_that_are_not_published() {
    let metadata = r#"{
        "workspace_members": ["cranpose-core 0.1.0", "cranpose-coroflow 0.1.0", "cranpose 0.1.0"],
        "packages": [
            {"id": "cranpose-core 0.1.0", "name": "cranpose-core", "dependencies": []},
            {"id": "cranpose-coroflow 0.1.0", "name": "cranpose-coroflow", "publish": [],
             "dependencies": [{"name": "cranpose-core", "kind": null}]},
            {"id": "cranpose 0.1.0", "name": "cranpose", "publish": null,
             "dependencies": [{"name": "cranpose-core", "kind": null}]}
        ]
    }"#;

    let order = resolve_publish_order(metadata).expect("the order resolves");

    assert_eq!(
        order,
        vec!["cranpose-core".to_owned(), "cranpose".to_owned()]
    );
}

#[test]
fn coroflow_is_published_before_the_crates_that_use_it() {
    let metadata = r#"{
        "workspace_members": ["cranpose-coroflow 0.1.0", "coroflow 0.1.0", "coroflow-demo 0.1.0"],
        "packages": [
            {"id": "cranpose-coroflow 0.1.0", "name": "cranpose-coroflow",
             "dependencies": [{"name": "coroflow", "kind": null}]},
            {"id": "coroflow 0.1.0", "name": "coroflow", "dependencies": []},
            {"id": "coroflow-demo 0.1.0", "name": "coroflow-demo",
             "dependencies": [{"name": "cranpose-coroflow", "kind": null}]}
        ]
    }"#;

    let order = resolve_publish_order(metadata).expect("the order resolves");

    assert_eq!(
        order,
        vec!["coroflow".to_owned(), "cranpose-coroflow".to_owned()],
        "coroflow is released with Cranpose; other crates are not"
    );
}
