pub mod app;
pub mod fonts;

#[cfg(test)]
mod tests;

pub mod test_screens;

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use cranpose::{AppFonts, AppLauncher};

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::fonts::DEMO_FONTS;

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn create_app() -> AppLauncher<AppFonts> {
    let dev_controls = cranpose::launch_args().string("test_screen").is_none();
    with_window_icon(
        AppLauncher::new()
            .with_title("Cranpose Demo")
            .with_size(800, 600)
            .with_fonts(DEMO_FONTS)
            .with_fps_counter(dev_controls)
            .with_frame_pacing_controls(dev_controls),
    )
}

/// Windows and Linux show this picture on the demo's windows and taskbar
/// entries. It is drawn from `assets/icon/icon.svg` like every other platform's
/// icon.
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_os = "macos"),
    not(target_arch = "wasm32")
))]
fn with_window_icon(launcher: AppLauncher<AppFonts>) -> AppLauncher<AppFonts> {
    match app::net_image::decode_bitmap(include_bytes!("../assets/icon/window-icon.png")) {
        Ok(icon) => launcher.with_window_icon(icon),
        Err(error) => {
            log::warn!("the demo's window icon is unusable: {error:#}");
            launcher
        }
    }
}

/// macOS shows the icon of the application bundle and ignores a window icon.
#[cfg(all(feature = "desktop", feature = "renderer-wgpu", target_os = "macos"))]
fn with_window_icon(launcher: AppLauncher<AppFonts>) -> AppLauncher<AppFonts> {
    launcher
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub fn init_logging() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub fn entry_point() {
    init_logging();
    if let Err(error) = create_app().try_run(app::DesktopApp) {
        eprintln!("Failed to launch Cranpose Demo: {error}");
        std::process::exit(1);
    }
}

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
use cranpose::{
    local_safe_area_insets, AppLauncher as IosAppLauncher, Box as CranposeBox, BoxSpec, Modifier,
};

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
use crate::fonts::DEMO_FONTS as IOS_DEMO_FONTS;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
#[cranpose::composable]
fn ios_root() {
    let insets = local_safe_area_insets().current();
    CranposeBox(
        Modifier::empty().fill_max_size().padding_each(
            insets.left,
            insets.top,
            insets.right,
            insets.bottom,
        ),
        BoxSpec::default(),
        app::combined_app,
    );
}

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
pub fn ios_entry_point() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    if let Err(error) = IosAppLauncher::new()
        .with_title("Cranpose Demo")
        .with_fonts(IOS_DEMO_FONTS)
        .try_run(ios_root)
    {
        eprintln!("Failed to launch Cranpose Demo: {error}");
        std::process::exit(1);
    }
}
