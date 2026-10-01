pub static DEMO_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/NotoSansMerged.ttf"),
    include_bytes!("../assets/NotoSansBold.ttf"),
    include_bytes!("../assets/TwemojiMozilla.ttf"),
];

/// Registers the system font variations used by the native Liquid comparison fixtures.
pub fn register_liquid_reference_fonts(
    registry: &mut cranpose::SoftwareTextFontRegistry,
) -> Result<(), cranpose::FontLoadError> {
    let Some(directory) = cranpose::system_font_directory() else {
        return Ok(());
    };
    for (weight, axis_weight, optical_size) in [
        (cranpose::text::FontWeight::NORMAL, 400.0, 17.0),
        (cranpose::text::FontWeight::MEDIUM, 510.0, 17.0),
        (cranpose::text::FontWeight::SEMI_BOLD, 590.0, 17.0),
        (cranpose::text::FontWeight::BOLD, 700.0, 28.0),
    ] {
        registry.register_system_face_with_variations(
            directory,
            &cranpose::text::FontFamily::SansSerif,
            weight,
            cranpose::text::FontStyle::Normal,
            &[(*b"opsz", optical_size), (*b"wght", axis_weight)],
        )?;
    }
    Ok(())
}
