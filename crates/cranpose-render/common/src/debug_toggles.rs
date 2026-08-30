use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    sync::{Mutex, OnceLock, PoisonError},
};

fn overrides() -> &'static Mutex<BTreeMap<&'static str, OsString>> {
    static OVERRIDES: OnceLock<Mutex<BTreeMap<&'static str, OsString>>> = OnceLock::new();
    OVERRIDES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[doc(hidden)]
pub fn debug_toggle(name: &'static str) -> Option<String> {
    let map = overrides().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(value) = map.get(name) {
        return value.to_str().map(str::to_owned);
    }
    drop(map);
    std::env::var(name).ok()
}

#[doc(hidden)]
pub fn debug_toggle_os(name: &'static str) -> Option<OsString> {
    let map = overrides().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(value) = map.get(name) {
        return Some(value.clone());
    }
    drop(map);
    std::env::var_os(name)
}

#[doc(hidden)]
pub fn set_debug_toggle(name: &'static str, value: Option<&str>) {
    set_debug_toggle_os(name, value.map(OsStr::new));
}

#[doc(hidden)]
pub fn set_debug_toggle_os(name: &'static str, value: Option<&OsStr>) {
    let mut map = overrides().lock().unwrap_or_else(PoisonError::into_inner);
    match value {
        Some(value) => map.insert(name, value.to_owned()),
        None => map.remove(name),
    };
}

#[cfg(all(test, unix))]
mod tests {
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
}
