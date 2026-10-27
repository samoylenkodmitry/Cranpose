use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;

use super::event_loop::android_platform_env;

pub(super) fn system_theme_from_android(
    night: ndk::configuration::UiModeNight,
) -> cranpose_services::SystemTheme {
    match night {
        ndk::configuration::UiModeNight::Yes => cranpose_services::SystemTheme::Dark,
        _ => cranpose_services::SystemTheme::Light,
    }
}

pub(super) fn refresh_android_configuration(
    app: &android_activity::AndroidApp,
    shell: &mut Option<AppShell<WgpuRenderer>>,
) {
    #[cfg(feature = "localization")]
    if android_platform_env().refresh_languages()
        && let Some(shell) = shell
    {
        shell.request_root_render();
    }
    let theme = system_theme_from_android(app.config().ui_mode_night());
    if android_platform_env().set_system_theme(theme)
        && let Some(shell) = shell
    {
        shell.request_root_render();
    }
    if super::font_scale::refresh_font_scale(app)
        && let Some(shell) = shell
    {
        shell.set_font_scale_curve(super::font_scale::font_scale_curve());
    }
}
