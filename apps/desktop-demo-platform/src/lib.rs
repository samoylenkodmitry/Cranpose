#[cfg(any(
    all(feature = "android", target_os = "android", feature = "renderer-wgpu"),
    all(feature = "web", target_arch = "wasm32", feature = "renderer-wgpu")
))]
use cranpose::AppLauncher;

#[cfg(any(
    all(feature = "android", target_os = "android", feature = "renderer-wgpu"),
    all(feature = "web", target_arch = "wasm32", feature = "renderer-wgpu")
))]
fn create_app() -> AppLauncher {
    AppLauncher::new()
        .with_title("Cranpose Demo")
        .with_size(800, 600)
        .with_web_fill_viewport(true)
        .with_fonts(desktop_demo::fonts::DEMO_FONTS)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
}

cranpose::android_main! {
    launcher: create_app(),
    content: desktop_demo::app::combined_app,
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
use wasm_bindgen::prelude::*;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen(start)]
pub fn web_init() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    console_error_panic_hook::set_once();
    log::info!("Cranpose demo starting in browser");
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen]
pub async fn run_app(
    initial_tab: Option<String>,
    initial_shader_section: Option<String>,
) -> Result<(), JsValue> {
    log::info!("Initializing Cranpose app");

    let requested_tab = initial_tab
        .as_deref()
        .and_then(desktop_demo::app::DemoTab::from_startup_name);
    let requested_shader_section = initial_shader_section
        .as_deref()
        .and_then(desktop_demo::app::ShaderSection::from_startup_name);
    let startup = desktop_demo::app::StartupSelection::from_requested(
        requested_tab,
        requested_shader_section,
    );

    if let Some(requested) = initial_tab.as_deref() {
        match requested_tab {
            Some(tab) => {
                log::info!(
                    "Applying startup tab override: requested='{}' resolved='{}'",
                    requested,
                    tab.label()
                );
            }
            None => {
                log::warn!(
                    "Ignoring unknown startup tab override '{}'; continuing with default tab",
                    requested
                );
            }
        }
    }
    if let Some(requested) = initial_shader_section.as_deref() {
        match requested_shader_section {
            Some(section) if startup.initial_shader_section == Some(section) => {
                log::info!(
                    "Applying startup shader section override: requested='{}' resolved='{}'",
                    requested,
                    section.label()
                );
            }
            Some(section) => {
                log::warn!(
                    "Ignoring startup shader section override '{}' ('{}') because the active startup tab is '{}'",
                    requested,
                    section.label(),
                    startup
                        .initial_tab
                        .unwrap_or(desktop_demo::app::DemoTab::Counter)
                        .label()
                );
            }
            None => {
                log::warn!(
                    "Ignoring unknown startup shader section override '{}'; continuing with default shaders content",
                    requested
                );
            }
        }
    }

    create_app()
        .run_web("cranpose-canvas", move || {
            desktop_demo::app::combined_app_with_startup(startup);
        })
        .await
}
