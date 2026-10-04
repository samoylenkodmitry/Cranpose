use cranpose_core::{
    CompositionLocal, CompositionLocalProvider, LaunchedEffect, MutableState, compositionLocalOf,
    launchBlocking, rememberKeyed, rememberMutableStateOf,
};

use crate::localization::{Catalog, LanguagePreference, Locale, Localization, ProvideTranslator};

#[derive(Clone, Default, PartialEq)]
struct SelectionState {
    revision: u64,
    loading: bool,
    saving: bool,
    pending: Option<LanguagePreference>,
    error: Option<String>,
}

/// Reactive application language state with asynchronous preference persistence.
/// The controller retains at most one pending choice while a storage request runs.
#[derive(Clone)]
pub struct LocalizationController {
    application: Localization,
    state: MutableState<SelectionState>,
}

impl PartialEq for LocalizationController {
    fn eq(&self, other: &Self) -> bool {
        self.application == other.application
    }
}

impl LocalizationController {
    /// The current saved language choice. Reading it subscribes this component.
    pub fn preference(&self) -> LanguagePreference {
        let _revision = self.state.get().revision;
        self.application.preference()
    }

    /// Supported languages and translations for this application.
    pub fn catalog(&self) -> &Catalog {
        self.application.catalog()
    }

    /// The shared handle for worker messages and native services.
    pub fn application(&self) -> &Localization {
        &self.application
    }

    /// Whether initial preferences or a selected choice await storage completion.
    pub fn is_busy(&self) -> bool {
        let state = self.state.get();
        state.loading || state.saving
    }

    /// The latest preference-storage error, if a request failed.
    pub fn error(&self) -> Option<String> {
        self.state.get().error
    }

    /// Saves a language asynchronously, then updates all subscribed components.
    /// If a request is active, only the most recent additional choice is retained.
    pub fn select(&self, preference: LanguagePreference) {
        if !self.application.is_persistent() {
            let result = self.application.select(preference);
            self.state.update(|state| {
                state.revision = self.application.revision();
                state.error = result.err().map(|error| error.to_string());
            });
            return;
        }
        self.state.update(|state| {
            state.pending = Some(preference);
            state.error = None;
        });
        self.save_next();
    }

    fn save_next(&self) {
        let mut next = None;
        self.state.update(|state| {
            if !state.loading && !state.saving {
                next = state.pending.take();
                state.saving = next.is_some();
            }
        });
        let Some(preference) = next else { return };
        let application = self.application.clone();
        let controller = self.clone();
        let started = launchBlocking(
            move || application.select(preference),
            move |result| {
                controller.state.update(|state| {
                    state.saving = false;
                    state.revision = controller.application.revision();
                    state.error = storage_error(result);
                });
                controller.save_next();
            },
        );
        if let Err(error) = started {
            self.state.update(|state| {
                state.saving = false;
                state.error = Some(error.to_string());
            });
        }
    }
}

/// The language controller installed by the nearest application provider.
pub fn local_localization() -> CompositionLocal<Option<LocalizationController>> {
    crate::environment_locals::cached_local(
        |locals| &locals.localization_controller,
        || compositionLocalOf(|| None),
    )
}

/// Provides automatic language selection and stored overrides to a UI subtree.
/// Native applications normally use `cranpose::ProvideApplicationLocalization`,
/// which supplies and refreshes the host language list automatically.
#[expect(non_snake_case)]
#[track_caller]
pub fn ProvideLanguagePreferences(
    application: Localization,
    system: &[Locale],
    content: impl FnOnce(),
) {
    application.refresh_system(system);
    let state = rememberMutableStateOf(|| SelectionState {
        loading: application.is_persistent(),
        ..SelectionState::default()
    });
    let controller = LocalizationController {
        application: application.clone(),
        state,
    };
    let loader = controller.clone();
    LaunchedEffect(application.clone(), move |_| {
        let application = loader.application.clone();
        if !application.is_persistent() {
            return;
        }
        let state = loader.state;
        let started = launchBlocking(
            move || application.load(),
            move |result| {
                loader.state.update(|state| {
                    state.loading = false;
                    state.revision = loader.application.revision();
                    state.error = storage_error(result);
                });
                loader.save_next();
            },
        );
        if let Err(error) = started {
            state.update(|state| {
                state.loading = false;
                state.error = Some(error.to_string());
            });
        }
    });
    let _observed = state.get().revision;
    let translator = rememberKeyed(
        (application.clone(), application.revision()),
        |(application, _)| application.translator(),
    );
    CompositionLocalProvider([local_localization().provides(Some(controller))], || {
        ProvideTranslator(translator, content);
    });
}

fn storage_error(
    result: Result<
        Result<(), crate::localization::LocalizationError>,
        cranpose_core::BlockingError,
    >,
) -> Option<String> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(error) => Some(error.to_string()),
    }
}
