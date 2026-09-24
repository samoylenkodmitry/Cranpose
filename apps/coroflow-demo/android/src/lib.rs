//! Runs Coroflow Notes on Android.

cranpose::app_capabilities!();

#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
fn create_app() -> cranpose::AppLauncher {
    coroflow_demo::ui::app::create_app().with_capabilities(&CAPABILITIES)
}

cranpose::android_main! {
    launcher: create_app(),
    content: coroflow_demo::ui::app::CoroflowDemoApp,
}
