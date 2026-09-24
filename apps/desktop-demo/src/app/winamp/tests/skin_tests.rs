use super::*;

#[test]
fn normalize_name_extracts_file_name() {
    assert_eq!(normalize_name("SKINS\\MAIN.BMP"), "main.bmp");
    assert_eq!(normalize_name("foo/bar/PLAYPAUS.BMP"), "playpaus.bmp");
}

#[test]
fn load_bundled_skin_dimensions_match_classic_template() {
    let wsz = include_bytes!("../../../../assets/winamp.wsz");
    let skin = load_skin(wsz).expect("bundled skin should load");
    assert_eq!(skin.main.width(), 275);
    assert_eq!(skin.main.height(), 116);
    assert_eq!(skin.titlebar.width(), 344);
    assert_eq!(skin.cbuttons.width(), 136);
    assert_eq!(skin.cbuttons.height(), 36);
    assert_eq!(skin.posbar.width(), 307);
    assert_eq!(skin.posbar.height(), 10);
}
