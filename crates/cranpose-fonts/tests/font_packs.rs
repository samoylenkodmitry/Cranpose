#![cfg(any(feature = "arabic", feature = "devanagari", feature = "cjk"))]

use ab_glyph::{Font, FontRef};

#[cfg(feature = "arabic")]
#[test]
fn arabic_pack_covers_arabic_text() {
    assert_pack_covers(cranpose_fonts::ARABIC, "العربية");
}

#[cfg(feature = "devanagari")]
#[test]
fn devanagari_pack_covers_devanagari_text() {
    assert_pack_covers(cranpose_fonts::DEVANAGARI, "हिन्दी");
}

#[cfg(feature = "cjk")]
#[test]
fn cjk_pack_covers_cjk_text_beyond_common_ui_labels() {
    assert_pack_covers(cranpose_fonts::CJK, "髙");
}

fn assert_pack_covers(pack: cranpose_fonts::FontPack, text: &str) {
    assert_eq!(pack.weights(), &[400, 500, 600, 700, 800]);
    let font = FontRef::try_from_slice(pack.bytes()).expect("valid bundled font");
    for character in text.chars() {
        assert_ne!(font.glyph_id(character).0, 0, "missing {character:?}");
    }
}
