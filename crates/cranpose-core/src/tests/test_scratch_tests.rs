use super::*;

#[test]
fn a_scratch_directory_is_empty_under_the_workspace_target() {
    let path = test_scratch_dir(env!("CARGO_MANIFEST_DIR"), "scratch");
    assert!(path.is_dir());
    assert_eq!(std::fs::read_dir(&path).expect("read").count(), 0);
    assert!(
        path.components().any(|part| part.as_os_str() == "target"),
        "the scratch directory belongs under the workspace target: {}",
        path.display()
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn two_calls_do_not_share_a_directory() {
    let first = test_scratch_dir(env!("CARGO_MANIFEST_DIR"), "scratch");
    let second = test_scratch_dir(env!("CARGO_MANIFEST_DIR"), "scratch");
    assert_ne!(first, second);
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}
