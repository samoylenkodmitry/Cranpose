use std::io::Cursor;

use cranpose_ui::text::{SpanStyle, TextStyle};

use super::*;

const REGULAR: &[u8] = include_bytes!("../../assets/NotoSansMerged.ttf");
const BOLD: &[u8] = include_bytes!("../../assets/NotoSansBold.ttf");

fn style_for(family: &FontFamily, weight: FontWeight) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_family: Some(family.clone()),
            font_weight: Some(weight),
            ..Default::default()
        },
        ..Default::default()
    }
}

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(name: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/test-output/cranpose-font-source")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("scratch font file");
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn register_family_picks_the_face_matching_the_requested_weight() {
    let dir = ScratchDir::new("weights");
    let regular = dir.write("Test-Regular.ttf", REGULAR);
    let bold = dir.write("Test-Bold.ttf", BOLD);
    let family = FontFamily::file_backed(vec![
        FontFile::new(regular.to_string_lossy().into_owned()),
        FontFile::new(bold.to_string_lossy().into_owned()).with_weight(FontWeight::BOLD),
    ])
    .expect("file-backed family");

    let mut registry = SoftwareTextFontRegistry::new();
    registry.register_family(&family).expect("family loads");
    let fonts = registry.into_font_set(&[]);

    let resolved_regular = fonts
        .resolve(&style_for(&family, FontWeight::NORMAL))
        .expect("regular face");
    let resolved_bold = fonts
        .resolve(&style_for(&family, FontWeight::BOLD))
        .expect("bold face");

    assert_eq!(resolved_regular.weight(), FontWeight::NORMAL);
    assert_eq!(resolved_bold.weight(), FontWeight::BOLD);
    assert_ne!(
        resolved_regular.content_hash(),
        resolved_bold.content_hash(),
        "distinct faces must key the glyph atlas distinctly"
    );
}

#[test]
fn register_family_honours_a_declared_weight_over_the_face_header() {
    let dir = ScratchDir::new("declared");
    let path = dir.write("Test-Regular.ttf", REGULAR);
    let family = FontFamily::file_backed(vec![
        FontFile::new(path.to_string_lossy().into_owned()).with_weight(FontWeight::MEDIUM),
    ])
    .expect("file-backed family");

    let mut registry = SoftwareTextFontRegistry::new();
    registry.register_family(&family).expect("family loads");
    let fonts = registry.into_font_set(&[]);

    let resolved = fonts
        .resolve(&style_for(&family, FontWeight::MEDIUM))
        .expect("declared face");
    assert_eq!(resolved.weight(), FontWeight::MEDIUM);
}

#[test]
fn register_family_reports_a_missing_file_without_panicking() {
    let dir = ScratchDir::new("missing");
    let family = FontFamily::file_backed(vec![FontFile::new(
        dir.path().join("Absent.ttf").to_string_lossy().into_owned(),
    )])
    .expect("file-backed family");

    let mut registry = SoftwareTextFontRegistry::new();
    let error = registry
        .register_family(&family)
        .expect_err("a missing file cannot register");
    assert!(matches!(error, FontLoadError::Read { .. }), "{error}");
    assert!(registry.is_empty());
}

#[test]
fn register_family_reports_a_corrupt_file_without_panicking() {
    let dir = ScratchDir::new("corrupt");
    let path = dir.write("Corrupt.ttf", b"this is not a font");
    let family = FontFamily::file_backed(vec![FontFile::new(path.to_string_lossy().into_owned())])
        .expect("file-backed family");

    let mut registry = SoftwareTextFontRegistry::new();
    let error = registry
        .register_family(&family)
        .expect_err("a corrupt file cannot register");
    assert!(matches!(error, FontLoadError::Parse { .. }), "{error}");
    assert!(registry.is_empty());
}

#[test]
fn a_family_that_failed_to_load_falls_back_to_another_app_face() {
    let dir = ScratchDir::new("fallback");
    let family = FontFamily::file_backed(vec![FontFile::new(
        dir.path().join("Absent.ttf").to_string_lossy().into_owned(),
    )])
    .expect("file-backed family");

    let mut registry = SoftwareTextFontRegistry::new();
    let _ = registry.register_family(&family);
    let fonts = registry.into_font_set(&[REGULAR]);

    let resolved = fonts
        .resolve(&style_for(&family, FontWeight::NORMAL))
        .expect("fallback face");
    assert_eq!(
        resolved.content_hash(),
        fonts.default_font().expect("default face").content_hash()
    );
}

#[test]
fn a_partly_loadable_family_keeps_the_faces_that_did_load() {
    let dir = ScratchDir::new("partial");
    let regular = dir.write("Test-Regular.ttf", REGULAR);
    let family = FontFamily::file_backed(vec![
        FontFile::new(regular.to_string_lossy().into_owned()),
        FontFile::new(dir.path().join("Absent.ttf").to_string_lossy().into_owned())
            .with_weight(FontWeight::BOLD),
    ])
    .expect("file-backed family");

    let mut registry = SoftwareTextFontRegistry::new();
    registry
        .register_family(&family)
        .expect("one readable face is enough");
    assert_eq!(registry.faces().len(), 1);
}

#[test]
fn register_face_reader_accepts_a_font_that_is_not_a_file() {
    let family = FontFamily::named("Bundled");
    let mut registry = SoftwareTextFontRegistry::new();
    registry
        .register_face_reader(
            &family,
            FontWeight::NORMAL,
            FontStyle::Normal,
            &mut Cursor::new(REGULAR.to_vec()),
        )
        .expect("streamed face loads");

    let fonts = registry.into_font_set(&[]);
    let resolved = fonts
        .resolve(&style_for(&family, FontWeight::NORMAL))
        .expect("streamed face");
    assert_eq!(resolved.registered_family(), {
        let mut expected = SoftwareTextFontRegistry::new();
        expected
            .register_face_bytes(
                &family,
                FontWeight::NORMAL,
                FontStyle::Normal,
                REGULAR.to_vec(),
            )
            .expect("face loads");
        expected.faces()[0].registered_family()
    });
}

#[test]
fn an_empty_registry_leaves_the_embedded_face_to_the_launcher() {
    let fonts = SoftwareTextFontRegistry::new().into_font_set(&[]);
    assert!(
        fonts.faces().is_empty(),
        "a registry must not reference the embedded face, or no binary can drop it"
    );
    assert!(fonts.resolve(&TextStyle::default()).is_none());
}

#[test]
fn system_sans_resolves_the_apple_variable_face() {
    let directory = ScratchDir::new("apple-system-sans");
    let core = directory.path().join("Core");
    std::fs::create_dir(&core).expect("Core directory");
    let path = core.join("SFUI.ttf");
    std::fs::write(&path, []).expect("system font");
    for weight in [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::BOLD] {
        assert_eq!(
            system_font_file(directory.path(), &FontFamily::SansSerif, weight),
            Some(path.clone())
        );
    }
    assert_eq!(
        system_font_file(directory.path(), &FontFamily::Serif, FontWeight::NORMAL),
        None
    );
}

#[test]
fn system_font_file_prefers_a_weight_specific_static_face() {
    let dir = ScratchDir::new("system-static");
    dir.write("Roboto-Regular.ttf", REGULAR);
    let medium = dir.write("Roboto-Medium.ttf", BOLD);

    assert_eq!(
        system_font_file(dir.path(), &FontFamily::SansSerif, FontWeight::MEDIUM),
        Some(medium)
    );
}

#[test]
fn system_font_file_falls_back_to_the_regular_face_for_other_weights() {
    let dir = ScratchDir::new("system-regular");
    let regular = dir.write("Roboto-Regular.ttf", REGULAR);

    assert_eq!(
        system_font_file(dir.path(), &FontFamily::SansSerif, FontWeight::MEDIUM),
        Some(regular)
    );
}

#[test]
fn system_font_file_reports_nothing_when_the_directory_is_empty() {
    let dir = ScratchDir::new("system-empty");
    assert_eq!(
        system_font_file(dir.path(), &FontFamily::SansSerif, FontWeight::NORMAL),
        None
    );
    assert_eq!(
        system_font_file(dir.path(), &FontFamily::named("Roboto"), FontWeight::NORMAL),
        None
    );
}

#[test]
fn register_system_family_binds_the_generic_alias_to_the_platform_face() {
    let dir = ScratchDir::new("system-family");
    dir.write("Roboto-Regular.ttf", REGULAR);
    dir.write("Roboto-Bold.ttf", BOLD);

    let mut registry = SoftwareTextFontRegistry::new();
    registry
        .register_system_family(
            dir.path(),
            &FontFamily::SansSerif,
            DEFAULT_SYSTEM_FAMILY_WEIGHTS,
        )
        .expect("system family loads");
    let fonts = registry.into_font_set(&[]);

    assert!(fonts.has_registered_family(&FontFamily::SansSerif));
    let bold = fonts
        .resolve(&style_for(&FontFamily::SansSerif, FontWeight::BOLD))
        .expect("bold system face");
    assert_eq!(bold.weight(), FontWeight::BOLD);
    assert_eq!(bold.registered_family(), {
        fonts
            .resolve(&style_for(&FontFamily::SansSerif, FontWeight::NORMAL))
            .expect("regular system face")
            .registered_family()
    });
}

#[test]
fn register_system_family_reports_an_absent_font_directory() {
    let mut registry = SoftwareTextFontRegistry::new();
    let error = registry
        .register_system_family(
            Path::new("/definitely/not/a/font/directory"),
            &FontFamily::SansSerif,
            DEFAULT_SYSTEM_FAMILY_WEIGHTS,
        )
        .expect_err("an absent directory cannot register");
    assert!(
        matches!(error, FontLoadError::NoSystemFontFile { .. }),
        "{error}"
    );
    assert!(registry.is_empty());
}

#[test]
fn a_system_weight_the_font_config_does_not_declare_resolves_to_the_declared_one() {
    for (requested, expected) in [(450u16, 400u16), (550, 500), (401, 400), (599, 500)] {
        assert_eq!(
            system_declared_weight(&FontFamily::SansSerif, FontWeight(requested)),
            FontWeight(expected),
            "sans-serif {requested}"
        );
    }
    for weight in DECLARED_HUNDREDS {
        assert_eq!(
            system_declared_weight(&FontFamily::SansSerif, FontWeight(*weight)),
            FontWeight(*weight)
        );
    }
    assert_eq!(
        system_declared_weight(&FontFamily::SansSerif, FontWeight(50)),
        FontWeight(100)
    );
    assert_eq!(
        system_declared_weight(&FontFamily::SansSerif, FontWeight(1000)),
        FontWeight(900)
    );
}

#[test]
fn a_sparse_system_family_resolves_by_distance_and_keeps_the_lighter_face_on_a_tie() {
    for (requested, expected) in [(500u16, 400u16), (550, 400), (600, 700), (650, 700)] {
        assert_eq!(
            system_declared_weight(&FontFamily::Serif, FontWeight(requested)),
            FontWeight(expected),
            "serif {requested}"
        );
    }
    assert_eq!(
        closest_declared_weight(&[300, 500], FontWeight(400)),
        Some(FontWeight(300))
    );
    assert_eq!(
        closest_declared_weight(&[500, 300], FontWeight(400)),
        Some(FontWeight(500)),
        "declaration order, not magnitude, is what breaks the tie"
    );
}

#[test]
fn a_family_with_no_system_files_keeps_the_weight_it_was_given() {
    assert_eq!(
        system_declared_weight(&FontFamily::named("Roboto"), FontWeight(450)),
        FontWeight(450)
    );
}

#[test]
fn explicit_system_axes_reject_invalid_registration_without_poisoning_retry() {
    let dir = ScratchDir::new("system-explicit-axes");
    dir.write("Roboto-Regular.ttf", REGULAR);
    let mut registry = SoftwareTextFontRegistry::new();
    assert!(matches!(
        registry.register_system_face_with_variations(
            dir.path(),
            &FontFamily::SansSerif,
            FontWeight::NORMAL,
            FontStyle::Normal,
            &[(*b"opsz", 17.0)],
        ),
        Err(FontLoadError::Parse {
            source: SoftwareTextFontError::InvalidVariation { .. },
            ..
        })
    ));
    assert!(registry.is_empty());
    registry
        .register_system_face_with_variations(
            dir.path(),
            &FontFamily::SansSerif,
            FontWeight::NORMAL,
            FontStyle::Normal,
            &[],
        )
        .unwrap();
    assert_eq!(registry.faces().len(), 1);
}

#[test]
fn explicit_byte_axes_reject_unknown_axis_without_registering() {
    let mut registry = SoftwareTextFontRegistry::new();
    assert!(
        registry
            .register_face_bytes_with_variations(
                &FontFamily::SansSerif,
                FontWeight::NORMAL,
                FontStyle::Normal,
                REGULAR,
                &[(*b"opsz", 17.0)],
            )
            .is_err()
    );
    assert!(registry.is_empty());
    registry
        .register_face_bytes_with_variations(
            &FontFamily::SansSerif,
            FontWeight::NORMAL,
            FontStyle::Normal,
            REGULAR,
            &[],
        )
        .unwrap();
    assert_eq!(registry.faces().len(), 1);
}

#[test]
fn register_system_face_registers_the_declared_weight_not_the_requested_one() {
    let dir = ScratchDir::new("system-undeclared-weight");
    dir.write("Roboto-Regular.ttf", REGULAR);

    let mut registry = SoftwareTextFontRegistry::new();
    registry
        .register_system_face(
            dir.path(),
            &FontFamily::SansSerif,
            FontWeight(450),
            FontStyle::Normal,
        )
        .expect("system face loads");
    let fonts = registry.into_font_set(&[]);

    let resolved = fonts
        .resolve(&style_for(&FontFamily::SansSerif, FontWeight(450)))
        .expect("a face for the off-grid request");
    assert_eq!(
        resolved.weight(),
        FontWeight::NORMAL,
        "450 must land on the 400 entry Android's matcher returns"
    );

    let mut declared = SoftwareTextFontRegistry::new();
    declared
        .register_system_face(
            dir.path(),
            &FontFamily::SansSerif,
            FontWeight::NORMAL,
            FontStyle::Normal,
        )
        .expect("system face loads");
    let declared = declared.into_font_set(&[]);
    assert_eq!(
        resolved.content_hash(),
        declared
            .resolve(&style_for(&FontFamily::SansSerif, FontWeight::NORMAL))
            .expect("the 400 face")
            .content_hash(),
        "the off-grid request must produce the very same face, glyph masks included"
    );
}

#[test]
fn system_requests_that_resolve_alike_register_one_face() {
    let dir = ScratchDir::new("system-dedupe");
    dir.write("Roboto-Regular.ttf", REGULAR);

    let mut registry = SoftwareTextFontRegistry::new();
    for weight in [400u16, 450, 499] {
        registry
            .register_system_face(
                dir.path(),
                &FontFamily::SansSerif,
                FontWeight(weight),
                FontStyle::Normal,
            )
            .expect("system face loads");
    }

    assert_eq!(
        registry.faces().len(),
        1,
        "three requests for one declared entry are one face"
    );
}

#[test]
fn an_app_registered_face_keeps_the_weight_it_declares() {
    let dir = ScratchDir::new("app-off-grid");
    let path = dir.write("Test-Regular.ttf", REGULAR);
    let family = FontFamily::file_backed(vec![
        FontFile::new(path.to_string_lossy().into_owned()).with_weight(FontWeight(450)),
    ])
    .expect("file-backed family");

    let mut registry = SoftwareTextFontRegistry::new();
    registry.register_family(&family).expect("family loads");
    let fonts = registry.into_font_set(&[]);

    assert_eq!(
        fonts
            .resolve(&style_for(&family, FontWeight(450)))
            .expect("app face")
            .weight(),
        FontWeight(450)
    );
}

#[test]
fn register_family_rejects_a_family_that_names_no_files() {
    let mut registry = SoftwareTextFontRegistry::new();
    let error = registry
        .register_family(&FontFamily::named("Roboto"))
        .expect_err("a named family has nothing to read");
    assert!(matches!(error, FontLoadError::NotFileBacked), "{error}");
}

#[test]
fn loaded_typeface_families_register_their_single_file() {
    let dir = ScratchDir::new("typeface");
    let path = dir.write("Test-Regular.ttf", REGULAR);
    let family = FontFamily::loaded_typeface_path(path.to_string_lossy().into_owned());

    let mut registry = SoftwareTextFontRegistry::new();
    registry.register_family(&family).expect("typeface loads");
    let fonts = registry.into_font_set(&[]);

    assert!(fonts.has_registered_family(&family));
    assert!(
        fonts
            .resolve(&style_for(&family, FontWeight::NORMAL))
            .is_some_and(|font| font.registered_family().is_some())
    );
}
