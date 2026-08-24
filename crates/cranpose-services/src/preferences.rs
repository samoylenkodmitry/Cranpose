//! Small durable key/value storage, and the composition state built on it.
//!
//! Preferences are for the handful of values an application must remember
//! across launches — the chosen theme, the last opened document, a sync folder
//! handle. They are read and written synchronously and are thread-safe, so a
//! worker can persist without hopping to the UI thread.
//!
//! On top of the store sits [`rememberSaveable`], the state that survives host
//! recreation and process death. It stores through a [`Saver`], so a type that
//! is not a string still has one obvious, testable way to become one.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

#[cfg(not(target_arch = "wasm32"))]
use crate::host::application_directories;
use crate::registry::ServiceRegistry;

/// Errors produced by a preferences backend.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum PreferencesError {
    /// The backing store could not be read or written.
    #[error("preferences storage failed: {0}")]
    Io(String),
    /// No durable storage is available on this platform or build.
    #[error("preferences are not available on this platform")]
    Unavailable,
}

/// Durable key/value storage owned by the application.
///
/// Implementations are `Send + Sync`: preferences are written from wherever the
/// decision is made, including a worker thread.
pub trait PreferencesStore: Send + Sync {
    /// Reads `key`, or `None` when it has never been written.
    fn get(&self, key: &str) -> Option<String>;

    /// Writes `key`.
    fn set(&self, key: &str, value: &str) -> Result<(), PreferencesError>;

    /// Removes `key`. Succeeds if it is already absent.
    fn remove(&self, key: &str) -> Result<(), PreferencesError>;

    /// Every key currently stored, in sorted order.
    fn keys(&self) -> Vec<String>;

    /// Removes everything.
    fn clear(&self) -> Result<(), PreferencesError>;
}

/// Shared handle to a [`PreferencesStore`].
pub type PreferencesRef = Arc<dyn PreferencesStore>;

static PLATFORM_PREFERENCES: ServiceRegistry<dyn PreferencesStore> = ServiceRegistry::new();

/// Installs the platform preferences backend, replacing any previous one.
pub fn set_platform_preferences(store: PreferencesRef) {
    PLATFORM_PREFERENCES.set(store);
}

/// Removes any installed platform backend (tests and teardown).
pub fn clear_platform_preferences() {
    PLATFORM_PREFERENCES.clear();
}

/// The active preferences store: the platform backend if one is installed,
/// otherwise the framework's own file-backed store under the application's
/// config directory.
pub fn preferences() -> PreferencesRef {
    if let Some(store) = PLATFORM_PREFERENCES.get() {
        return store;
    }
    default_preferences()
}

#[cfg(not(target_arch = "wasm32"))]
fn default_preferences() -> PreferencesRef {
    static DEFAULT: OnceLock<PreferencesRef> = OnceLock::new();
    DEFAULT
        .get_or_init(|| Arc::new(FilePreferences::new()) as PreferencesRef)
        .clone()
}

#[cfg(all(target_arch = "wasm32", feature = "preferences-web"))]
fn default_preferences() -> PreferencesRef {
    static DEFAULT: OnceLock<PreferencesRef> = OnceLock::new();
    DEFAULT
        .get_or_init(|| Arc::new(BrowserPreferences) as PreferencesRef)
        .clone()
}

#[cfg(all(target_arch = "wasm32", not(feature = "preferences-web")))]
fn default_preferences() -> PreferencesRef {
    static DEFAULT: OnceLock<PreferencesRef> = OnceLock::new();
    DEFAULT
        .get_or_init(|| Arc::new(MemoryPreferences::default()) as PreferencesRef)
        .clone()
}

/// The browser's `localStorage`, which is where preferences outlive a reload.
///
/// Holds no handle of its own: `localStorage` is reached through the window each
/// call, so the store is `Send + Sync` like every other backend even though the
/// object it talks to is not.
#[cfg(all(target_arch = "wasm32", feature = "preferences-web"))]
pub struct BrowserPreferences;

#[cfg(all(target_arch = "wasm32", feature = "preferences-web"))]
impl BrowserPreferences {
    fn storage() -> Result<web_sys::Storage, PreferencesError> {
        web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or(PreferencesError::Unavailable)
    }
}

#[cfg(all(target_arch = "wasm32", feature = "preferences-web"))]
fn storage_error(value: wasm_bindgen::JsValue) -> PreferencesError {
    PreferencesError::Io(
        value
            .as_string()
            .unwrap_or_else(|| "localStorage rejected the operation".to_string()),
    )
}

#[cfg(all(target_arch = "wasm32", feature = "preferences-web"))]
impl PreferencesStore for BrowserPreferences {
    fn get(&self, key: &str) -> Option<String> {
        Self::storage().ok()?.get_item(key).ok().flatten()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), PreferencesError> {
        Self::storage()?.set_item(key, value).map_err(storage_error)
    }

