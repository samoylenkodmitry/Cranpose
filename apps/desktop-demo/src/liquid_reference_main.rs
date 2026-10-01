#[path = "test_screens/liquid_control_reference.rs"]
mod liquid_control_reference;
#[path = "test_screens/liquid_tab_reference.rs"]
mod liquid_tab_reference;

fn main() -> anyhow::Result<()> {
    let dark = std::env::var("REFERENCE_SCHEME").as_deref() == Ok("dark");
    let checkerboard = std::env::var("REFERENCE_BACKDROP").as_deref() != Ok("solid");
    let gray = std::env::var("REFERENCE_BACKDROP")
        .ok()
        .and_then(|value| {
            value
                .strip_prefix("gray-")
                .and_then(|number| number.parse::<f32>().ok())
        })
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 1.0));
    let control = std::env::var("REFERENCE_COMPONENT")
        .ok()
        .map(|name| liquid_control_reference::Control::parse(&name))
        .transpose()?;
    let initial = std::env::var("REFERENCE_VALUE")
        .unwrap_or_else(|_| "0".to_string())
        .parse()?;
    cranpose::AppLauncher::new()
        .with_title("Cranpose Liquid Reference")
        .with_size(402, 874)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_fonts_from(|registry| {
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
        })
        .try_run(move || {
            if let Some(control) = control {
                liquid_control_reference::LiquidControlReference(
                    control,
                    checkerboard,
                    dark,
                    initial,
                    gray,
                );
            } else {
                liquid_tab_reference::LiquidTabReference(checkerboard, dark);
            }
        })?;
    Ok(())
}
