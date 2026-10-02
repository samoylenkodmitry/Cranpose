#[path = "test_screens/liquid_control_reference.rs"]
mod liquid_control_reference;
#[path = "test_screens/liquid_tab_reference.rs"]
mod liquid_tab_reference;

fn main() -> anyhow::Result<()> {
    let dark = std::env::var("REFERENCE_SCHEME").as_deref() == Ok("dark");
    let backdrop = liquid_tab_reference::ReferenceBackdrop::parse(
        &std::env::var("REFERENCE_BACKDROP").unwrap_or_else(|_| "checkerboard".to_string()),
    )?;
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
        .with_fonts_from(desktop_app::fonts::register_liquid_reference_fonts)
        .try_run(move || {
            if let Some(control) = control {
                liquid_control_reference::LiquidControlReference(control, backdrop, dark, initial);
            } else {
                liquid_tab_reference::LiquidTabReference(backdrop, dark);
            }
        })?;
    Ok(())
}
