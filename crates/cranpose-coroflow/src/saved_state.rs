use std::{
    any::Any,
    collections::BTreeMap,
    fmt::Display,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock, PoisonError, Weak},
};

use coroflow::MutableStateFlow;
use cranpose_services::{
    DurableSaveRegistration, PreferencesRef, preferences, register_preference_save,
};

/// Invalid use of a saved-state key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SavedStateError {
    /// A live key was requested using a different Rust type.
    #[error("saved-state key `{0}` already has a different type")]
    TypeMismatch(String),
}

struct Entry<T> {
    state: MutableStateFlow<T>,
    _registration: DurableSaveRegistration,
}

struct Namespace {
    name: String,
    store: PreferencesRef,
    entries: Mutex<BTreeMap<String, Box<dyn Any + Send + Sync>>>,
}

/// Shared observable values persisted at lifecycle or explicit save boundaries.
///
/// Live handles with the same namespace and preferences backend share their keys.
/// The first request determines each key's Rust type. `set`, `get` and returned
/// flows observe that same value. Call `cranpose_services::run_durable_saves`
/// before dropping the last handle when restoration is required; an abrupt exit
/// before a successful flush can lose pending changes. Retaining a returned flow
/// alone does not retain its save registration.
#[derive(Clone)]
pub struct SavedStateHandle {
    namespace: Arc<Namespace>,
}

impl SavedStateHandle {
    /// Opens a namespace in the current preferences backend.
    pub fn new(namespace: impl Into<String>) -> Self {
        let name = namespace.into();
        let store = preferences();
        static NAMESPACES: OnceLock<Mutex<Vec<Weak<Namespace>>>> = OnceLock::new();
        let mut namespaces = NAMESPACES
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut existing = None;
        namespaces.retain(|entry| {
            let Some(entry) = entry.upgrade() else {
                return false;
            };
            if entry.name == name && Arc::ptr_eq(&entry.store, &store) {
                existing = Some(entry);
            }
            true
        });
        let namespace = existing.unwrap_or_else(|| {
            let namespace = Arc::new(Namespace {
                name,
                store,
                entries: Mutex::default(),
            });
            namespaces.push(Arc::downgrade(&namespace));
            namespace
        });
        Self { namespace }
    }

    fn stored_key(&self, key: &str) -> String {
        let mut stored = String::with_capacity(self.namespace.name.len() + key.len() + 1);
        push_segment(&mut stored, &self.namespace.name);
        stored.push('/');
        push_segment(&mut stored, key);
        stored
    }

    /// Reads the live value, or restores it from storage when not opened yet.
    /// Unparsable stored data is treated as absent. A conflicting live type is
    /// returned as an error instead of creating a second writer for the key.
    pub fn get<T: Clone + FromStr + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Option<T>, SavedStateError> {
        let entries = self
            .namespace
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = entries.get(key) {
            return entry
                .downcast_ref::<Entry<T>>()
                .map(|entry| Some(entry.state.value()))
                .ok_or_else(|| SavedStateError::TypeMismatch(self.stored_key(key)));
        }
        Ok(self
            .namespace
            .store
            .get(&self.stored_key(key))
            .and_then(|stored| stored.parse().ok()))
    }

    /// Updates the shared value and collectors. Persistence occurs at the next
    /// successful lifecycle or explicit save boundary, independently of frames.
    pub fn set<T>(&self, key: &str, value: &T) -> Result<(), SavedStateError>
    where
        T: Display + FromStr + Clone + PartialEq + Send + Sync + 'static,
    {
        let (state, created) = self.entry(key, || value.clone())?;
        if !created {
            state.set(value.clone());
        }
        Ok(())
    }

    /// Returns the one shared flow for this key. The initial value is used only
    /// when neither a live value nor a restorable persisted value exists.
    pub fn get_mutable_state_flow<T>(
        &self,
        key: &str,
        initial: T,
    ) -> Result<MutableStateFlow<T>, SavedStateError>
    where
        T: Display + FromStr + Clone + PartialEq + Send + Sync + 'static,
    {
        self.entry(key, || {
            self.namespace
                .store
                .get(&self.stored_key(key))
                .and_then(|stored| stored.parse().ok())
                .unwrap_or(initial)
        })
        .map(|(state, _)| state)
    }

    fn entry<T>(
        &self,
        key: &str,
        initial: impl FnOnce() -> T,
    ) -> Result<(MutableStateFlow<T>, bool), SavedStateError>
    where
        T: Display + FromStr + Clone + PartialEq + Send + Sync + 'static,
    {
        let mut entries = self
            .namespace
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = entries.get(key) {
            return entry
                .downcast_ref::<Entry<T>>()
                .map(|entry| (entry.state.clone(), false))
                .ok_or_else(|| SavedStateError::TypeMismatch(self.stored_key(key)));
        }
        let state = MutableStateFlow::new(initial());
        let saved = state.clone();
        let registration = register_preference_save(
            Arc::clone(&self.namespace.store),
            self.stored_key(key),
            move || saved.value().to_string(),
        );
        entries.insert(
            key.to_owned(),
            Box::new(Entry {
                state: state.clone(),
                _registration: registration,
            }),
        );
        Ok((state, true))
    }
}

fn push_segment(stored: &mut String, segment: &str) {
    for character in segment.chars() {
        match character {
            '%' => stored.push_str("%25"),
            '/' => stored.push_str("%2F"),
            character => stored.push(character),
        }
    }
}
