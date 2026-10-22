//! The usage descriptions an Apple bundle shows in each language the
//! application's catalogs translate them into.

use std::{fs, path::PathBuf};

use cranpose_capabilities::{Capabilities, Use, apple_usage_strings};

const USES: [Use; 3] = [
    Use::microphone("Hears speech to write it down."),
    Use::heart_rate("Shows the heart rate."),
    Use::haptics(),
];

const CAPABILITIES: Capabilities<'static> = Capabilities {
    uses: &USES,
    demands: &[],
    opens: &[],
    links: &[],
};

/// Writes `catalogs`, pairs of a language tag and its catalog, into a folder
/// of the test's own and returns the folder.
fn catalogs(name: &str, catalogs: &[(&str, &str)]) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    for (tag, catalog) in catalogs {
        let language = folder.join(tag);
        fs::create_dir_all(&language).expect("a language folder");
        fs::write(language.join("app.ftl"), catalog).expect("the catalog");
    }
    folder
}

#[test]
fn each_translated_language_gets_its_reasons_under_every_key() {
    let folder = catalogs(
        "translated",
        &[
            (
                "en",
                "# Source: build.rs:4\nm-1 = Hears speech to write it down.\nm-2 = Shows the heart rate.\nm-3 = Next\n",
            ),
            (
                "de",
                "m-1 = Hört zu, um es \"aufzuschreiben\".\nm-2 = Zeigt den Herzschlag.\nm-3 = Weiter\n",
            ),
            ("fr", "m-1 = Écoute la voix pour l’écrire.\n"),
            ("ja", "m-3 = 次へ\n"),
        ],
    );
    let strings = apple_usage_strings(&CAPABILITIES, &folder);
    let tags: Vec<&str> = strings.iter().map(|(tag, _)| tag.as_str()).collect();
    // Japanese translates none of the reasons, so the system shows the
    // Info.plist ones there.
    assert_eq!(tags, ["de", "en", "fr"]);
    assert_eq!(
        strings[0].1,
        "\"NSMicrophoneUsageDescription\" = \"Hört zu, um es \\\"aufzuschreiben\\\".\";\n\
         \"NSHealthShareUsageDescription\" = \"Zeigt den Herzschlag.\";\n\
         \"NSHealthUpdateUsageDescription\" = \"Zeigt den Herzschlag.\";\n"
    );
    assert_eq!(
        strings[2].1,
        "\"NSMicrophoneUsageDescription\" = \"Écoute la voix pour l’écrire.\";\n"
    );
}

#[test]
fn a_reason_no_catalog_holds_and_a_missing_folder_give_nothing() {
    let folder = catalogs("untranslated", &[("de", "m-3 = Weiter\n")]);
    assert!(apple_usage_strings(&CAPABILITIES, &folder).is_empty());
    let missing = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-catalogs");
    assert!(apple_usage_strings(&CAPABILITIES, &missing).is_empty());
}

#[test]
fn a_translation_with_a_variable_is_left_out() {
    let folder = catalogs(
        "variable",
        &[
            ("en", "m-1 = Hears speech to write it down.\n"),
            ("de", "m-1 = Hört { $name }.\n"),
        ],
    );
    let strings = apple_usage_strings(&CAPABILITIES, &folder);
    assert_eq!(strings.len(), 1);
    assert_eq!(strings[0].0, "en");
}