    fn remove(&self, key: &str) -> Result<(), PreferencesError> {
        Self::storage()?.remove_item(key).map_err(storage_error)
    }

    fn keys(&self) -> Vec<String> {
        let Ok(storage) = Self::storage() else {
            return Vec::new();
        };
        let count = storage.length().unwrap_or(0);
        let mut keys: Vec<String> = (0..count)
            .filter_map(|index| storage.key(index).ok().flatten())
            .collect();
        keys.sort();
        keys
    }

    fn clear(&self) -> Result<(), PreferencesError> {
        Self::storage()?.clear().map_err(storage_error)
    }
}

/// An in-memory store. The web default until a platform backend registers, and
/// what tests use.
#[derive(Default)]
pub struct MemoryPreferences {
    entries: Mutex<BTreeMap<String, String>>,
}

impl MemoryPreferences {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl PreferencesStore for MemoryPreferences {
    fn get(&self, key: &str) -> Option<String> {
        self.entries
            .lock()
            .ok()
            .and_then(|entries| entries.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), PreferencesError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| PreferencesError::Io("preferences lock poisoned".into()))?;
        entries.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<(), PreferencesError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| PreferencesError::Io("preferences lock poisoned".into()))?;
        entries.remove(key);
        Ok(())
    }

    fn keys(&self) -> Vec<String> {
        self.entries
            .lock()
            .map(|entries| entries.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn clear(&self) -> Result<(), PreferencesError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| PreferencesError::Io("preferences lock poisoned".into()))?;
        entries.clear();
        Ok(())
    }
}

/// The framework's file-backed store: one line per entry, `key=value` with the
/// value percent-escaped so newlines and equals signs round-trip.
#[cfg(not(target_arch = "wasm32"))]
pub struct FilePreferences {
    entries: Mutex<Option<BTreeMap<String, String>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for FilePreferences {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FilePreferences {
    /// A store that loads lazily from the application's config directory.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(None),
        }
    }

    fn path() -> Result<std::path::PathBuf, PreferencesError> {
        let directories =
            application_directories().map_err(|error| PreferencesError::Io(error.to_string()))?;
        Ok(directories.config.join("preferences"))
    }

    fn with_entries<T>(
        &self,
        body: impl FnOnce(&mut BTreeMap<String, String>) -> T,
    ) -> Result<T, PreferencesError> {
        let mut slot = self
            .entries
            .lock()
            .map_err(|_| PreferencesError::Io("preferences lock poisoned".into()))?;
        if slot.is_none() {
            *slot = Some(Self::load()?);
        }
        let entries = slot
            .as_mut()
            .ok_or_else(|| PreferencesError::Io("preferences were not loaded".into()))?;
        Ok(body(entries))
    }

