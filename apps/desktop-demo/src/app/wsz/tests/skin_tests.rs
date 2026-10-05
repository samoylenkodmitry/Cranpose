use super::*;

#[test]
fn load_skin_accepts_nested_case_insensitive_sheets() {
    use std::io::Write;

    let bundled = include_bytes!("../../../../assets/catamp-silverplay.wsz");
    let mut original = zip::ZipArchive::new(Cursor::new(bundled)).expect("bundled archive");
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..original.len() {
        let mut entry = original.by_index(index).expect("skin entry");
        let prefix = if index % 2 == 0 { "SKINS\\" } else { "skins/" };
        nested
            .start_file(
                format!("{prefix}{}", entry.name().to_ascii_uppercase()),
                zip::write::SimpleFileOptions::default(),
            )
            .expect("nested entry");
        std::io::copy(&mut entry, &mut nested).expect("copy entry");
    }
    nested
        .start_file("unused.bmp", zip::write::SimpleFileOptions::default())
        .expect("unused entry");
    nested.write_all(b"not an image").expect("unused bytes");
    let bytes = nested.finish().expect("nested archive").into_inner();
    assert!(load_skin(&bytes).expect("nested skin") == load_skin(bundled).expect("bundled skin"));
}

#[test]
fn load_skin_reports_missing_sheets() {
    let empty = zip::ZipWriter::new(Cursor::new(Vec::new()))
        .finish()
        .expect("empty archive")
        .into_inner();
    let error = load_skin(&empty).err().expect("missing skin sheets");
    assert!(error
        .to_string()
        .contains("missing required skin entry: main.bmp"));
}

#[test]
fn load_bundled_skin_dimensions_match_classic_template() {
    let wsz = include_bytes!("../../../../assets/catamp-silverplay.wsz");
    let skin = load_skin(wsz).expect("bundled skin should load");
    assert_eq!(skin.main.width(), 275);
    assert_eq!(skin.main.height(), 116);
    assert_eq!(skin.titlebar.width(), 344);
    assert_eq!(skin.cbuttons.width(), 136);
    assert_eq!(skin.cbuttons.height(), 36);
    assert_eq!(skin.posbar.width(), 307);
    assert_eq!(skin.posbar.height(), 10);
}
