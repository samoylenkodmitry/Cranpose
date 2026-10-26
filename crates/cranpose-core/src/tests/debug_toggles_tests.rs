use std::os::unix::ffi::OsStrExt;

use super::*;

#[test]
fn a_non_utf8_path_override_survives_byte_for_byte() {
    let raw = OsStr::from_bytes(b"/cranpose/\xff\xfe/cache.bin");
    set_debug_toggle_os("CRANPOSE_TOGGLE_OS_TEST", Some(raw));
    assert_eq!(
        debug_toggle_os("CRANPOSE_TOGGLE_OS_TEST").as_deref(),
        Some(raw)
    );
    assert_eq!(debug_toggle("CRANPOSE_TOGGLE_OS_TEST"), None);
    set_debug_toggle_os("CRANPOSE_TOGGLE_OS_TEST", None);
    assert_eq!(debug_toggle_os("CRANPOSE_TOGGLE_OS_TEST"), None);
}

#[test]
fn an_unset_toggle_observes_overrides_from_another_thread() {
    let toggle = std::sync::Arc::new(DebugToggle::new("CRANPOSE_TOGGLE_THREAD_TEST"));
    assert!(!toggle.is_set());
    let reader = std::sync::Arc::clone(&toggle);
    std::thread::spawn(move || {
        set_debug_toggle("CRANPOSE_TOGGLE_THREAD_TEST", Some("1"));
        assert!(reader.flag());
        assert_eq!(reader.parse::<u32>(), Some(1));
    })
    .join()
    .expect("override thread");
    assert!(toggle.equals("1"));
    set_debug_toggle("CRANPOSE_TOGGLE_THREAD_TEST", None);
    assert!(!toggle.is_set());
    assert_eq!(toggle.parse::<u32>(), None);
}

#[test]
fn a_cached_toggle_follows_every_override() {
    static TOGGLE: DebugToggle = DebugToggle::new("CRANPOSE_TOGGLE_CACHE_TEST");
    assert!(!TOGGLE.is_set());
    set_debug_toggle("CRANPOSE_TOGGLE_CACHE_TEST", Some("1"));
    assert!(TOGGLE.flag());
    assert!(TOGGLE.equals("1"));
    assert_eq!(TOGGLE.parse::<u32>(), Some(1));
    set_debug_toggle("CRANPOSE_TOGGLE_CACHE_TEST", Some("frame"));
    assert!(!TOGGLE.flag());
    assert!(TOGGLE.equals("frame"));
    assert_eq!(TOGGLE.parse::<u32>(), None);
    set_debug_toggle("CRANPOSE_TOGGLE_CACHE_TEST", None);
    assert!(!TOGGLE.is_set());
}