    fn load() -> Result<BTreeMap<String, String>, PreferencesError> {
        let path = Self::path()?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(PreferencesError::Io(error.to_string())),
        };
        Ok(parse(&text))
    }

    fn store(entries: &BTreeMap<String, String>) -> Result<(), PreferencesError> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PreferencesError::Io(e.to_string()))?;
        }
        let staging = path.with_extension("partial");
        std::fs::write(&staging, encode(entries))
            .map_err(|error| PreferencesError::Io(error.to_string()))?;
        std::fs::rename(&staging, &path).map_err(|error| PreferencesError::Io(error.to_string()))
    }

    fn mutate(
        &self,
        body: impl FnOnce(&mut BTreeMap<String, String>),
    ) -> Result<(), PreferencesError> {
        let snapshot = self.with_entries(|entries| {
            body(entries);
            entries.clone()
        })?;
        Self::store(&snapshot)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl PreferencesStore for FilePreferences {
    fn get(&self, key: &str) -> Option<String> {
        self.with_entries(|entries| entries.get(key).cloned())
            .ok()
            .flatten()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), PreferencesError> {
        self.mutate(|entries| {
            entries.insert(key.to_string(), value.to_string());
        })
    }

    fn remove(&self, key: &str) -> Result<(), PreferencesError> {
        self.mutate(|entries| {
            entries.remove(key);
        })
    }

    fn keys(&self) -> Vec<String> {
        self.with_entries(|entries| entries.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn clear(&self) -> Result<(), PreferencesError> {
        self.mutate(|entries| entries.clear())
    }
}

/// Encodes entries as `key=value` lines, escaping the separators.
#[cfg(not(target_arch = "wasm32"))]
fn encode(entries: &BTreeMap<String, String>) -> String {
    let mut text = String::new();
    for (key, value) in entries {
        text.push_str(&escape(key));
        text.push('=');
        text.push_str(&escape(value));
        text.push('\n');
    }
    text
}

#[cfg(not(target_arch = "wasm32"))]
fn parse(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((unescape(key), unescape(value)))
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '%' => out.push_str("%25"),
            '=' => out.push_str("%3D"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '%' {
            out.push(character);
            continue;
        }
        let high = characters.next();
        let low = characters.next();
        match (high, low) {
            (Some(high), Some(low)) => match u8::from_str_radix(&format!("{high}{low}"), 16) {
                Ok(byte) => out.push(byte as char),
                Err(_) => {
                    out.push('%');
                    out.push(high);
                    out.push(low);
                }
            },
            _ => out.push('%'),
        }
    }
    out
}

// ---- Saveable state ------------------------------------------------------

/// Converts a value to and from the string form preferences store.
///
/// A saver is deliberately explicit rather than derived: the stored form is a
/// compatibility surface, and the framework will not guess it for you.
pub struct Saver<T> {
    save: SaveFn<T>,
    restore: RestoreFn<T>,
}

/// Turns a value into its stored form.
type SaveFn<T> = Box<dyn Fn(&T) -> String + 'static>;
/// Reads a value back out of its stored form.
type RestoreFn<T> = Box<dyn Fn(&str) -> Option<T> + 'static>;

impl<T> Saver<T> {
    /// Builds a saver from a pair of conversions.
    pub fn new(
        save: impl Fn(&T) -> String + 'static,
        restore: impl Fn(&str) -> Option<T> + 'static,
    ) -> Self {
        Self {
            save: Box::new(save),
            restore: Box::new(restore),
        }
    }

    /// The stored form of `value`.
    pub fn save(&self, value: &T) -> String {
        (self.save)(value)
    }

    /// The value `stored` represents, or `None` when it cannot be read — a
    /// stored form written by an older build, or a corrupted entry.
    pub fn restore(&self, stored: &str) -> Option<T> {
        (self.restore)(stored)
    }
}

impl<T> Saver<T>
where
    T: std::fmt::Display + std::str::FromStr + 'static,
{
    /// The saver for a type that already round-trips through `Display` and
    /// `FromStr` — numbers, booleans, strings, enums with a parse impl.
    pub fn of_display() -> Self {
        Self::new(
            |value: &T| value.to_string(),
            |stored: &str| stored.parse::<T>().ok(),
        )
    }
}

/// State that survives host recreation and process death, stored under `key`.
///
/// Reads restore through the saver on first composition; every write is stored
/// immediately, so nothing is lost to a process the OS kills without warning.
#[allow(non_snake_case)]
pub fn rememberSaveable<T>(
    key: &'static str,
    saver: Saver<T>,
    initial: impl FnOnce() -> T,
) -> cranpose_core::MutableState<T>
where
    T: Clone + 'static,
{
    let store = preferences();
    let restored = store
        .get(key)
        .and_then(|stored| saver.restore(&stored))
        .unwrap_or_else(initial);
    let state = cranpose_core::remember(|| cranpose_core::mutableStateOf(restored)).with(|s| *s);

    let saved = cranpose_core::remember(|| std::cell::RefCell::new(Option::<String>::None));
    let stored = saver.save(&state.get());
    saved.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_deref() != Some(stored.as_str()) {
            if let Err(error) = store.set(key, &stored) {
                log::warn!("cranpose: could not store `{key}`: {error}");
            }
            *slot = Some(stored);
        }
    });
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_preferences_round_trip() {
        let store = MemoryPreferences::new();
        assert!(store.get("theme").is_none());
        store.set("theme", "dark").expect("set");
        assert_eq!(store.get("theme").as_deref(), Some("dark"));
        assert_eq!(store.keys(), vec!["theme".to_string()]);
        store.remove("theme").expect("remove");
        assert!(store.keys().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn encoding_round_trips_separators_and_escapes() {
        let mut entries = BTreeMap::new();
        entries.insert("a=b".to_string(), "line1\nline2".to_string());
        entries.insert("percent".to_string(), "100%".to_string());
        let text = encode(&entries);
        assert!(!text.trim_end().contains('\n') || text.lines().count() == 2);
        assert_eq!(parse(&text), entries);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_corrupt_entry_falls_back_to_the_raw_text() {
        assert_eq!(unescape("50%"), "50%");
        assert_eq!(unescape("%ZZ"), "%ZZ");
    }

    #[test]
    fn display_savers_round_trip_and_reject_junk() {
        let saver = Saver::<u32>::of_display();
        assert_eq!(saver.save(&42), "42");
        assert_eq!(saver.restore("42"), Some(42));
        assert_eq!(saver.restore("not a number"), None);
    }

    #[test]
    fn a_custom_saver_states_its_own_stored_form() {
        let saver = Saver::new(
            |value: &Vec<u8>| {
                value
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            },
            |stored: &str| {
                stored
                    .split(',')
                    .filter(|part| !part.is_empty())
                    .map(|part| part.parse().ok())
                    .collect()
            },
        );
        assert_eq!(saver.save(&vec![1, 2, 3]), "1,2,3");
        assert_eq!(saver.restore("1,2,3"), Some(vec![1, 2, 3]));
        assert_eq!(saver.restore("1,x"), None);
    }
}
