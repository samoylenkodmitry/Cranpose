#![cfg(feature = "tooling")]

use std::{fs, process::Command};

use cranpose_localization::tooling::{
    TranslationCall, catalog_signatures, extract_sources, load_catalogs, merge_source_catalog,
    validate_catalogs,
};

#[test]
fn call_validation_rejects_missing_extra_and_duplicate_arguments() {
    for call in [
        r#""Hello {name}""#,
        r#""Hello", name = "Ana""#,
        r#""{name}", name = "Ana", name = "Bob""#,
        r#""Hello", id = """#,
        r#""Hello", source_locale = """#,
        r#""Hello", source_locale = "en", source_locale = "fr""#,
    ] {
        assert!(
            syn::parse_str::<TranslationCall>(call).is_err(),
            "accepted {call}"
        );
    }
    assert!(
        syn::parse_str::<TranslationCall>(r#""Hello {name}", name = user.name.as_str()"#).is_ok()
    );
}

#[test]
fn context_changes_identity_but_source_locations_do_not() {
    let a: TranslationCall = syn::parse_str(r#""Open", context = "verb""#).expect("call");
    let b: TranslationCall = syn::parse_str(r#""Open", context = "adjective""#).expect("call");
    assert_ne!(a.id, b.id);
    let explicit: TranslationCall =
        syn::parse_str(r#""New wording", id = "open-action""#).expect("call");
    assert_eq!(explicit.id, "open-action");
}

#[test]
fn extraction_sees_nested_macros_and_reports_conflicting_ids() {
    let directory = tempfile::tempdir().expect("directory");
    let file = directory.path().join("main.rs");
    fs::write(
        &file,
        r#"fn main() { layout! { tr!("Save", id = "save", comment = "Persists edits"); } }"#,
    )
    .expect("source");
    let extracted = extract_sources(directory.path(), "en").expect("extract");
    assert!(extracted.contains("# Persists edits"));
    assert!(extracted.contains("main.rs:1"));
    assert!(extracted.contains("save = Save"));
    fs::write(
        directory.path().join("other.rs"),
        r#"fn screen() { tr!("Save", id = "save", comment = "Toolbar action"); }"#,
    )
    .expect("same message");
    let extracted = extract_sources(directory.path(), "en").expect("merge guidance");
    assert!(extracted.contains("# Persists edits"));
    assert!(extracted.contains("# Toolbar action"));
    fs::write(
        directory.path().join("other.rs"),
        r#"fn screen() { tr!("Different", id = "save"); }"#,
    )
    .expect("source");
    assert!(extract_sources(directory.path(), "en").is_err());
}

#[test]
fn signatures_follow_references_and_reject_cycles() {
    let signatures =
        catalog_signatures("name = { $name }\nhello = Hello { name }").expect("references");
    assert!(signatures["hello"].variables.contains("name"));
    assert!(catalog_signatures("a = { b }\nb = { a }").is_err());
    assert!(catalog_signatures("a = { missing }").is_err());
    assert!(catalog_signatures("a = { UNKNOWN() }").is_err());
}

#[test]
fn translator_term_parameters_do_not_become_application_arguments() {
    let source = "-brand = { $case ->\n [acc] Cranpose\n *[other] Cranpose\n}\nhello = Welcome to { -brand(case: \"acc\") }";
    let signatures = catalog_signatures(source).expect("term parameters");
    assert!(signatures["hello"].variables.is_empty());
}

#[test]
fn catalog_checks_allow_omitted_arguments_but_reject_unknown_ones() {
    let directory = tempfile::tempdir().expect("directory");
    for (language, text) in [
        ("en", "hello = Hello { $name }\nbye = Bye"),
        ("fr", "hello = Bonjour"),
    ] {
        let path = directory.path().join(language);
        fs::create_dir(&path).expect("locale");
        fs::write(path.join("app.ftl"), text).expect("catalog");
    }
    let missing = validate_catalogs(&load_catalogs(directory.path()).expect("load"), "en")
        .expect("valid subset");
    assert_eq!(missing.len(), 1);
    fs::write(directory.path().join("en/extra.ftl"), "save = Save").expect("namespace");
    let missing = validate_catalogs(&load_catalogs(directory.path()).expect("load"), "EN")
        .expect("normalized locale");
    assert!(
        missing
            .iter()
            .any(|message| message == "fr/extra.ftl: missing `save`")
    );
    fs::write(directory.path().join("fr/app.ftl"), "hello = { $unknown }")
        .expect("invalid argument");
    assert!(validate_catalogs(&load_catalogs(directory.path()).expect("load"), "en").is_err());
}

#[test]
fn cli_extract_and_check_are_reproducible() {
    let directory = tempfile::tempdir().expect("directory");
    let source = directory.path().join("src");
    fs::create_dir(&source).expect("source dir");
    fs::write(
        source.join("lib.rs"),
        r#"fn label() { tr!("Save", id = "save"); }"#,
    )
    .expect("source");
    let output = directory.path().join("locales/en/app.ftl");
    let run = |check: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cranpose-l10n"));
        command
            .args(["extract", "--source"])
            .arg(&source)
            .arg("--output")
            .arg(&output);
        if check {
            command.arg("--check");
        }
        command.output().expect("CLI")
    };
    assert!(run(false).status.success());
    assert!(run(true).status.success());
    fs::write(
        source.join("lib.rs"),
        r#"fn label() { tr!("Save changes", id = "save"); }"#,
    )
    .expect("edit source");
    assert!(!run(true).status.success());
}

#[test]
fn extraction_preserves_handwritten_messages_and_translator_notes() {
    let previous = "manual = Handwritten\n\n# Translator note\n# Source: old.rs:2\nsave = Save\n\n# Source: gone.rs:1\nold = Removed\n";
    let extracted = "# Source: new.rs:4\nsave = Save changes\n";
    let merged = merge_source_catalog(previous, extracted).expect("merge");
    assert!(merged.contains("manual = Handwritten"));
    assert!(merged.contains("Translator note"));
    assert!(merged.contains("save = Save changes"));
    assert!(!merged.contains("old = Removed"));
    assert_eq!(
        merged,
        merge_source_catalog(&merged, extracted).expect("idempotent")
    );
}
