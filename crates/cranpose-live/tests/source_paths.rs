use std::{path::PathBuf, process::Command};

use cranpose_live::host::original_source_path;

#[test]
fn private_source_paths_resolve_to_the_most_specific_original_root() {
    if std::env::var_os("CRANPOSE_LIVE_PATH_TEST").is_some() {
        assert_eq!(
            original_source_path("/cache/app/screen.rs".into()).expect("root map"),
            PathBuf::from("/project/screen.rs")
        );
        assert_eq!(
            original_source_path("/cache/app/widget/screen.rs".into()).expect("nested map"),
            PathBuf::from("/shared/widget/screen.rs")
        );
        assert_eq!(
            original_source_path("/cache/application/screen.rs".into()).expect("different path"),
            PathBuf::from("/cache/application/screen.rs")
        );
        return;
    }
    let result = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("private_source_paths_resolve_to_the_most_specific_original_root")
        .env("CRANPOSE_LIVE_PATH_TEST", "1")
        .env(
            "CRANPOSE_DEV_SOURCE_MAP",
            r#"[["/cache/app","/project"],["/cache/app/widget","/shared/widget"]]"#,
        )
        .output()
        .expect("isolated environment");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
