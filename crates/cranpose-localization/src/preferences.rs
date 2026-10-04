use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{Catalog, DeferredMessage, Language, Locale, LocalizationError, Translator};

/// A saved application language choice, independent of the current system locale.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LanguagePreference {
    /// Follow the host's ordered language preferences.
    #[default]
    System,
    /// Use an explicitly selected application language.
    Selected(Language),
}

impl LanguagePreference {
    /// Reads a saved tag. Missing or unsupported tags follow the system.
    pub fn from_setting(value: Option<&str>, catalog: &Catalog) -> Self {
        value
            .and_then(|tag| {
                catalog
                    .languages()
                    .iter()
                    .find(|language| language.tag() == tag)
            })
            .cloned()
            .map_or(Self::System, Self::Selected)
    }

    /// A stable value suitable for application preference storage.
    pub fn setting(&self) -> &str {
        match self {
            Self::System => "system",
            Self::Selected(language) => language.tag(),
        }
    }

    /// Selects a formatter for this preference and the current host languages.
    pub fn translator(&self, catalog: &Catalog, system: &[Locale]) -> Translator {
        match self {
            Self::System => catalog.negotiate(system),
            Self::Selected(language) => catalog.translator(language.locale().clone()),
        }
    }
}

/// An application's storage adapter for its language preference.
/// Native UI providers call these methods on a worker so storage cannot block a frame.
pub trait LanguagePreferenceStore: Send + Sync {
    /// Loads the saved language tag, or `None` for the system default.
    fn load(&self) -> Result<Option<String>, LocalizationError>;
    /// Saves a canonical language tag or `system`.
    fn save(&self, value: &str) -> Result<(), LocalizationError>;
}

struct State {
    preference: LanguagePreference,
    system: Vec<Locale>,
    choice_revision: u64,
}

struct Data {
    catalog: Catalog,
    store: Option<Arc<dyn LanguagePreferenceStore>>,
    state: Mutex<State>,
    revision: AtomicU64,
    writes: Mutex<()>,
}

/// Shared application localization used by UI roots, workers, and native services.
/// Each thread retains only its most recently used formatter. Language changes
/// invalidate that formatter; repeated messages do not reconstruct Fluent bundles.
#[derive(Clone)]
pub struct Localization(Arc<Data>);

impl PartialEq for Localization {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Localization {
    /// Creates an application scope with automatic system-language selection.
    pub fn new(catalog: Catalog, system: Vec<Locale>) -> Self {
        Self::create(catalog, system, None)
    }

    /// Creates an application scope backed by its own preference storage.
    pub fn persistent(
        catalog: Catalog,
        system: Vec<Locale>,
        store: impl LanguagePreferenceStore + 'static,
    ) -> Self {
        Self::create(catalog, system, Some(Arc::new(store)))
    }

    fn create(
        catalog: Catalog,
        system: Vec<Locale>,
        store: Option<Arc<dyn LanguagePreferenceStore>>,
    ) -> Self {
        Self(Arc::new(Data {
            catalog,
            store,
            state: Mutex::new(State {
                preference: LanguagePreference::System,
                system,
                choice_revision: 0,
            }),
            revision: AtomicU64::new(0),
            writes: Mutex::new(()),
        }))
    }

    /// The application's catalog and supported language choices.
    pub fn catalog(&self) -> &Catalog {
        &self.0.catalog
    }

    /// Whether preference changes access application storage.
    /// Without storage, selection is immediate and needs no worker runtime.
    pub fn is_persistent(&self) -> bool {
        self.0.store.is_some()
    }

    /// The current saved or explicitly selected preference.
    pub fn preference(&self) -> LanguagePreference {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .preference
            .clone()
    }

    /// A version that changes when the selected preference or host languages change.
    pub fn revision(&self) -> u64 {
        self.0.revision.load(Ordering::Acquire)
    }

    /// Loads stored preferences without replacing a newer user selection.
    /// This can access storage; native applications should call it on a worker.
    pub fn load(&self) -> Result<(), LocalizationError> {
        let Some(store) = &self.0.store else {
            return Ok(());
        };
        let revision = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .choice_revision;
        let stored = store.load()?;
        let preference = LanguagePreference::from_setting(stored.as_deref(), self.catalog());
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.choice_revision == revision && state.preference != preference {
            state.preference = preference;
            self.0.revision.fetch_add(1, Ordering::Release);
        }
        Ok(())
    }

    /// Persists a choice and publishes it only after storage succeeds.
    /// Calls are serialized; native applications should call this on a worker.
    pub fn select(&self, preference: LanguagePreference) -> Result<(), LocalizationError> {
        let _write = self
            .0
            .writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let LanguagePreference::Selected(language) = &preference
            && !self.catalog().languages().contains(language)
        {
            return Err(LocalizationError::Preference(
                "language is not offered by this catalog".into(),
            ));
        }
        if let Some(store) = &self.0.store {
            store.save(preference.setting())?;
        }
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.choice_revision = state.choice_revision.wrapping_add(1);
        if state.preference != preference {
            state.preference = preference;
            self.0.revision.fetch_add(1, Ordering::Release);
        }
        Ok(())
    }

    /// Updates the ordered host preferences without changing an explicit choice.
    pub fn refresh_system(&self, system: &[Locale]) -> bool {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.system == system {
            return false;
        }
        state.system.clear();
        state.system.extend_from_slice(system);
        self.0.revision.fetch_add(1, Ordering::Release);
        true
    }

    /// Creates a formatter for the current application choice on this thread.
    pub fn translator(&self) -> Translator {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.preference.translator(self.catalog(), &state.system)
    }

    /// Resolves a deferred message on any thread using its cached formatter.
    pub fn format(&self, message: &DeferredMessage) -> Result<String, LocalizationError> {
        THREAD_FORMATTER.with(|cache| {
            let mut cache = cache.borrow_mut();
            let revision = self.revision();
            if !cache.as_ref().is_some_and(|cached| {
                cached.owner.as_ptr() == Arc::as_ptr(&self.0) && cached.revision == revision
            }) {
                *cache = Some(Cached {
                    owner: Arc::downgrade(&self.0),
                    revision,
                    translator: self.translator(),
                });
            }
            message.format(cache.as_ref().map(|cached| &cached.translator))
        })
    }

    /// Resolves native UI text, with readable source text if a runtime resource is malformed.
    pub fn text(&self, message: &DeferredMessage) -> String {
        self.format(message)
            .unwrap_or_else(|_| message.fallback_text())
    }
}

struct Cached {
    owner: Weak<Data>,
    revision: u64,
    translator: Translator,
}

thread_local! {
    static THREAD_FORMATTER: RefCell<Option<Cached>> = const { RefCell::new(None) };
}
