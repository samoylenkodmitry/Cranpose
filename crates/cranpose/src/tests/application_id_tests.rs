use std::path::Path;

use super::*;

#[test]
fn a_configured_id_wins_over_the_executable_name() {
    assert_eq!(
        resolve(Some("dev.example.tool"), Some(Path::new("/bin/other"))),
        "dev.example.tool"
    );
}

#[test]
fn an_unconfigured_id_is_the_executable_stem() {
    assert_eq!(
        resolve(None, Some(Path::new("/opt/tools/panel-ui.exe"))),
        "panel-ui"
    );
}

#[test]
fn without_either_the_fallback_id_is_used() {
    assert_eq!(resolve(None, None), FALLBACK_APPLICATION_ID);
}
