use std::{fs, path::Path};

use super::unlinked_integration_tests;

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
