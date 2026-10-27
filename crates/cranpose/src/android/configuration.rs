use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;

use super::event_loop::android_platform_env;

// Takes the system theme and the kind of device before the first
// composition reads them.
pub(super) fn read_android_configuration(app: &android_activity::AndroidApp) {
    let env = android_platform_env();
    env.set_system_theme(system_theme_from_android(app.config().ui_mode_night()));
    env.set_configuration(configuration_from_android(app));
}

fn system_theme_from_android(
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
    if android_platform_env().set_configuration(configuration_from_android(app))
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

// The kind of device Android's configuration names, as `LocalConfiguration`'s
// `uiMode` type tells a Compose app.
fn configuration_from_android(app: &android_activity::AndroidApp) -> cranpose_ui::Configuration {
    use cranpose_ui::UiModeType;
    use ndk::configuration::UiModeType as Android;

    let ui_mode_type = match app.config().ui_mode_type() {
        Android::Desk => UiModeType::Desk,
        Android::Car => UiModeType::Car,
        Android::Television => UiModeType::Television,
        Android::Applicance => UiModeType::Appliance,
        Android::Watch => UiModeType::Watch,
        Android::VrHeadset => UiModeType::VrHeadset,
        _ => UiModeType::Normal,
    };
    cranpose_ui::Configuration { ui_mode_type }
}
