use super::*;

#[test]
fn font_family_maps_compose_generic_names() {
    assert_eq!(FontFamily::from_name("Default"), FontFamily::Default);
    assert_eq!(FontFamily::from_name("sans-serif"), FontFamily::SansSerif);
    assert_eq!(FontFamily::from_name("serif"), FontFamily::Serif);
    assert_eq!(FontFamily::from_name("monospace"), FontFamily::Monospace);
    assert_eq!(FontFamily::from_name("cursive"), FontFamily::Cursive);
    assert_eq!(FontFamily::from_name("fantasy"), FontFamily::Fantasy);
}

#[test]
fn font_family_preserves_custom_names() {
    let family = FontFamily::named("Fira Sans");
    assert_eq!(family, FontFamily::Named("Fira Sans".to_string()));
    assert_eq!(family.family_name(), Some("Fira Sans"));
}

#[test]
fn font_family_file_backed_preserves_font_entries() {
    let family = FontFamily::file_backed(vec![
        FontFile::new("fixtures/fonts/Roboto-Regular.ttf"),
        FontFile::new("fixtures/fonts/Roboto-Bold.ttf").with_weight(FontWeight::BOLD),
    ])
    .expect("valid file-backed font family");
    let FontFamily::FileBacked(file_backed) = family else {
        panic!("expected file-backed family");
    };
    assert_eq!(file_backed.fonts.len(), 2);
    assert_eq!(
        file_backed.fonts[0].path,
        "fixtures/fonts/Roboto-Regular.ttf"
    );
    assert_eq!(file_backed.fonts[0].weight, FontWeight::NORMAL);
    assert_eq!(file_backed.fonts[1].path, "fixtures/fonts/Roboto-Bold.ttf");
    assert_eq!(file_backed.fonts[1].weight, FontWeight::BOLD);
}

#[test]
fn font_file_builder_applies_style_and_weight() {
    let file = FontFile::new("fixtures/fonts/Roboto-Italic.ttf")
        .with_weight(FontWeight::MEDIUM)
        .with_style(FontStyle::Italic);
    assert_eq!(file.path, "fixtures/fonts/Roboto-Italic.ttf");
    assert_eq!(file.weight, FontWeight::MEDIUM);
    assert_eq!(file.style, FontStyle::Italic);
}

#[test]
fn file_backed_font_family_rejects_empty_font_list() {
    assert_eq!(
        FileBackedFontFamily::new(Vec::new()),
        Err(FontFamilyError::EmptyFileList)
    );
}

#[test]
fn font_family_loaded_typeface_path_preserves_path() {
    let family = FontFamily::loaded_typeface_path("fixtures/fonts/FiraSans-Regular.ttf");
    let FontFamily::LoadedTypeface(typeface) = family else {
        panic!("expected loaded typeface family");
    };
    assert_eq!(typeface.path, "fixtures/fonts/FiraSans-Regular.ttf");
}

#[test]
fn font_weight_default_is_normal() {
    assert_eq!(FontWeight::default(), FontWeight::NORMAL);
}

#[test]
fn font_weight_try_new_validates_range() {
    assert_eq!(FontWeight::try_new(0), None);
    assert_eq!(FontWeight::try_new(1), Some(FontWeight(1)));
    assert_eq!(FontWeight::try_new(1000), Some(FontWeight(1000)));
    assert_eq!(FontWeight::try_new(1001), None);
}

#[test]
fn font_weight_new_clamps_invalid_input() {
    assert_eq!(FontWeight::new(0), FontWeight(1));
    assert_eq!(FontWeight::new(500), FontWeight(500));
    assert_eq!(FontWeight::new(1001), FontWeight(1000));
}
