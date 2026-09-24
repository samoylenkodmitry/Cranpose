use std::{
    fmt::Display,
    str::FromStr,
    sync::{Mutex, PoisonError},
};

use coroflow::MutableStateFlow;
use cranpose_services::{DurableSaveRegistration, preferences, register_durable_save};

/// Values a view model keeps when the app is killed in the background —
/// Android's `SavedStateHandle`.
///
/// Values live in the app's preferences under `namespace/key`. A state flow
/// from [`get_mutable_state_flow`](SavedStateHandle::get_mutable_state_flow)
/// is written whenever the app leaves the foreground, when Android saves its
/// instance state, and read back the next time a view model asks for it.
pub struct SavedStateHandle {
    namespace: String,
    saves: Mutex<Vec<DurableSaveRegistration>>,
}

impl SavedStateHandle {
    /// A handle whose keys live under `namespace`, usually the view model's
    /// name.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            saves: Mutex::default(),
        }
    }

    fn stored_key(&self, key: &str) -> String {
        format!("{}/{key}", self.namespace)
    }

    /// The saved value for `key`, if there is one that parses — Android's
    /// `get`.
    pub fn get<T: FromStr>(&self, key: &str) -> Option<T> {
        preferences()
            .get(&self.stored_key(key))
            .and_then(|stored| stored.parse().ok())
    }

    /// Saves `value` for `key` at once — Android's `set`.
    pub fn set<T: Display>(&self, key: &str, value: &T) {
        store(&self.stored_key(key), &value.to_string());
    }

    /// A state flow for `key` that starts from the saved value, or `initial`
    /// when there is none, and is saved whenever the app leaves the
    /// foreground — Android's `getMutableStateFlow`.
    pub fn get_mutable_state_flow<T>(&self, key: &str, initial: T) -> MutableStateFlow<T>
    where
        T: Display + FromStr + Clone + PartialEq + Send + Sync + 'static,
    {
        let state = MutableStateFlow::new(self.get(key).unwrap_or(initial));
        let saved = state.clone();
        let stored = self.stored_key(key);
        let registration =
            register_durable_save(move || store(&stored, &saved.value().to_string()));
        self.saves
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(registration);
        state
    }
}

fn store(key: &str, value: &str) {
    if let Err(error) = preferences().set(key, value) {
        log::warn!("cranpose-coroflow: could not save `{key}`: {error}");
    }
}
