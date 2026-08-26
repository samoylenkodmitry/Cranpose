use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock, PoisonError},
};

fn overrides() -> &'static Mutex<BTreeMap<&'static str, String>> {
    static OVERRIDES: OnceLock<Mutex<BTreeMap<&'static str, String>>> = OnceLock::new();
    OVERRIDES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// The value of a debug toggle: a test override if one is set, else the
/// process environment. Toggles are read where they act, every time, so a
/// parity comparison can flip them mid-process.
#[doc(hidden)]
pub fn debug_toggle(name: &'static str) -> Option<String> {
    let map = overrides().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(value) = map.get(name) {
        return Some(value.clone());
    }
    drop(map);
    std::env::var(name).ok()
}

/// As [`debug_toggle`], for settings whose values are paths: the
/// environment fallback keeps its platform encoding instead of demanding
/// UTF-8.
pub fn debug_toggle_os(name: &'static str) -> Option<std::ffi::OsString> {
    let map = overrides().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(value) = map.get(name) {
        return Some(std::ffi::OsString::from(value.clone()));
    }
    drop(map);
    std::env::var_os(name)
}

/// Test-side control of a debug toggle without mutating the process
/// environment; `None` clears the override.
#[doc(hidden)]
pub fn set_debug_toggle(name: &'static str, value: Option<&str>) {
    let mut map = overrides().lock().unwrap_or_else(PoisonError::into_inner);
    match value {
        Some(value) => map.insert(name, value.to_owned()),
        None => map.remove(name),
    };
}
