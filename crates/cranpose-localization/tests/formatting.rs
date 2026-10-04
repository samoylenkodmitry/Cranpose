use cranpose_localization::{
    Argument, Catalog, Locale, Message, PreviewMode, Resource, SourceCatalog,
};

static SOURCE: SourceCatalog = SourceCatalog::new(
    "en",
    "hello = Hello, { $name }!\nfiles = { $count ->\n [one] One file\n *[other] { $count } files\n}\nplain = Source\n",
);
static HELLO: Message = Message::new("app", "hello", &SOURCE);
static FILES: Message = Message::new("app", "files", &SOURCE);
static PLAIN: Message = Message::new("app", "plain", &SOURCE);

fn visible(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, '\u{2068}' | '\u{2069}'))
        .collect()
}

fn catalog(resources: &[Resource<'_>]) -> Catalog {
    Catalog::from_resources("en", resources).expect("valid catalogs")
}

fn locale(tag: &str) -> Locale {
    Locale::parse(tag).expect("valid locale")
}

#[test]
fn translated_sentences_can_reorder_borrowed_arguments() {
    let catalog = catalog(&[Resource {
        locale: "fr",
        namespace: "app",
        source: "hello = { $name }, bonjour !",
    }]);
    let name = String::from("Ana");
    let text = catalog
        .translator(locale("fr-CA"))
        .format(&HELLO, &[Argument::new("name", &name)])
        .expect("format");
    assert_eq!(visible(&text), "Ana, bonjour !");
    assert!(
        text.contains('\u{2068}'),
        "interpolated text keeps bidi isolation"
    );
}

#[test]
fn numeric_arguments_choose_arabic_plural_categories() {
    let catalog = catalog(&[Resource {
        locale: "ar",
        namespace: "app",
        source: "files = { $count ->\n [zero] zero\n [one] one\n [two] two\n [few] few\n [many] many\n *[other] other\n}",
    }]);
    let translator = catalog.translator(locale("ar"));
    for (count, expected) in [
        (0, "zero"),
        (1, "one"),
        (2, "two"),
        (3, "few"),
        (11, "many"),
        (100, "other"),
    ] {
        assert_eq!(
            translator
                .format(&FILES, &[Argument::new("count", count)])
                .expect("plural"),
            expected
        );
    }
}

#[test]
fn absent_or_broken_translations_use_source_language_plural_rules() {
    let catalog = catalog(&[Resource {
        locale: "ar",
        namespace: "app",
        source: "files = { $missing }",
    }]);
    let translator = catalog.translator(locale("ar"));
    assert_eq!(
        visible(
            &translator
                .format(&FILES, &[Argument::new("count", 2)])
                .expect("fallback")
        ),
        "2 files"
    );
    assert_eq!(translator.format(&PLAIN, &[]).expect("source"), "Source");
}

#[test]
fn fallback_catalog_language_precedes_inline_source() {
    let catalog = catalog(&[Resource {
        locale: "en",
        namespace: "app",
        source: "plain = Catalog fallback",
    }]);
    assert_eq!(
        catalog
            .translator(locale("ja"))
            .format(&PLAIN, &[])
            .expect("fallback"),
        "Catalog fallback"
    );
}

#[test]
fn script_preferences_and_order_are_respected() {
    let catalog = catalog(&[
        Resource {
            locale: "sr-Latn",
            namespace: "app",
            source: "plain = Latinica",
        },
        Resource {
            locale: "sr-Cyrl",
            namespace: "app",
            source: "plain = Ћирилица",
        },
        Resource {
            locale: "fr",
            namespace: "app",
            source: "plain = Français",
        },
    ]);
    assert_eq!(
        catalog
            .translator(locale("sr-Latn-RS"))
            .format(&PLAIN, &[])
            .expect("Latin"),
        "Latinica"
    );
    assert_eq!(
        catalog
            .translator(locale("sr-Cyrl-RS"))
            .format(&PLAIN, &[])
            .expect("Cyrillic"),
        "Ћирилица"
    );
    assert_eq!(
        catalog
            .negotiate(&[locale("ja"), locale("fr")])
            .format(&PLAIN, &[])
            .expect("preferences"),
        "Français"
    );
}

#[test]
fn application_overrides_do_not_hide_other_library_messages() {
    let library = catalog(&[Resource {
        locale: "fr",
        namespace: "app",
        source: "plain = Bibliothèque\nhello = Bonjour { $name }",
    }]);
    let app = catalog(&[Resource {
        locale: "fr",
        namespace: "app",
        source: "plain = Application",
    }])
    .with_fallback(&library);
    let translator = app.translator(locale("fr"));
    assert_eq!(
        translator.format(&PLAIN, &[]).expect("override"),
        "Application"
    );
    assert_eq!(
        visible(
            &translator
                .format(&HELLO, &[Argument::new("name", "Ana")])
                .expect("library")
        ),
        "Bonjour Ana"
    );
}

#[test]
fn an_invalid_application_override_uses_the_library_in_the_same_language() {
    let library = catalog(&[Resource {
        locale: "fr",
        namespace: "app",
        source: "plain = Bibliothèque",
    }]);
    let app = catalog(&[Resource {
        locale: "fr",
        namespace: "app",
        source: "plain = { $missing }",
    }])
    .with_fallback(&library);
    assert_eq!(
        app.translator(locale("fr"))
            .format(&PLAIN, &[])
            .expect("library fallback"),
        "Bibliothèque"
    );
}

#[test]
fn a_catalog_can_be_shared_with_a_background_worker() {
    let catalog = catalog(&[]);
    let result = std::thread::spawn(move || catalog.translator(locale("en")).format(&PLAIN, &[]))
        .join()
        .expect("worker");
    assert_eq!(result.expect("source"), "Source");
}

#[test]
fn invalid_catalogs_and_missing_arguments_are_reported() {
    for source in [
        "hello =\n",
        "hello = One\nhello = Two",
        "hello = One\n-hello = Two",
        "NUMBER = reserved",
    ] {
        assert!(
            Catalog::from_resources(
                "en",
                &[Resource {
                    locale: "en",
                    namespace: "app",
                    source
                }]
            )
            .is_err()
        );
    }
    assert!(Locale::parse("not a language").is_err());
    assert!(HELLO.format_source(&[]).is_err());
    assert!(HELLO.fallback_text(&[]).starts_with("Hello,"));
}

#[test]
fn number_builtin_is_available_in_catalogs_and_source_fallbacks() {
    static SOURCE: SourceCatalog = SourceCatalog::new(
        "en",
        "rank = { NUMBER($count, type: \"ordinal\") ->\n [one] first\n [two] second\n *[other] other\n}",
    );
    static RANK: Message = Message::new("app", "rank", &SOURCE);
    assert_eq!(
        RANK.format_source(&[Argument::new("count", 2)])
            .expect("NUMBER source"),
        "second"
    );
    let translator = catalog(&[Resource { locale: "en", namespace: "app", source: "rank = { NUMBER($count, type: \"ordinal\") ->\n [one] first\n [two] second\n *[other] other\n}" }]).translator(locale("en"));
    assert_eq!(
        translator
            .format(&RANK, &[Argument::new("count", 1)])
            .expect("NUMBER catalog"),
        "first"
    );
}

#[test]
fn previews_expand_text_and_set_the_requested_direction() {
    let catalog = catalog(&[]);
    let expanded = catalog.translator(locale("en").with_preview(PreviewMode::Expanded));
    assert!(
        expanded
            .format(&PLAIN, &[])
            .expect("preview")
            .chars()
            .count()
            > "Source".len()
    );
    let rtl = locale("en").with_preview(PreviewMode::Rtl);
    assert!(rtl.is_rtl());
    assert!(
        catalog
            .translator(rtl)
            .format(&PLAIN, &[])
            .expect("preview")
            .starts_with('\u{2067}')
    );
    assert!(locale("ar").is_rtl());
    assert!(!locale("ar-Latn").is_rtl());
}
