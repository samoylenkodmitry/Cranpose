//! Which keyboard types into a text field that takes focus in a headset.

use std::sync::OnceLock;

/// Which keyboard types into a text field that takes focus in a headset.
///
/// Jetpack XR Compose has no such choice. There the system's input method types, and Compose
/// opens it as it does on a phone. Cranpose gives a headset two keyboards. A text field opens
/// either through the same request, `show_keyboard_on_focus` of its options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keyboard {
    /// The system's input method where the runtime shows it over an immersive app, which Android
    /// XR does, and the keyboard that Cranpose draws on every other runtime.
    #[default]
    System,
    /// The keyboard that Cranpose draws, on every runtime: a `SpatialPanel` that stands below the
    /// panel of the focused text field. It has the keys of the Latin alphabet only.
    Drawn,
}

static CHOICE: OnceLock<Keyboard> = OnceLock::new();

/// Chooses the keyboard of a headset for the rest of the process. The choice counts if it comes
/// before the app starts, in the launcher's setup. A second call changes nothing. Outside a
/// headset it has no effect.
pub fn use_keyboard(keyboard: Keyboard) {
    if CHOICE.set(keyboard).is_err() {
        log::warn!("cranpose xr: the keyboard is chosen already");
    }
}

// The keyboard that the app chose, or the default. Only an Android host reads it: a desktop runtime
// shows no keyboard of the system.
#[cfg(all(headset, target_os = "android"))]
pub(crate) fn chosen() -> Keyboard {
    CHOICE.get().copied().unwrap_or_default()
}
