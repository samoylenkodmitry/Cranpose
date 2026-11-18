// Runs the player demo on Android.

cranpose::app_capabilities!();

#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
fn launcher() -> cranpose::AppLauncher {
    cranpose::AppLauncher::new().with_capabilities(&CAPABILITIES)
}

cranpose::android_main! {
    launcher: launcher(),
    content: player_demo::PlayerDemo,
}
