use std::cell::{Cell, RefCell};

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};
use cranpose_macros::composable;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemTheme {
    Light,
    Dark,
}

thread_local! {
    static PLATFORM_SYSTEM_THEME: Cell<Option<SystemTheme>> = const { Cell::new(None) };
}

/// Installs the platform-reported system theme. Platform backends call this at
/// startup and whenever the OS reports a change (then force a root render so
/// composition observes it).
pub fn set_platform_system_theme(theme: SystemTheme) {
    PLATFORM_SYSTEM_THEME.with(|cell| cell.set(Some(theme)));
}

/// Removes any platform-reported theme (tests and teardown).
pub fn clear_platform_system_theme() {
    PLATFORM_SYSTEM_THEME.with(|cell| cell.set(None));
}

pub fn default_system_theme() -> SystemTheme {
    if let Some(theme) = PLATFORM_SYSTEM_THEME.with(Cell::get) {
        return theme;
    }
    detected_system_theme()
}

fn detected_system_theme() -> SystemTheme {
    thread_local! {
        static DETECTED: Cell<Option<SystemTheme>> = const { Cell::new(None) };
    }
    DETECTED.with(|cell| {
        if let Some(theme) = cell.get() {
            return theme;
        }
        let theme = detect_system_theme_uncached();
        cell.set(Some(theme));
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

pub fn local_system_theme() -> CompositionLocal<SystemTheme> {
    thread_local! {
        static LOCAL_SYSTEM_THEME: RefCell<Option<CompositionLocal<SystemTheme>>> = const { RefCell::new(None) };
    }

    LOCAL_SYSTEM_THEME.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOf(default_system_theme))
            .clone()
    })
}

#[composable]
pub fn ProvideSystemTheme(theme: SystemTheme, content: impl FnOnce()) {
    let local = local_system_theme();
    CompositionLocalProvider(vec![local.provides(theme)], move || {
        content();
    });
}

#[composable]
pub fn isSystemInDarkTheme() -> bool {
    matches!(local_system_theme().current(), SystemTheme::Dark)
}

#[cfg(test)]
#[path = "tests/theme_tests.rs"]
mod tests;
