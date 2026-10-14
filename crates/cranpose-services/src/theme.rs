use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};
use cranpose_macros::composable;

/// The light or dark appearance the operating system is set to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemTheme {
    /// The system uses light colors.
    Light,
    /// The system uses dark colors.
    Dark,
}

/// Installs the platform-reported system theme. Platform backends call this at
/// startup and whenever the OS reports a change (then force a root render so
/// composition observes it).
pub fn set_platform_system_theme(theme: SystemTheme) {
    crate::thread_state::with(|state| state.platform_system_theme.set(Some(theme)));
}

/// Removes any platform-reported theme (tests and teardown).
pub fn clear_platform_system_theme() {
    crate::thread_state::with(|state| state.platform_system_theme.set(None));
}

/// The system theme the platform reported or a detection found, without
/// detecting it: detecting runs a system command on macOS (`defaults`).
pub fn known_system_theme() -> Option<SystemTheme> {
    crate::thread_state::with(|state| {
        state
            .platform_system_theme
            .get()
            .or_else(|| state.detected_system_theme.get())
    })
}

/// Returns the system theme from the platform report or an earlier detection. When there is none,
/// it detects the theme once and keeps the result. Detection yields `Light` where the platform
/// cannot tell.
pub fn default_system_theme() -> SystemTheme {
    known_system_theme().unwrap_or_else(|| {
        let theme = detect_system_theme_uncached();
        crate::thread_state::with(|state| state.detected_system_theme.set(Some(theme)));
        theme
    })
}

fn detect_system_theme_uncached() -> SystemTheme {
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios"),
        feature = "system-theme"
    ))]
    {
        detect_native_system_theme().unwrap_or(SystemTheme::Light)
    }

    #[cfg(all(target_arch = "wasm32", feature = "system-theme-web"))]
    {
        web_sys::window()
            .and_then(|window| {
                window
                    .match_media("(prefers-color-scheme: dark)")
                    .ok()
                    .flatten()
            })
            .map_or(SystemTheme::Light, |query| {
                if query.matches() {
                    SystemTheme::Dark
                } else {
                    SystemTheme::Light
                }
            })
    }

    #[cfg(any(
        target_os = "android",
        target_os = "ios",
        all(
            not(target_arch = "wasm32"),
            not(target_os = "android"),
            not(target_os = "ios"),
            not(feature = "system-theme")
        ),
        all(target_arch = "wasm32", not(feature = "system-theme-web"))
    ))]
    {
        SystemTheme::Light
    }
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios"),
    feature = "system-theme"
))]
fn detect_native_system_theme() -> Option<SystemTheme> {
    detect_env_theme().or_else(detect_platform_theme)
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios"),
    feature = "system-theme"
))]
fn detect_env_theme() -> Option<SystemTheme> {
    ["GTK_THEME", "QT_STYLE_OVERRIDE", "XDG_CURRENT_DESKTOP"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|value| theme_from_text(&value))
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "system-theme"
))]
fn detect_platform_theme() -> Option<SystemTheme> {
    command_stdout(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "color-scheme"],
    )
    .and_then(|value| theme_from_text(&value))
    .or_else(|| {
        command_stdout(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "gtk-theme"],
        )
        .and_then(|value| theme_from_text(&value))
    })
    .or_else(|| {
        command_stdout(
            "kreadconfig6",
            &["--group", "General", "--key", "ColorScheme"],
        )
        .and_then(|value| theme_from_text(&value))
    })
    .or_else(|| {
        command_stdout(
            "kreadconfig5",
            &["--group", "General", "--key", "ColorScheme"],
        )
        .and_then(|value| theme_from_text(&value))
    })
}

#[cfg(all(target_os = "macos", feature = "system-theme"))]
fn detect_platform_theme() -> Option<SystemTheme> {
    command_stdout("defaults", &["read", "-g", "AppleInterfaceStyle"])
        .and_then(|value| theme_from_text(&value))
}

#[cfg(all(target_os = "windows", feature = "system-theme"))]
fn detect_platform_theme() -> Option<SystemTheme> {
    command_stdout(
        "reg",
        &[
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ],
    )
    .and_then(|value| theme_from_windows_registry(&value))
}

#[cfg(all(
    not(target_os = "linux"),
    not(target_os = "macos"),
    not(target_os = "windows"),
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios"),
    feature = "system-theme"
))]
fn detect_platform_theme() -> Option<SystemTheme> {
    None
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios"),
    feature = "system-theme"
))]
fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = crate::windowless_command(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios"),
    feature = "system-theme"
))]
fn theme_from_text(value: &str) -> Option<SystemTheme> {
    let value = value.to_ascii_lowercase();
    if value.contains("dark") {
        Some(SystemTheme::Dark)
    } else if value.contains("light") || value.contains("default") {
        Some(SystemTheme::Light)
    } else {
        None
    }
}

#[cfg(all(target_os = "windows", feature = "system-theme"))]
fn theme_from_windows_registry(value: &str) -> Option<SystemTheme> {
    value.lines().find_map(|line| {
        if !line.contains("AppsUseLightTheme") {
            return None;
        }
        if line.contains("0x0") {
            Some(SystemTheme::Dark)
        } else if line.contains("0x1") {
            Some(SystemTheme::Light)
        } else {
            None
        }
    })
}

/// The system theme a composable reads. It is the platform theme unless a [`ProvideSystemTheme`]
/// above it supplies another one.
pub fn local_system_theme() -> CompositionLocal<SystemTheme> {
    crate::composition_locals::cached_local(
        |locals| &locals.theme,
        || compositionLocalOf(default_system_theme),
    )
}

/// Gives the content below it a fixed system theme, for a preview or a test.
#[composable]
pub fn ProvideSystemTheme(theme: SystemTheme, content: impl FnOnce()) {
    let local = local_system_theme();
    CompositionLocalProvider([local.provides(theme)], move || {
        content();
    });
}

/// Returns whether the system theme in scope is dark. Compose's `isSystemInDarkTheme`.
#[composable]
pub fn isSystemInDarkTheme() -> bool {
    matches!(local_system_theme().current(), SystemTheme::Dark)
}

#[cfg(test)]
#[path = "tests/theme_tests.rs"]
mod tests;
