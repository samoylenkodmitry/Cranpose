use std::sync::{Arc, Mutex};

use cranpose_localization::{
    Catalog, DeferredMessage, LanguagePreference, LanguagePreferenceStore, Locale, Localization,
    LocalizationError, Message, Resource, SourceCatalog,
};

static SOURCE: SourceCatalog = SourceCatalog::new("en", "save = Save\n");
static SAVE: Message = Message::new("app", "save", &SOURCE);

fn catalog() -> Catalog {
    Catalog::from_resources(
        "en",
        &[
            Resource {
                locale: "en",
                namespace: "app",
                source: "save = Save\n",
            },
            Resource {
                locale: "fr",
                namespace: "app",
                source: "save = Enregistrer\n",
            },
            Resource {
                locale: "de",
                namespace: "app",
                source: "save = Speichern\n",
            },
        ],
    )
    .expect("catalog")
    .with_languages(&[("en", "English"), ("fr", "Français"), ("de", "Deutsch")])
    .expect("languages")
}

fn locales(tags: &[&str]) -> Vec<Locale> {
    tags.iter()
        .map(|tag| Locale::parse(tag).expect("locale"))
        .collect()
}

fn choice(application: &Localization, tag: &str) -> LanguagePreference {
    LanguagePreference::from_setting(Some(tag), application.catalog())
}

fn save(application: &Localization) -> String {
    application.text(&DeferredMessage::new(&SAVE, []))
}

struct Store(Arc<Mutex<Option<String>>>);

impl LanguagePreferenceStore for Store {
    fn load(&self) -> Result<Option<String>, LocalizationError> {
        Ok(self.0.lock().expect("store").clone())
    }
    fn save(&self, value: &str) -> Result<(), LocalizationError> {
        *self.0.lock().expect("store") = Some(value.into());
        Ok(())
    }
}

#[test]
fn declared_choices_survive_library_fallback_without_extra_languages() {
    let library = Catalog::from_resources(
        "en",
        &[Resource {
            locale: "ar",
            namespace: "library",
            source: "copy = نسخ\n",
        }],
    )
    .expect("library");
    let application = catalog().with_fallback(&library);
    assert_eq!(
        application
            .languages()
            .iter()
            .map(|language| (language.tag(), language.name()))
            .collect::<Vec<_>>(),
        [("en", "English"), ("fr", "Français"), ("de", "Deutsch")]
    );
    assert!(catalog().with_languages(&[("en", "English")]).is_err());
    assert!(
        catalog()
            .with_languages(&[("en", "English"), ("fr", "Français"), ("fr", "Français")])
            .is_err()
    );
}

#[test]
fn saved_choice_overrides_system_and_system_choice_follows_later_changes() {
    let stored = Arc::new(Mutex::new(Some("fr".into())));
    let application =
        Localization::persistent(catalog(), locales(&["de-DE"]), Store(stored.clone()));
    application.load().expect("load");
    assert_eq!(save(&application), "Enregistrer");
    application.refresh_system(&locales(&["en"]));
    assert_eq!(save(&application), "Enregistrer");
    application
        .select(LanguagePreference::System)
        .expect("select");
    assert_eq!(save(&application), "Save");
    application.refresh_system(&locales(&["de"]));
    assert_eq!(save(&application), "Speichern");
    assert_eq!(stored.lock().expect("store").as_deref(), Some("system"));
    let restarted = Localization::persistent(catalog(), locales(&["fr"]), Store(stored));
    restarted.load().expect("restart");
    assert_eq!(save(&restarted), "Enregistrer");
}

#[test]
fn thread_formatters_follow_changes_without_crossing_application_scopes() {
    let first = Localization::new(catalog(), locales(&["en"]));
    let second = Localization::new(catalog(), locales(&["de"]));
    assert_eq!(save(&first), "Save");
    assert_eq!(save(&second), "Speichern");
    let worker = first.clone();
    let (request, receive) = std::sync::mpsc::channel();
    let (reply, result) = std::sync::mpsc::channel();
    let task = std::thread::spawn(move || {
        for () in receive {
            reply.send(save(&worker)).expect("reply");
        }
    });
    request.send(()).expect("request");
    assert_eq!(result.recv().expect("text"), "Save");
    first.select(choice(&first, "fr")).expect("select");
    request.send(()).expect("request");
    assert_eq!(result.recv().expect("text"), "Enregistrer");
    assert_eq!(save(&second), "Speichern");
    drop(request);
    task.join().expect("worker");
}

#[test]
fn unsupported_saved_preferences_fall_back_to_system() {
    for value in [None, Some("system"), Some("invalid"), Some("ar")] {
        let application = Localization::persistent(
            catalog(),
            locales(&["fr"]),
            Store(Arc::new(Mutex::new(value.map(str::to_owned)))),
        );
        application.load().expect("load");
        assert_eq!(application.preference(), LanguagePreference::System);
        assert_eq!(save(&application), "Enregistrer");
    }
}

struct FailedStore;

impl LanguagePreferenceStore for FailedStore {
    fn load(&self) -> Result<Option<String>, LocalizationError> {
        Ok(None)
    }
    fn save(&self, _: &str) -> Result<(), LocalizationError> {
        Err(LocalizationError::Preference("storage unavailable".into()))
    }
}

#[test]
fn failed_save_keeps_visible_language() {
    let application = Localization::persistent(catalog(), locales(&["en"]), FailedStore);
    assert!(application.select(choice(&application, "fr")).is_err());
    assert_eq!(save(&application), "Save");
}
