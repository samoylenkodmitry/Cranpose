use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use jni::{jni_sig, jni_str};

use super::{
    event_loop::android_platform_env,
    jni::{activity_configuration, jni_failure},
};

// `Configuration.uiMode` keeps the night setting in these bits and the kind
// of device in the low four.
const UI_MODE_NIGHT_MASK: i32 = 0x30;
const UI_MODE_NIGHT_YES: i32 = 0x20;
const UI_MODE_TYPE_MASK: i32 = 0x0f;

// Takes the system theme and the kind of device before the first
// composition reads them.
pub(super) fn read_android_configuration(app: &android_activity::AndroidApp) {
    let ui_mode = ui_mode(app);
    let env = android_platform_env();
    env.set_system_theme(system_theme_from(ui_mode));
    env.set_configuration(configuration_from(ui_mode));
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
    let ui_mode = ui_mode(app);
    if android_platform_env().set_system_theme(system_theme_from(ui_mode))
        && let Some(shell) = shell
    {
        shell.request_root_render();
    }
    if android_platform_env().set_configuration(configuration_from(ui_mode))
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

fn ui_mode(app: &android_activity::AndroidApp) -> i32 {
    super::jni::with_android_activity_env(app, |env, activity| {
        let (_, configuration) = activity_configuration(env, &activity)?;
        env.get_field(&configuration, jni_str!("uiMode"), jni_sig!("I"))
            .and_then(jni::JValueOwned::i)
            .map_err(|error| jni_failure(env, "Configuration.uiMode", error))
    })
    .unwrap_or_else(|error| {
        log::warn!("[android-configuration] could not read Configuration.uiMode: {error}");
        let config = app.config();
        i32::from(config.ui_mode_type()) | (i32::from(config.ui_mode_night()) << 4)
    })
}

fn system_theme_from(ui_mode: i32) -> cranpose_services::SystemTheme {
    if ui_mode & UI_MODE_NIGHT_MASK == UI_MODE_NIGHT_YES {
        cranpose_services::SystemTheme::Dark
    } else {
        cranpose_services::SystemTheme::Light
    }
}

// The kind of device Android's configuration names, as `LocalConfiguration`'s
// `uiMode` type tells a Compose app.
fn configuration_from(ui_mode: i32) -> cranpose_ui::Configuration {
    use cranpose_ui::UiModeType;
    use ndk::configuration::UiModeType as Android;

    let ui_mode_type = match Android::from(ui_mode & UI_MODE_TYPE_MASK) {
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
