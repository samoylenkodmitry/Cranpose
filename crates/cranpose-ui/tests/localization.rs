use std::{cell::RefCell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_ui::{
    Catalog, LayoutDirection, Locale, Modifier, ProvideLayoutDirection, ProvideLocalization, Text,
    TextStyle, UiString, composable, layout_direction, run_test_composition, tr,
    translation_messages, translations,
};

translation_messages!(pub mod messages, "tests/fixtures/localization/en/cranpose-ui.ftl");

fn locale(tag: &str) -> Locale {
    Locale::parse(tag).expect("locale")
}

#[test]
fn event_messages_own_arguments_and_follow_later_language_changes() {
    let name = String::from("Ana 商品");
    let message = cranpose_ui::message!("Hello, {name}!", id = "hello", name = name.as_str());
    drop(name);
    let mut composition = run_test_composition(|| {});
    let language = MutableState::with_runtime(locale("en"), composition.runtime_handle());
    let output = Rc::new(RefCell::new(String::new()));
    composition
        .render(981, {
            let output = Rc::clone(&output);
            move || DeferredRoot(language, message.clone(), Rc::clone(&output))
        })
        .expect("initial render");
    assert!(output.borrow().contains("Hello"));
    language.set_value(locale("fr"));
    composition
        .process_invalid_scopes()
        .expect("language change");
    assert!(output.borrow().contains("Bonjour"));
    assert!(output.borrow().contains("Ana 商品"));
}

#[composable]
fn DeferredRoot(
    language: MutableState<Locale>,
    message: cranpose_ui::localization::DeferredMessage,
    output: Rc<RefCell<String>>,
) {
    ProvideLocalization(&catalog(), language.value(), || {
        *output.borrow_mut() = cranpose_ui::localization::localized_message(&message).into();
    });
}

fn catalog() -> Catalog {
    translations!("tests/fixtures/localization", fallback = "en")
}

#[composable]
fn Label(output: Rc<RefCell<Vec<String>>>, tick: u32) {
    let label = tr!("Save", id = "save");
    output
        .borrow_mut()
        .push(format!("{tick}:{}", label.as_str()));
    Text(
        label.clone(),
        Modifier::empty().content_description(label),
        TextStyle::default(),
    );
}

#[composable]
fn Root(
    catalog: Catalog,
    language: MutableState<Locale>,
    tick: MutableState<u32>,
    output: Rc<RefCell<Vec<String>>>,
) {
    ProvideLocalization(&catalog, language.value(), || Label(output, tick.value()));
}

#[test]
fn changing_language_after_a_cached_read_updates_existing_composables() {
    let mut composition = run_test_composition(|| {});
    let language = MutableState::with_runtime(locale("en"), composition.runtime_handle());
    let tick = MutableState::with_runtime(0, composition.runtime_handle());
    let output = Rc::new(RefCell::new(Vec::new()));
    composition
        .render(901, {
            let output = Rc::clone(&output);
            move || Root(catalog(), language, tick, Rc::clone(&output))
        })
        .expect("render");
    tick.set_value(1);
    composition.process_invalid_scopes().expect("cached read");
    language.set_value(locale("fr"));
    composition
        .process_invalid_scopes()
        .expect("language switch");
    assert_eq!(
        output.borrow().last().map(String::as_str),
        Some("1:Enregistrer")
    );
}

#[test]
fn nested_providers_restore_language_and_direction_and_allow_overrides() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    run_test_composition({
        let seen = Rc::clone(&seen);
        move || {
            ProvideLocalization(&catalog(), locale("fr"), || {
                let save = tr!("Save", id = "save").to_string();
                seen.borrow_mut().push((save, layout_direction()));
                ProvideLocalization(&catalog(), locale("ar"), || {
                    let search = UiString::Search.resolve();
                    seen.borrow_mut().push((search, layout_direction()));
                    ProvideLayoutDirection(LayoutDirection::Ltr, || {
                        assert_eq!(layout_direction(), LayoutDirection::Ltr);
                    });
                });
                let save = tr!("Save", id = "save").to_string();
                seen.borrow_mut().push((save, layout_direction()));
            });
        }
    });
    assert_eq!(
        *seen.borrow(),
        [
            ("Enregistrer".into(), LayoutDirection::Ltr),
            ("بحث".into(), LayoutDirection::Rtl),
            ("Enregistrer".into(), LayoutDirection::Ltr)
        ]
    );
}

