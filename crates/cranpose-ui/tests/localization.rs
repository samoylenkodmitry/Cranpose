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
        .render(981, || {
            DeferredRoot(language, message.clone(), output.clone());
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
        .render(901, || Root(catalog(), language, tick, output.clone()))
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
    let mut seen = Vec::new();
    run_test_composition(|| {
        ProvideLocalization(&catalog(), locale("fr"), || {
            seen.push((tr!("Save", id = "save").to_string(), layout_direction()));
            ProvideLocalization(&catalog(), locale("ar"), || {
                seen.push((UiString::Search.resolve(), layout_direction()));
                ProvideLayoutDirection(LayoutDirection::Ltr, || {
                    assert_eq!(layout_direction(), LayoutDirection::Ltr);
                });
            });
            seen.push((tr!("Save", id = "save").to_string(), layout_direction()));
        });
    });
    assert_eq!(
        seen,
        [
            ("Enregistrer".into(), LayoutDirection::Ltr),
            ("بحث".into(), LayoutDirection::Rtl),
            ("Enregistrer".into(), LayoutDirection::Ltr)
        ]
    );
}

#[test]
fn separate_compositions_and_framework_overrides_do_not_share_language() {
    let mut first = String::new();
    let mut second = String::new();
    let _a = run_test_composition(|| {
        ProvideLocalization(&catalog(), locale("fr"), || {
            first = UiString::Copy.resolve();
        });
    });
    let _b = run_test_composition(|| {
        ProvideLocalization(&catalog(), locale("en"), || {
            second = UiString::Copy.resolve();
        });
    });
    assert_eq!(first, "Copier autrement");
    assert_eq!(second, "Copy");
}

#[test]
fn generated_accessors_and_borrowed_arguments_work_without_a_provider() {
    let mut seen = Vec::new();
    run_test_composition(|| {
        let name = "Ana".to_owned();
        seen.push(tr!("Hello, {name}!", name = name.as_str()).to_string());
        seen.push(messages::files(2).to_string());
        seen.push(messages::save().to_string());
    });
    assert!(seen[0].contains("Ana"));
    assert!(seen[1].contains("files"));
    assert_eq!(seen[2], "Save");
}

#[test]
fn replacing_catalogs_and_changing_arguments_refreshes_text() {
    let mut composition = run_test_composition(|| {});
    let mut result = String::new();
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
            .render(905, || {
                ProvideLocalization(&catalog, locale("fr"), || {
                    result = messages::files(count).to_string();
                });
            })
            .expect("render");
        assert!(result.contains(expected));
        assert!(result.contains(&count.to_string()));
    }
}
