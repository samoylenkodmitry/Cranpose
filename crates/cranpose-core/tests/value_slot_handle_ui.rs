use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct TempCrate {
    dir: PathBuf,
}

impl TempCrate {
    fn new(test_name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("ui-compile-fail")
            .join(format!("{test_name}-{unique}"));
        fs::create_dir_all(dir.join("src")).expect("create temp crate directory");
        Self { dir }
    }
}

impl Drop for TempCrate {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn run_compile_fail_case(test_name: &str, fixture: &str, expected_fragments: &[&str]) {
    let temp = TempCrate::new(test_name);
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_path = crate_root.join("tests/ui").join(fixture);
    let source = fs::read_to_string(&source_path).expect("read compile-fail fixture");
    let wrapped_source = format!("pub use cranpose_core::*;\n{source}");
    let temp_source_path = temp.dir.join("case.rs");
    fs::write(&temp_source_path, wrapped_source).expect("write wrapped compile-fail source");
    let deps_dir = resolve_deps_dir();
    let core_artifact = find_artifact(&deps_dir, "libcranpose_core-", &["rlib"]);
    let macros_artifact = find_artifact(
        &deps_dir,
        "libcranpose_macros-",
        &[std::env::consts::DLL_EXTENSION],
    );

    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(rustc)
        .arg("--crate-name")
        .arg("ui_compile_fail_case")
        .arg("--edition")
        .arg("2021")
        .arg("--crate-type")
        .arg("bin")
        .arg("--out-dir")
        .arg(temp.dir.join("out"))
        .arg("-L")
        .arg(format!("dependency={}", deps_dir.display()))
        .arg("--extern")
        .arg(format!("cranpose_core={}", core_artifact.display()))
        .arg("--extern")
        .arg(format!("cranpose_macros={}", macros_artifact.display()))
        .arg(&temp_source_path)
        .output()
        .expect("run compile-fail case");

    assert!(
        !output.status.success(),
        "fixture `{fixture}` unexpectedly compiled successfully",
    );

    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for fragment in expected_fragments {
        assert!(
            diagnostics.contains(fragment),
            "fixture `{fixture}` missing expected diagnostic fragment `{fragment}`.\nfull output:\n{diagnostics}",
        );
    }
}

fn find_artifact(dir: &Path, prefix: &str, extensions: &[&str]) -> PathBuf {
    let mut candidates = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read artifact dir `{}`: {err}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| extensions.contains(&ext))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        let left_modified = fs::metadata(left)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let right_modified = fs::metadata(right)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        left_modified
            .cmp(&right_modified)
            .then_with(|| left.cmp(right))
    });
    candidates
        .pop()
        .unwrap_or_else(|| panic!("missing artifact `{prefix}*` in `{}`", dir.display()))
}

fn resolve_deps_dir() -> PathBuf {
    let current_exe = env::current_exe().expect("resolve current test executable");
    let deps_dir = current_exe
        .parent()
        .expect("current test executable has a parent directory")
        .to_path_buf();
    assert!(
        deps_dir.file_name().and_then(|name| name.to_str()) == Some("deps"),
        "test executable `{}` is not in a Cargo deps directory",
        current_exe.display()
    );
    deps_dir
}

#[test]
fn value_slot_handle_cannot_escape_composable_scope() {
    run_compile_fail_case(
        "escape_body",
        "value_slot_handle_escape_body.rs",
        &[
            "borrowed data escapes outside of closure",
            "composer.use_value_slot",
            "escapes the closure body here",
        ],
    );
    run_compile_fail_case(
        "return",
        "value_slot_handle_return.rs",
        &[
            "lifetime may not live long enough",
            "ValueSlotHandle",
            "returning this value requires that",
        ],
    );
    run_compile_fail_case(
        "state",
        "value_slot_handle_state.rs",
        &[
            "borrowed data escapes outside of closure",
            "mutableStateOf(handle)",
            "must outlive `'static`",
        ],
    );
    run_compile_fail_case(
        "write_removed",
        "value_slot_write_removed.rs",
        &["no method named `write_slot_value`"],
    );
}
