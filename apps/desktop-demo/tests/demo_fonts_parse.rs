#[test]
fn demo_fonts_parse_in_debug_builds() {
    for (index, bytes) in desktop_app::fonts::DEMO_FONTS.iter().enumerate() {
        let parsed = cranpose_text::Typeface::from_bytes(*bytes);
        assert!(
            parsed.is_ok(),
            "DEMO_FONTS[{index}] failed to parse: {:?}",
            parsed.err()
        );
    }
}
