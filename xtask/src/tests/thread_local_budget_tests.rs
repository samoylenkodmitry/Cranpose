use std::{fs, path::Path};

use super::{check, run_at};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture directory");
    }
    fs::write(path, contents).expect("write fixture file");
}

const DECLARATIONS: &str = r##"
use std::cell::{Cell, RefCell};

// thread_local! { static COMMENTED: Cell<u8> = Cell::new(0); }
/* thread_local! { static BLOCK_COMMENTED: Cell<u8> = Cell::new(0); } */
const TEXT: &str = "thread_local! { static QUOTED: Cell<u8> = Cell::new(0); }";
const RAW: &str = r#"thread_local! { static RAW_QUOTED: Cell<u8> = Cell::new(0); }"#;
const BRACE: char = '{';

thread_local! {
    #[cfg(debug_assertions)]
    static LABELS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    pub(crate) static DEPTH: Cell<usize> = const { Cell::new(0) };
}

std::thread_local!(static ONE_LINE: Cell<bool> = const { Cell::new(false) });

#[cfg(not(target_os = "android"))]
thread_local! {
    static PLATFORM: Cell<u8> = const { Cell::new(0) };
}

#[cfg(target_os = "android")]
thread_local! {
    static PLATFORM: Cell<u8> = const { Cell::new(1) };
}

fn nested() -> u64 {
    thread_local! {
        static NESTED: Cell<u64> = const { Cell::new(0) };
    }
    NESTED.with(Cell::get)
}
"##;

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("fixture workspace");
    write(
        &root.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/probe\", \"crates/empty\", \"apps/demo\"]\n",
    );
    write(&root.path().join("crates/probe/src/lib.rs"), DECLARATIONS);
    write(
        &root.path().join("crates/probe/src/tests/lib_tests.rs"),
        "thread_local! { static TEST_ONLY: u8 = 0; }",
    );
    write(
        &root.path().join("crates/empty/src/lib.rs"),
        "pub fn empty() {}",
    );
    write(
        &root.path().join("apps/demo/src/main.rs"),
        "thread_local! { static APP: u8 = 0; }\nfn main() {}",
    );
    root
}

#[test]
fn the_real_workspace_stays_at_its_thread_local_budget() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the workspace");
    run_at(root).expect("framework thread-local statics match the budget");
}

#[test]
fn counts_each_static_of_every_declaration_form_once_and_nothing_else() {
    let root = workspace();
    check(root.path(), 5).expect("five statics in framework sources");
}

#[test]
fn a_static_over_the_budget_fails_with_the_android_key_limit() {
    let root = workspace();
    let error = check(root.path(), 4).expect_err("five statics exceed a budget of four");
    assert!(error.contains("declare 5 statics"), "{error}");
    assert!(error.contains("pthread key"), "{error}");
    assert!(error.contains("out of TLS keys"), "{error}");
    assert!(error.contains("   5 crates/probe"), "{error}");
}

#[test]
fn a_budget_above_the_count_asks_to_be_lowered() {
    let root = workspace();
    let error = check(root.path(), 6).expect_err("a slack budget fails");
    assert!(error.contains("lower BUDGET"), "{error}");
    assert!(error.contains("to 5"), "{error}");
}