#[test]
fn separate_compositions_and_framework_overrides_do_not_share_language() {
    let copy_in = |tag: &'static str| {
        let copy = Rc::new(RefCell::new(String::new()));
        let composition = run_test_composition({
            let copy = Rc::clone(&copy);
            move || {
                ProvideLocalization(&catalog(), locale(tag), || {
                    *copy.borrow_mut() = UiString::Copy.resolve();
                });
            }
        });
        (composition, copy)
    };
    let (_a, first) = copy_in("fr");
    let (_b, second) = copy_in("en");
    assert_eq!(*first.borrow(), "Copier autrement");
    assert_eq!(*second.borrow(), "Copy");
}

#[test]
fn serbian_framework_strings_keep_the_script_the_app_asks_for() {
    let app = Catalog::from_resources("en", &[]).expect("catalog");
    let seen = Rc::new(RefCell::new(Vec::new()));
    for tag in ["sr", "sr-Latn"] {
        let (app, seen) = (app.clone(), Rc::clone(&seen));
        let _composition = run_test_composition(move || {
            ProvideLocalization(&app, locale(tag), || {
                seen.borrow_mut().push(UiString::Copy.resolve());
            });
        });
    }
    assert_eq!(*seen.borrow(), ["Копирај", "Kopiraj"]);
}

#[test]
fn generated_accessors_and_borrowed_arguments_work_without_a_provider() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    run_test_composition({
        let seen = Rc::clone(&seen);
        move || {
            let name = "Ana".to_owned();
            let mut seen = seen.borrow_mut();
            seen.push(tr!("Hello, {name}!", name = name.as_str()).to_string());
            seen.push(messages::files(2).to_string());
            seen.push(messages::save().to_string());
        }
    });
    let seen = seen.borrow();
    assert!(seen[0].contains("Ana"));
    assert!(seen[1].contains("files"));
    assert_eq!(seen[2], "Save");
}

#[test]
fn replacing_catalogs_and_changing_arguments_refreshes_text() {
    let mut composition = run_test_composition(|| {});
    let result = Rc::new(RefCell::new(String::new()));
    for (source, count, expected) in [
        ("files = { $count } documents", 2, "documents"),
        ("files = { $count } articles", 3, "articles"),
    ] {
        let catalog = Catalog::from_resources(
            "en",
            &[cranpose_ui::localization::Resource {
                locale: "fr",
                namespace: "cranpose-ui",
                source,
            }],
        )
        .expect("catalog");
        composition
            .render(905, {
                let result = Rc::clone(&result);
                move || {
                    ProvideLocalization(&catalog, locale("fr"), || {
                        *result.borrow_mut() = messages::files(count).to_string();
                    });
                }
            })
            .expect("render");
        assert!(result.borrow().contains(expected));
        assert!(result.borrow().contains(&count.to_string()));
    }
}

#[test]
fn application_preferences_update_retained_text_and_restore_system_language() {
    use cranpose_ui::{
        UiText,
        localization::{LanguagePreference, Localization},
    };
    let application =
        Localization::persistent(catalog(), vec![locale("fr")], PreferenceStore::default());
    let controller = Rc::new(RefCell::new(None));
    let output = Rc::new(RefCell::new(String::new()));
    let retained = UiText::from(cranpose_ui::message!("Save", id = "save"));
    let mut composition = run_test_composition(|| {});
    composition
        .render(991, {
            let (application, controller) = (application.clone(), Rc::clone(&controller));
            let output = Rc::clone(&output);
            move || {
                PreferenceRoot(
                    application.clone(),
                    Rc::clone(&controller),
                    retained.clone(),
                    Rc::clone(&output),
                );
            }
        })
        .expect("render");
    let controller = controller.borrow().clone().expect("controller");
    wait_for_preferences(&mut composition, &controller);
    assert_eq!(*output.borrow(), "Enregistrer");
    choose_language(
        &mut composition,
        &controller,
        LanguagePreference::from_setting(Some("en"), application.catalog()),
    );
    assert_eq!(*output.borrow(), "Save");
    choose_language(&mut composition, &controller, LanguagePreference::System);
    assert_eq!(*output.borrow(), "Enregistrer");
}

