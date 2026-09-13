#[path = "test_screens/liquid_tab_reference.rs"]
mod liquid_tab_reference;

fn main() -> anyhow::Result<()> {
    let dark = std::env::var("REFERENCE_SCHEME").as_deref() == Ok("dark");
    let checkerboard = std::env::var("REFERENCE_BACKDROP").as_deref() != Ok("solid");
    cranpose::AppLauncher::new()
        .with_title("Cranpose Liquid Reference")
        .with_size(402, 874)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_system_fonts(
            &cranpose::text::FontFamily::SansSerif,
            &[
                cranpose::text::FontWeight::NORMAL,
                cranpose::text::FontWeight::BOLD,
            ],
        )
        .with_fonts_from(|registry| {
            let Some(directory) = cranpose::system_font_directory() else {
                return Ok(());
            };
            for (weight, axis_weight) in [
                (cranpose::text::FontWeight::MEDIUM, 510.0),
                (cranpose::text::FontWeight::SEMI_BOLD, 590.0),
            ] {
                registry.register_system_face_with_variations(
                    directory,
                    &cranpose::text::FontFamily::SansSerif,
                    weight,
                    cranpose::text::FontStyle::Normal,
                    &[(*b"opsz", 17.0), (*b"wght", axis_weight)],
                )?;
            }
            Ok(())
        })
        .try_run(move || liquid_tab_reference::LiquidTabReference(checkerboard, dark))?;
    Ok(())
}
