use std::{fs, path::Path};

use super::{unlinked_integration_tests, unlisted_robot_runners};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture directory");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn workspace_with_crate(autotests: Option<bool>) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("fixture workspace");
    write(
        &root.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/probe\"]\n",
    );
    let autotests_line = autotests.map_or(String::new(), |value| format!("autotests = {value}\n"));
    write(
        &root.path().join("crates/probe/Cargo.toml"),
        &format!("[package]\nname = \"probe\"\n{autotests_line}"),
    );
    write(
        &root.path().join("crates/probe/tests/integration.rs"),
        "#[path = \"../src/shared.rs\"]\nmod shared;\nmod support;\n\nmod linked;\n",
    );
    write(
        &root.path().join("crates/probe/tests/linked.rs"),
        "#[test]\nfn runs() {}\n",
    );
    write(&root.path().join("crates/probe/tests/support.rs"), "");
    root
}

#[test]
fn the_real_workspace_links_every_integration_test_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("resolve the workspace root");

    let unlinked = unlinked_integration_tests(&root).expect("scan the workspace");

    assert!(
        unlinked.is_empty(),
        "these test files are compiled by nothing; add them as modules of their crate's \
         tests/integration.rs:\n{unlinked:#?}"
    );
}

#[test]
fn a_test_file_missing_from_the_integration_root_is_reported() {
    let root = workspace_with_crate(Some(false));
    write(
        &root.path().join("crates/probe/tests/orphan.rs"),
        "#[test]\nfn never_runs() {}\n",
    );

    let unlinked = unlinked_integration_tests(root.path()).expect("scan the fixture");

    assert_eq!(
        unlinked,
        vec![root.path().join("crates/probe/tests/orphan.rs")]
    );
}

#[test]
fn linked_test_files_and_shared_modules_pass() {
    let root = workspace_with_crate(Some(false));

    let unlinked = unlinked_integration_tests(root.path()).expect("scan the fixture");

    assert!(unlinked.is_empty(), "{unlinked:?}");
}

#[test]
fn a_crate_that_discovers_its_own_tests_is_not_checked() {
    let root = workspace_with_crate(None);
    write(
        &root.path().join("crates/probe/tests/standalone.rs"),
        "#[test]\nfn runs_as_its_own_binary() {}\n",
    );

    let unlinked = unlinked_integration_tests(root.path()).expect("scan the fixture");

    assert!(unlinked.is_empty(), "{unlinked:?}");
}

fn workspace_with_robot_runners(table: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("fixture workspace");
    let runners = root.path().join("apps/desktop-demo/robot-runners");
    write(&runners.join("main.rs"), table);
    write(
        &runners.join("robot_listed.rs"),
        "pub(crate) fn main() {}\n",
    );
    write(
        &runners.join("robot_helper.rs"),
        "pub(crate) fn helper() {}\n",
    );
    write(
        &runners.join("not_a_runner.rs"),
        "pub(crate) fn main() {}\n",
    );
    root
}

#[test]
fn the_real_workspace_lists_every_robot_runner() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("resolve the workspace root");

    let unlisted = unlisted_robot_runners(&root).expect("scan the robot runners");

    assert!(
        unlisted.is_empty(),
        "these robot runners are missing from the runners! table:\n{unlisted:#?}"
    );
}

#[test]
fn a_robot_runner_missing_from_the_table_is_reported() {
    let root = workspace_with_robot_runners("runners! {\n    robot_listed,\n}\n");
    let runners = root.path().join("apps/desktop-demo/robot-runners");
    write(
        &runners.join("robot_forgotten.rs"),
        "pub(crate) fn main() {}\n",
    );

    let unlisted = unlisted_robot_runners(root.path()).expect("scan the fixture");

    assert_eq!(unlisted, vec![runners.join("robot_forgotten.rs")]);
}

#[test]
fn listed_runners_helpers_and_other_files_pass() {
    let root = workspace_with_robot_runners("runners! {\n    robot_listed,\n}\n");

    let unlisted = unlisted_robot_runners(root.path()).expect("scan the fixture");

    assert!(unlisted.is_empty(), "{unlisted:?}");
}

#[test]
fn a_workspace_without_robot_runners_has_nothing_to_list() {
    let root = tempfile::tempdir().expect("fixture workspace");

    let unlisted = unlisted_robot_runners(root.path()).expect("scan the fixture");

    assert!(unlisted.is_empty(), "{unlisted:?}");
}