#[derive(Default)]
struct PreferenceStore(std::sync::Mutex<Option<String>>);

impl cranpose_ui::localization::LanguagePreferenceStore for PreferenceStore {
    fn load(&self) -> Result<Option<String>, cranpose_ui::localization::LocalizationError> {
        Ok(self.0.lock().expect("preference store").clone())
    }

    fn save(&self, value: &str) -> Result<(), cranpose_ui::localization::LocalizationError> {
        *self.0.lock().expect("preference store") = Some(value.into());
        Ok(())
    }
}

#[test]
fn in_memory_language_changes_do_not_require_a_worker_runtime() {
    use cranpose_ui::localization::{LanguagePreference, Localization};
    let application = Localization::new(catalog(), vec![locale("fr")]);
    let controller = Rc::new(RefCell::new(None));
    let output = Rc::new(RefCell::new(String::new()));
    let mut composition = run_test_composition(|| {});
    composition
        .render(992, {
            let (application, controller) = (application.clone(), Rc::clone(&controller));
            let output = Rc::clone(&output);
            move || {
                PreferenceRoot(
                    application.clone(),
                    Rc::clone(&controller),
                    cranpose_ui::message!("Save", id = "save").into(),
                    Rc::clone(&output),
                );
            }
        })
        .expect("render");
    let controller = controller.borrow().clone().expect("controller");
    assert!(!controller.is_busy());
    controller.select(LanguagePreference::from_setting(
        Some("en"),
        application.catalog(),
    ));
    composition
        .process_invalid_scopes()
        .expect("language change");
    assert_eq!(controller.error(), None);
    assert_eq!(*output.borrow(), "Save");
}

#[composable]
fn PreferenceRoot(
    application: cranpose_ui::localization::Localization,
    controller: Rc<RefCell<Option<cranpose_ui::LocalizationController>>>,
    retained: cranpose_ui::UiText,
    output: Rc<RefCell<String>>,
) {
    cranpose_ui::ProvideLanguagePreferences(application, &[locale("fr")], || {
        *controller.borrow_mut() = cranpose_ui::local_localization().current();
        *output.borrow_mut() = retained.resolve().to_string();
    });
}

fn choose_language(
    composition: &mut cranpose_ui::TestComposition,
    controller: &cranpose_ui::LocalizationController,
    preference: cranpose_ui::localization::LanguagePreference,
) {
    let action = controller.clone();
    composition
        .runtime_handle()
        .spawn_ui(async move {
            action.select(preference);
        })
        .expect("event");
    wait_for_preferences(composition, controller);
}

fn wait_for_preferences(
    composition: &mut cranpose_ui::TestComposition,
    controller: &cranpose_ui::LocalizationController,
) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        composition.runtime_handle().drain_ui();
        composition
            .process_invalid_scopes()
            .expect("preference update");
        if !controller.is_busy() {
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "preference request exceeded deadline"
        );
        std::thread::yield_now();
    }
    assert_eq!(controller.error(), None);
}

#[test]
fn literal_ui_text_remains_literal_inside_a_language_provider() {
    let literal = cranpose_ui::UiText::from("Save");
    run_test_composition(move || {
        ProvideLocalization(&catalog(), locale("fr"), || {
            assert_eq!(literal.resolve().as_str(), "Save");
            Text(literal.clone(), Modifier::empty(), TextStyle::default());
        });
    });
}
