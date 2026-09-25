use std::{fs, path::Path};

use super::{
    inline_test_modules, undeclared_integration_roots, unlinked_integration_tests,
    unlisted_robot_runners,
};

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

fn workspace_with_test_targets(targets: &str) -> tempfile::TempDir {
    let root = workspace_with_crate(Some(false));
    write(
        &root.path().join("crates/probe/Cargo.toml"),
        &format!("[package]\nname = \"probe\"\nautotests = false\n\n{targets}"),
    );
    root
}

#[test]
fn the_real_workspace_declares_every_integration_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("resolve the workspace root");

    let undeclared = undeclared_integration_roots(&root).expect("scan the workspace");

    assert!(
        undeclared.is_empty(),
        "these integration roots are compiled by nothing; declare each as a [[test]] target \
         of its crate's Cargo.toml:\n{undeclared:#?}"
    );
}

#[test]
fn an_integration_root_without_its_test_target_is_reported() {
    for targets in [
        "",
        "[[test]]\nname = \"allocations\"\npath = \"tests/allocations/main.rs\"\n",
        "[[test]]\nname = \"integration\"\npath = \"tests/other.rs\"\n",
    ] {
        let root = workspace_with_test_targets(targets);

        let undeclared = undeclared_integration_roots(root.path()).expect("scan the fixture");

        assert_eq!(
            undeclared,
            vec![root.path().join("crates/probe/tests/integration.rs")],
            "with targets:\n{targets}"
        );
    }
}

#[test]
fn a_declared_integration_root_passes() {
    for targets in [
        "[[test]]\nname = \"integration\"\npath = \"tests/integration.rs\"\n",
        "[[test]]\nname = \"integration\"\n",
        "[[test]]\nname = \"allocations\"\npath = \"tests/allocations/main.rs\"\n\n\
         [[test]]\nname = \"everything\"\npath = \"tests/integration.rs\"\n",
    ] {
        let root = workspace_with_test_targets(targets);

        let undeclared = undeclared_integration_roots(root.path()).expect("scan the fixture");

        assert!(
            undeclared.is_empty(),
            "with targets:\n{targets}\n{undeclared:?}"
        );
    }
}

#[test]
fn a_crate_that_discovers_its_own_tests_needs_no_integration_target() {
    let root = workspace_with_crate(None);

    let undeclared = undeclared_integration_roots(root.path()).expect("scan the fixture");

    assert!(undeclared.is_empty(), "{undeclared:?}");
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

fn workspace_with_source(path: &str, source: &str) -> tempfile::TempDir {
    let root = workspace_with_crate(None);
    write(&root.path().join("crates/probe").join(path), source);
    root
}

fn inline_lines(root: &tempfile::TempDir) -> Vec<(String, usize)> {
    inline_test_modules(root.path())
        .expect("scan the fixture")
        .into_iter()
        .map(|(path, line)| {
            let relative = path
                .strip_prefix(root.path())
                .expect("a path inside the fixture");
            (relative.display().to_string(), line)
        })
        .collect()
}

#[test]
fn the_real_workspace_keeps_no_test_module_inline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("resolve the workspace root");

    let inline = inline_test_modules(&root).expect("scan the workspace");

    assert!(
        inline.is_empty(),
        "these test modules are inline; move each into a tests/ folder beside its file \
         with scripts/dev/move_inline_tests.py:\n{inline:#?}"
    );
}

#[test]
fn an_inline_test_module_is_reported_at_its_line() {
    let root = workspace_with_source(
        "src/lib.rs",
        "pub fn answer() -> u8 {\n    42\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn answers() {}\n}\n",
    );

    assert_eq!(
        inline_lines(&root),
        vec![("crates/probe/src/lib.rs".to_owned(), 6)]
    );
}

#[test]
fn gated_nested_and_multi_line_gated_test_modules_are_reported() {
    let root = workspace_with_source(
        "src/lib.rs",
        "#[cfg(all(test, feature = \"extra\"))]\nmod extra_tests {\n}\n\nmod outer {\n    #[cfg(test)]\n    pub(crate) mod tests {\n    }\n}\n\n#[cfg(all(\n    test,\n    not(feature = \"extra\")\n))]\n/// Runs without the extra feature.\nmod plain_tests {\n}\n",
    );

    assert_eq!(
        inline_lines(&root),
        vec![
            ("crates/probe/src/lib.rs".to_owned(), 2),
            ("crates/probe/src/lib.rs".to_owned(), 7),
            ("crates/probe/src/lib.rs".to_owned(), 16),
        ]
    );
}

#[test]
fn declared_modules_other_gates_and_test_folders_pass() {
    let root = workspace_with_source(
        "src/lib.rs",
        "#[cfg(test)]\n#[path = \"tests/lib_tests.rs\"]\nmod tests;\n\nmod plain {\n}\n\n#[cfg(feature = \"latest\")]\nmod contest {\n}\n\n#[cfg_attr(test, allow(dead_code))]\nmod helpers {\n}\n\n#[cfg(not(test))]\nmod production {\n}\n\n#[cfg(any(test, feature = \"robot\"))]\nmod shared {\n}\n",
    );
    write(
        &root.path().join("crates/probe/src/tests/lib_tests.rs"),
        "#[cfg(test)]\nmod nested {\n}\n",
    );

    assert!(inline_lines(&root).is_empty(), "{:?}", inline_lines(&root));
}
