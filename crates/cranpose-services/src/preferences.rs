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
    rc::Rc,
    sync::{Arc, Mutex, OnceLock},
};

#[cfg(not(target_arch = "wasm32"))]
mod file;
#[cfg(not(target_arch = "wasm32"))]
pub use file::FilePreferences;

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

    /// Removes `removed`, then writes `values`, as one group. File-backed
    /// stores replace the file once. A failure may leave pending changes
    /// visible; retry this batch or call `flush`.
    fn update_many(
        &self,
        values: &BTreeMap<String, String>,
        removed: &[String],
    ) -> Result<(), PreferencesError> {
        for key in removed {
            self.remove(key)?;
        }
        for (key, value) in values {
            self.set(key, value)?;
        }
        Ok(())
    }

    /// Retries pending writes. Success means the backend accepted all pending data.
    fn flush(&self) -> Result<(), PreferencesError> {
        Ok(())
    }

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

#[cfg(not(target_arch = "wasm32"))]
fn encode(entries: &BTreeMap<String, String>) -> String {
    let mut text = String::new();
    for (key, value) in entries {
        escape_into(key, &mut text);
        text.push('=');
        escape_into(value, &mut text);
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
fn escape_into(value: &str, out: &mut String) {
    for character in value.chars() {
        match character {
            '%' => out.push_str("%25"),
            '=' => out.push_str("%3D"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            other => out.push(other),
        }
    }
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

/// Converts a value to and from the string form preferences store.
///
/// A saver is deliberately explicit rather than derived: the stored form is a
/// compatibility surface, and the framework will not guess it for you.
pub struct Saver<T> {
    save: SaveFn<T>,
    restore: RestoreFn<T>,
}

type SaveFn<T> = Box<dyn Fn(&T) -> String + 'static>;
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

impl<T: 'static> Saver<T> {
    /// A saver that stores a value as the list of strings `save` gives —
    /// Compose's `listSaver`. Each string may hold any text, the stored form
    /// of another [`Saver`] too. `restore` gets the strings back in order.
    pub fn list(
        save: impl Fn(&T) -> Vec<String> + 'static,
        restore: impl Fn(&[String]) -> Option<T> + 'static,
    ) -> Self {
        Self::new(
            move |value| encode_list(&save(value)),
            move |stored| restore(&decode_list(stored)?),
        )
    }

    /// A saver that stores a value as the named strings `save` gives —
    /// Compose's `mapSaver`. `restore` gets them back by name; a name given
    /// twice keeps its last string.
    pub fn map(
        save: impl Fn(&T) -> Vec<(String, String)> + 'static,
        restore: impl Fn(&std::collections::HashMap<String, String>) -> Option<T> + 'static,
    ) -> Self {
        Self::new(
            move |value| {
                let pairs = save(value);
                let mut items = Vec::with_capacity(pairs.len() * 2);
                for (name, text) in pairs {
                    items.push(name);
                    items.push(text);
                }
                encode_list(&items)
            },
            move |stored| {
                let items = decode_list(stored)?;
                if items.len() % 2 != 0 {
                    return None;
                }
                let mut map = std::collections::HashMap::with_capacity(items.len() / 2);
                let mut items = items.into_iter();
                while let (Some(name), Some(text)) = (items.next(), items.next()) {
                    map.insert(name, text);
                }
                restore(&map)
            },
        )
    }
}

/// `items` as one string: each item's byte length, a colon, then the item.
fn encode_list(items: &[String]) -> String {
    use std::fmt::Write;

    let mut stored = String::with_capacity(items.iter().map(|item| item.len() + 4).sum());
    for item in items {
        let _ = write!(stored, "{}:{item}", item.len());
    }
    stored
}

/// The items [`encode_list`] stored, or `None` for a string it did not write.
fn decode_list(mut stored: &str) -> Option<Vec<String>> {
    let mut items = Vec::new();
    while !stored.is_empty() {
        let (length, rest) = stored.split_once(':')?;
        let (item, rest) = rest.split_at_checked(length.parse().ok()?)?;
        items.push(item.to_owned());
        stored = rest;
    }
    Some(items)
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

/// State restored from preferences on first composition and saved at a flush.
///
/// Native hosts flush when leaving the foreground. Call [`crate::run_durable_saves`]
/// on the composition thread for an explicit durability boundary. Mutations before
/// that boundary are captured without another frame; abrupt termination before a
/// successful flush can lose them. Failed flushes can be retried.
///
/// `key` is a key in the [`SaveableScope`] where this is composed, so a
/// navigation entry's state stays apart from another's. When the composable
/// leaves the composition its last value is kept and saved at the next
/// boundary, and a composable that comes back restores it.
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberSaveable<T>(
    key: &'static str,
    saver: Saver<T>,
    initial: impl FnOnce() -> T,
) -> cranpose_core::MutableState<T>
where
    T: Clone + 'static,
{
    cranpose_core::remember(move || {
        let scope = SaveableScope::current();
        let (store, key) = (scope.store(), scope.key(key));
        let restored = crate::durable_save::saved(store.as_ref(), &key)
            .and_then(|stored| saver.restore(&stored))
            .unwrap_or_else(initial);
        let owner = cranpose_core::mutableStateOfNeverEqual(restored);
        let state = owner;
        let registration = crate::durable_save::register_local(store, key, move || {
            state.try_with(|value| saver.save(value))
        });
        (owner, registration)
    })
    .with(|(state, _)| *state)
}

/// Where the [`rememberSaveable`] calls in some content keep their values: a
/// key namespace, and whether the values are saved in [`preferences`] or kept
/// only in memory. A navigation host gives each back-stack entry its own, so
/// two visits of one screen keep apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveableScope {
    namespace: Option<Rc<str>>,
    durable: bool,
}

fn local_saveable_scope() -> cranpose_core::CompositionLocal<SaveableScope> {
    crate::composition_locals::cached_local(
        |locals| &locals.saveable_scope,
        || {
            cranpose_core::compositionLocalOf(|| SaveableScope {
                namespace: None,
                durable: true,
            })
        },
    )
}

impl SaveableScope {
    /// The scope where this is composed: the one [`ProvideSaveableScope`]
    /// gives, else the root scope, which saves each key as it is in
    /// [`preferences`]. Call it while composing.
    pub fn current() -> Self {
        local_saveable_scope().current()
    }

    /// The scope named `name` inside this one. It saves where this one does.
    pub fn child(&self, name: impl std::fmt::Display) -> Self {
        Self {
            namespace: Some(self.key(name).into()),
            durable: self.durable,
        }
    }

    /// This scope with its values kept only in memory, until the scope is
    /// forgotten or the process ends.
    #[must_use]
    pub fn in_memory(self) -> Self {
        Self {
            durable: false,
            ..self
        }
    }

    /// The prefix of the keys this scope saves under; `None` for the root
    /// scope, whose keys stand as they are.
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Whether this scope saves its values in [`preferences`], rather than
    /// keeping them in memory.
    pub fn is_durable(&self) -> bool {
        self.durable
    }

    /// The value last saved under `key` in this scope.
    pub fn restore(&self, key: &str) -> Option<String> {
        crate::durable_save::saved(self.store().as_ref(), &self.key(key))
    }

    /// Saves what `read` returns under `key` in this scope at each save
    /// boundary while the registration lives, and keeps its last value once
    /// it is dropped, as [`rememberSaveable`] does. `read` must not perform
    /// storage I/O.
    pub fn register(
        &self,
        key: &str,
        read: impl Fn() -> Option<String> + 'static,
    ) -> crate::durable_save::SaveableRegistration {
        crate::durable_save::register_local(self.store(), self.key(key), read)
    }

    /// Forgets everything saved in this scope: the state of a back-stack
    /// entry once it is popped. Saves in it stop, and stored values are
    /// removed at the next save boundary. The root scope forgets nothing.
    pub fn forget(&self) {
        self.forget_children_except(std::iter::empty::<&str>());
    }

    /// Forgets everything saved in this scope apart from the child scopes
    /// named in `keep`, as [`SaveableScope::forget`] does.
    pub fn forget_children_except<N: std::fmt::Display>(&self, keep: impl IntoIterator<Item = N>) {
        let Some(namespace) = &self.namespace else {
            return;
        };
        let keep = keep
            .into_iter()
            .map(|name| format!("{namespace}/{name}/"))
            .collect();
        crate::durable_save::forget(self.store(), format!("{namespace}/"), keep);
    }

    pub(crate) fn key(&self, key: impl std::fmt::Display) -> String {
        match &self.namespace {
            Some(namespace) => format!("{namespace}/{key}"),
            None => key.to_string(),
        }
    }

    fn store(&self) -> Option<PreferencesRef> {
        self.durable.then(preferences)
    }
}

/// Gives the [`rememberSaveable`] calls in `content` the saveable `scope`.
#[expect(non_snake_case)]
#[track_caller]
pub fn ProvideSaveableScope(scope: SaveableScope, content: impl FnOnce()) {
    cranpose_core::CompositionLocalProvider([local_saveable_scope().provides(scope)], content);
}

#[cfg(test)]
#[path = "tests/preferences_tests.rs"]
mod tests;
