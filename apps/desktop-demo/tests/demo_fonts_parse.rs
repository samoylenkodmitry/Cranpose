//! Probe: DEMO_FONTS must parse in debug builds (regression: fonts failed
//! under debug_assertions, collapsing slim text rendering + inflating
//! layout via fallback metrics).

#[test]
fn demo_fonts_parse_in_debug_builds() {
    for (index, bytes) in desktop_app::fonts::DEMO_FONTS.iter().enumerate() {
        let parsed = cranpose_render_common::software_text_raster::SoftwareTextFont::from_bytes(
            bytes.to_vec(),
        );
        assert!(
            parsed.is_ok(),
            "DEMO_FONTS[{index}] failed to parse: {:?}",
            parsed.err()
        );
    }
}
