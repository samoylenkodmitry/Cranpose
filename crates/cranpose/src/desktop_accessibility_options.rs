//! The display options a desktop reports: what macOS keeps under
//! Accessibility, Display; what GNOME keeps in its interface settings; what
//! Windows keeps in the registry. Read once on a thread at start, so the
//! window does not wait on a process, and applied on the next frame. An
//! environment variable sets any of them for a test or a desktop with none
//! of the three.

use std::sync::{Arc, Mutex};

use cranpose_app_shell::AppShell;
use cranpose_render_common::Renderer;
use cranpose_services::AccessibilityOptions;

/// The environment variables a person or a test sets: `1`, `true` or `on`
/// turns a switch on, and `CRANPOSE_FONT_SCALE` takes a number.
const FONT_SCALE_VAR: &str = "CRANPOSE_FONT_SCALE";
type SwitchField = fn(&mut AccessibilityOptions) -> &mut bool;
const SWITCH_VARS: [(&str, SwitchField); 4] = [
    ("CRANPOSE_REDUCE_MOTION", |options| {
        &mut options.reduce_motion
    }),
    ("CRANPOSE_REDUCE_TRANSPARENCY", |options| {
        &mut options.reduce_transparency
    }),
    ("CRANPOSE_INCREASE_CONTRAST", |options| {
        &mut options.increase_contrast
    }),
    ("CRANPOSE_BOLD_TEXT", |options| &mut options.bold_text),
];

/// Reads the system's options on a thread and hands them to the shell once,
/// calling `ready` so the event loop wakes to apply them.
/// A run a robot drives reads only the environment, so a screenshot test
/// does not follow the host's text scale or animation switch.
pub(crate) struct OptionsProbe {
    slot: Arc<Mutex<Option<AccessibilityOptions>>>,
    applied: bool,
}

impl OptionsProbe {
    pub(crate) fn start(read_system: bool, ready: impl FnOnce() + Send + 'static) -> Self {
        let slot = Arc::new(Mutex::new(None));
        let filled = Arc::clone(&slot);
        std::thread::Builder::new()
            .name("cranpose-accessibility-options".to_string())
            .spawn(move || {
                let options = with_environment(if read_system {
                    system_options()
                } else {
                    AccessibilityOptions::default()
                });
                if let Ok(mut slot) = filled.lock() {
                    *slot = Some(options);
                }
                ready();
            })
            .ok();
        Self {
            slot,
            applied: false,
        }
    }

    /// Installs the options once they are in, with the font scale and a root
    /// render when they differ from the defaults. Returns whether they did.
    pub(crate) fn apply<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: std::fmt::Debug,
    {
        if self.applied {
            return false;
        }
        let Some(options) = self.slot.lock().ok().and_then(|mut slot| slot.take()) else {
            return false;
        };
        self.applied = true;
        let changed = crate::accessibility::apply_accessibility_options(shell, options);
        if changed {
            shell.set_font_scale(options.font_scale);
        }
        changed
    }
}

fn with_environment(mut options: AccessibilityOptions) -> AccessibilityOptions {
    if let Some(scale) = std::env::var(FONT_SCALE_VAR)
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
    {
        options.font_scale = scale;
    }
    for (name, field) in SWITCH_VARS {
        if let Ok(value) = std::env::var(name) {
            *field(&mut options) = flag_is_on(&value);
        }
    }
    options
}

/// Whether a setting's text says on: `1`, `true`, `on` or `yes`, in any case,
/// with the quotes a shell prints around it.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn flag_is_on(value: &str) -> bool {
    matches!(
        value
            .trim()
            .trim_matches('\'')
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// A number a settings tool printed, or nothing when it printed no number.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn number_in(value: &str) -> Option<f32> {
    value
        .split_whitespace()
        .last()
        .and_then(|word| word.trim_matches('\'').parse::<f32>().ok())
        .filter(|number| number.is_finite() && *number > 0.0)
}

/// The text a settings tool prints, or nothing when the tool is absent or
/// the key unset.
fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let output = cranpose_services::windowless_command(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn system_options() -> AccessibilityOptions {
    let switch = |key: &str| {
        output_of("defaults", &["read", "com.apple.universalaccess", key])
            .is_some_and(|value| flag_is_on(&value))
    };
    AccessibilityOptions {
        reduce_motion: switch("reduceMotion"),
        reduce_transparency: switch("reduceTransparency"),
        increase_contrast: switch("increaseContrast"),
        ..AccessibilityOptions::default()
    }
}

#[cfg(target_os = "linux")]
fn system_options() -> AccessibilityOptions {
    let setting = |key: &str| output_of("gsettings", &["get", "org.gnome.desktop.interface", key]);
    AccessibilityOptions {
        font_scale: setting("text-scaling-factor")
            .and_then(|value| number_in(&value))
            .unwrap_or(1.0),
        reduce_motion: setting("enable-animations").is_some_and(|value| !flag_is_on(&value)),
        increase_contrast: setting("gtk-theme")
            .is_some_and(|theme| theme.to_ascii_lowercase().contains("highcontrast")),
        ..AccessibilityOptions::default()
    }
}

#[cfg(target_os = "windows")]
fn system_options() -> AccessibilityOptions {
    let value = |key: &str, name: &str| {
        output_of("reg", &["query", key, "/v", name]).and_then(|text| registry_value(&text))
    };
    AccessibilityOptions {
        font_scale: value(r"HKCU\Software\Microsoft\Accessibility", "TextScaleFactor")
            .map_or(1.0, |percent| percent / 100.0),
        reduce_motion: value(r"HKCU\Control Panel\Desktop\WindowMetrics", "MinAnimate")
            .is_some_and(|minimize| minimize == 0.0),
        reduce_transparency: value(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "EnableTransparency",
        )
        .is_some_and(|transparency| transparency == 0.0),
        increase_contrast: value(r"HKCU\Control Panel\Accessibility\HighContrast", "Flags")
            .is_some_and(|flags| (flags as u32) & 1 != 0),
        ..AccessibilityOptions::default()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn system_options() -> AccessibilityOptions {
    AccessibilityOptions::default()
}

/// The number in a `reg query` line such as
/// `    MinAnimate    REG_SZ    0` or `    Flags    REG_DWORD    0x7e`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn registry_value(text: &str) -> Option<f32> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace().rev();
            let value = words.next()?;
            let kind = words.next()?;
            kind.starts_with("REG_").then_some(value)
        })
        .find_map(|value| match value.strip_prefix("0x") {
            Some(hex) => u32::from_str_radix(hex, 16)
                .ok()
                .map(|number| number as f32),
            None => value.parse::<f32>().ok(),
        })
}

#[cfg(test)]
#[path = "tests/desktop_accessibility_options_tests.rs"]
mod tests;
