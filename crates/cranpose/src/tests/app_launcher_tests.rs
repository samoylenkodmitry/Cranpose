use super::*;

#[test]
fn android_backend_selection_preserves_the_app_choice_without_a_valid_override() {
    assert_eq!(
        AppLauncher::new().settings.android_gpu_backend,
        AndroidGpuBackend::Vulkan
    );
    for backend in [AndroidGpuBackend::Vulkan, AndroidGpuBackend::OpenGlEs] {
        let selected = AppLauncher::new()
            .with_android_gpu_backend(backend)
            .settings
            .android_gpu_backend;
        for value in [None, Some(""), Some("vulkan2")] {
            assert_eq!(selected.with_override(value), backend);
        }
        for value in ["gl", "GL", " gles ", "OpenGL"] {
            assert_eq!(
                selected.with_override(Some(value)),
                AndroidGpuBackend::OpenGlEs
            );
        }
        assert_eq!(
            selected.with_override(Some(" VULKAN ")),
            AndroidGpuBackend::Vulkan
        );
    }
}

#[test]
fn a_launcher_records_the_application_id_it_is_given() {
    let launcher = AppLauncher::new();
    assert!(
        launcher.settings.application_id.is_none(),
        "an id appeared without being stated"
    );

    let launcher = AppLauncher::new().with_application_id("com.example.notes");
    assert_eq!(
        launcher.settings.application_id.as_deref(),
        Some("com.example.notes")
    );
}

#[test]
fn a_launcher_records_the_android_system_font_choice() {
    assert!(
        !AppLauncher::new().settings.android_use_system_fonts,
        "system fonts must be opt-in: loading them costs a scan of /system/fonts"
    );
    assert!(
        AppLauncher::new()
            .with_android_use_system_fonts(true)
            .settings
            .android_use_system_fonts
    );
}

#[test]
fn a_launcher_records_the_overlay_window_it_is_given() {
    assert!(AppLauncher::new().settings.android_overlay_window.is_none());

    let options = AndroidOverlayWindowOptions::new(320, 180);
    let launcher = AppLauncher::new().with_android_overlay_window(options);
    let stored = launcher
        .settings
        .android_overlay_window
        .expect("the overlay options were dropped");
    assert_eq!((stored.width, stored.height), (320, 180));
}

#[test]
fn a_font_registration_closure_is_run_against_the_launchers_own_registry() {
    use std::{cell::Cell, rc::Rc};

    let ran = Rc::new(Cell::new(false));
    let flag = Rc::clone(&ran);
    let _launcher = AppLauncher::new().with_fonts_from(move |_registry| {
        flag.set(true);
        Ok(())
    });
    assert!(ran.get(), "the registration closure never ran");
}

#[test]
fn a_font_registration_that_fails_leaves_a_usable_launcher() {
    let launcher = AppLauncher::new()
        .with_fonts_from(|_registry| Err(FontLoadError::EmptyFamily))
        .with_title("still here");
    assert_eq!(launcher.settings.window_title, "still here");
}

#[test]
fn face_bytes_that_are_not_a_font_do_not_stop_the_launcher() {
    let launcher = AppLauncher::new()
        .with_font_face_bytes(
            &FontFamily::SansSerif,
            FontWeight::NORMAL,
            FontStyle::Normal,
            b"not a font".to_vec(),
        )
        .with_title("still here");
    assert_eq!(launcher.settings.window_title, "still here");
}

#[test]
fn android_overlay_options_default_to_touch_only_top_left_window() {
    let options = AndroidOverlayWindowOptions::new(320, 180);

    assert_eq!(options.width, 320);
    assert_eq!(options.height, 180);
    assert_eq!(options.x, 0);
    assert_eq!(options.y, 0);
    assert!(!options.focusable);
    assert!(options.is_valid());
}

#[test]
fn android_overlay_options_apply_position_and_focus() {
    let options = AndroidOverlayWindowOptions::new(320, 180)
        .with_position(12, 34)
        .with_focusable(true);

    assert_eq!(options.x, 12);
    assert_eq!(options.y, 34);
    assert!(options.focusable);
}

#[test]
fn android_overlay_options_reject_zero_size() {
    assert!(!AndroidOverlayWindowOptions::new(0, 180).is_valid());
    assert!(!AndroidOverlayWindowOptions::new(320, 0).is_valid());
}

#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
#[test]
fn production_apps_default_to_vsync_frame_pacing() {
    assert_eq!(
        AppSettings::default().frame_pacing_mode,
        FramePacingMode::Vsync
    );
    assert_eq!(FramePacingMode::default(), FramePacingMode::Vsync);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    feature = "robot"
))]
#[test]
fn robot_test_driver_defaults_to_uncapped_frame_pacing() {
    let launcher = AppLauncher::new().with_test_driver(|_| {});
    assert_eq!(
        launcher.settings.frame_pacing_mode,
        FramePacingMode::NoVsync
    );

    let pinned = AppLauncher::new()
        .with_frame_pacing_mode(FramePacingMode::Hard60)
        .with_test_driver(|_| {});
    assert_eq!(pinned.settings.frame_pacing_mode, FramePacingMode::Hard60);

    let pinned_after = AppLauncher::new()
        .with_test_driver(|_| {})
        .with_frame_pacing_mode(FramePacingMode::Vsync);
    assert_eq!(
        pinned_after.settings.frame_pacing_mode,
        FramePacingMode::Vsync
    );
}
