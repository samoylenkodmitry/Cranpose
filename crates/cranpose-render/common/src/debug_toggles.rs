use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    str::FromStr,
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// A debug toggle cached until an override changes. Unset values can be
/// read without acquiring the cache lock.
pub struct DebugToggle {
    name: &'static str,
    cached: Mutex<Option<(u64, Option<String>)>>,
    absent_generation: AtomicU64,
}

impl DebugToggle {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            cached: Mutex::new(None),
            absent_generation: AtomicU64::new(u64::MAX),
        }
    }

    /// Reads the toggle's value through `read`.
    pub fn with<R>(&self, read: impl FnOnce(Option<&str>) -> R) -> R {
        let generation = GENERATION.load(Ordering::Acquire);
        if self.absent_generation.load(Ordering::Acquire) == generation {
            return read(None);
        }
        let mut cached = self.cached.lock().unwrap_or_else(PoisonError::into_inner);
        if cached.as_ref().is_none_or(|(seen, _)| *seen != generation) {
            *cached = Some((generation, debug_toggle(self.name)));
        }
        let value = cached.as_ref().and_then(|(_, value)| value.as_deref());
        if value.is_none() {
            self.absent_generation.store(generation, Ordering::Release);
        }
        read(value)
    }

    /// Whether the toggle holds any value.
    pub fn is_set(&self) -> bool {
        self.with(|value| value.is_some())
    }

    /// Whether the toggle is switched on: `1`, `true` or `yes`.
    pub fn flag(&self) -> bool {
        self.with(|value| matches!(value, Some("1" | "true" | "yes")))
    }

    /// Whether the toggle holds exactly `expected`.
    pub fn equals(&self, expected: &str) -> bool {
        self.with(|value| value == Some(expected))
    }

    /// The toggle parsed as `T`, when set and well formed.
    pub fn parse<T: FromStr>(&self) -> Option<T> {
        self.with(|value| value.and_then(|value| value.parse().ok()))
    }
}

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
    GENERATION.fetch_add(1, Ordering::Release);
}

#[cfg(all(test, unix))]
#[path = "tests/debug_toggles_tests.rs"]
mod tests;
