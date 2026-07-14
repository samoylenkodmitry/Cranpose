#![deny(unsafe_code)]

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
use crate::fonts::DEMO_FONTS;
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use cranpose::AppLauncher;

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn create_app() -> AppLauncher {
    AppLauncher::new()
        .with_title("Cranpose Demo")
        .with_size(800, 600)
        .with_fonts(DEMO_FONTS)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
}

/// Shared entry point for desktop
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub fn entry_point() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    if let Err(error) = create_app().try_run(app::DesktopApp) {
        eprintln!("Failed to launch Cranpose Demo: {error}");
        std::process::exit(1);
    }
}

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
use crate::fonts::DEMO_FONTS as IOS_DEMO_FONTS;
#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
use cranpose::{
    local_safe_area_insets, AppLauncher as IosAppLauncher, Box as CranposeBox, BoxSpec, Modifier,
};

/// iOS root: insets the demo by the system safe area (status bar, notch, home
/// indicator) read from [`local_safe_area_insets`], which the iOS backend
/// publishes from the live window.
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

/// Shared entry point for iOS.
///
/// winit starts `UIApplicationMain`, so this is invoked directly from the app
/// binary's `main`. The window is sized by UIKit, so no explicit size is set.
#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
pub fn ios_entry_point() {
    if let Err(error) = IosAppLauncher::new()
        .with_title("Cranpose Demo")
        .with_fonts(IOS_DEMO_FONTS)
        .try_run(ios_root)
    {
        eprintln!("Failed to launch Cranpose Demo: {error}");
        std::process::exit(1);
    }
}
