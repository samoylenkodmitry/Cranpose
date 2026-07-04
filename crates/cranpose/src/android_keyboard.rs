//! Android soft-keyboard visibility and key-event translation.
//!
//! Two halves of Android text input live here:
//!
//! 1. [`AndroidSoftKeyboard`] implements the framework's
//!    [`PlatformTextInputHandler`] hook: when a `BasicTextField` gains focus
//!    the framework calls `show_keyboard` and we ask the platform to show the
//!    soft keyboard via [`AndroidApp::show_soft_input`]; when text-field focus
//!    is cleared (or goes stale) we hide it again.
//!
//! 2. [`AndroidKeyTranslator`] converts [`android_activity`] `KeyEvent`s into
//!    the framework [`KeyEvent`] consumed by the focused text field. Printable
//!    characters are resolved through the device [`KeyCharacterMap`]
//!    (including dead-key/combining-accent composition), while editing keys
//!    (backspace, enter, arrows, ...) are mapped to framework [`KeyCode`]s.
//!
//! # Limitation: KeyCharacterMap path, not a full IME `InputConnection`
//!
//! A `NativeActivity` has no Java `View` overriding `onCreateInputConnection`,
//! so IMEs run in their fallback mode and deliver input as plain key events.
//! Simple keyboards (and Gboard's basic Latin layouts) work fine through the
//! `KeyCharacterMap` path implemented here, but anything that needs a real
//! `InputConnection` - autocorrect/suggestions, swipe typing, voice input,
//! and composing-text scripts such as CJK or Indic - cannot be delivered as
//! key events by the IME. Supporting those requires a Java editor overlay
//! providing an `InputConnection` (the framework side is already prepared:
//! see `AppShell::on_ime_preedit` / `on_paste` for committed text).

use android_activity::input::{KeyAction, KeyCharacterMap, KeyMapChar, Keycode, MetaState};
use android_activity::AndroidApp;
use cranpose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers, PlatformTextInputHandler};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

/// Shows/hides the Android soft keyboard in response to text-field focus
/// changes. Installed on the `AppShell` via `set_platform_text_input`.
pub(crate) struct AndroidSoftKeyboard {
    app: AndroidApp,
}

impl AndroidSoftKeyboard {
    pub(crate) fn new(app: AndroidApp) -> Self {
        Self { app }
    }
}

impl PlatformTextInputHandler for AndroidSoftKeyboard {
    fn show_keyboard(&self) {
        // `true` requests SHOW_IMPLICIT: the system may coordinate visibility
        // with hardware keyboards / display modes instead of forcing the IME.
        self.app.show_soft_input(true);
    }

    fn hide_keyboard(&self) {
        // `false` = do not restrict to implicitly-shown keyboards; always hide.
        self.app.hide_soft_input(false);
    }
}

/// Translates Android key events into framework [`KeyEvent`]s.
///
/// Keeps per-device [`KeyCharacterMap`]s cached (the lookup crosses JNI) and
/// carries dead-key state so accent + base-letter sequences compose (for
/// example `¨` then `o` produces `ö`).
pub(crate) struct AndroidKeyTranslator {
    app: AndroidApp,
    /// Character maps by input-device id. `None` caches a failed lookup so a
    /// misbehaving device does not retry JNI on every keystroke.
    key_maps: HashMap<i32, Option<KeyCharacterMap>>,
    /// Pending dead-key accent awaiting its base character.
    combining_accent: Option<char>,
}

impl AndroidKeyTranslator {
    pub(crate) fn new(app: AndroidApp) -> Self {
        Self {
            app,
            key_maps: HashMap::new(),
            combining_accent: None,
        }
    }

    /// Converts an Android key event into a framework key event.
    ///
    /// Returns `None` for events the text pipeline cannot use (unknown keys
    /// producing no character, and the legacy `Multiple` action); the caller
    /// should report those as unhandled so the system can process them.
    pub(crate) fn translate(
        &mut self,
        event: &android_activity::input::KeyEvent<'_>,
    ) -> Option<KeyEvent> {
        let event_type = match event.action() {
            KeyAction::Down => KeyEventType::KeyDown,
            KeyAction::Up => KeyEventType::KeyUp,
            _ => return None,
        };

        let key_code = map_keycode(event.key_code());
        let modifiers = map_modifiers(event.meta_state());

        // Character lookup only for key-down: the text field commits
        // characters on KeyDown, and dead-key state must not advance twice.
        let text = if event_type == KeyEventType::KeyDown {
            self.key_character(event)
        } else {
            String::new()
        };

        if key_code == KeyCode::Unknown && text.is_empty() {
            return None;
        }

        Some(KeyEvent::new(key_code, text, modifiers, event_type))
    }

    /// Resolves the unicode character for a key-down via the device
    /// [`KeyCharacterMap`], composing dead keys.
    fn key_character(&mut self, event: &android_activity::input::KeyEvent<'_>) -> String {
        let device_id = event.device_id();
        let key_map = match self.key_maps.entry(device_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let map = self
                    .app
                    .device_key_character_map(device_id)
                    .map_err(|error| {
                        log::warn!("No key character map for input device {device_id}: {error}");
                    })
                    .ok();
                entry.insert(map)
            }
        };
        let Some(key_map) = key_map.as_ref() else {
            return String::new();
        };

        match key_map.get(event.key_code(), event.meta_state()) {
            Ok(KeyMapChar::Unicode(ch)) => {
                let ch = match self.combining_accent.take() {
                    Some(accent) => match key_map.get_dead_char(accent, ch) {
                        Ok(Some(combined)) => combined,
                        Ok(None) => ch,
                        Err(error) => {
                            log::warn!("KeyCharacterMap::get_dead_char failed: {error}");
                            ch
                        }
                    },
                    None => ch,
                };
                if ch.is_control() {
                    // Control characters (\n, \t, backspace, ...) are handled
                    // through their KeyCode, mirroring the desktop path where
                    // only printable keys carry text.
                    String::new()
                } else {
                    ch.to_string()
                }
            }
            Ok(KeyMapChar::CombiningAccent(accent)) => {
                // Dead key: remember the accent, emit no character yet.
                self.combining_accent = Some(accent);
                String::new()
            }
            Ok(KeyMapChar::None) => String::new(),
            Err(error) => {
                log::warn!("KeyCharacterMap::get failed: {error}");
                String::new()
            }
        }
    }
}

/// Keys the app must leave to the system (navigation, volume, media, ...).
///
/// Consuming these would break the back gesture, volume rockers, and media
/// controls, so the input loop reports them as unhandled without translation.
pub(crate) fn is_system_key(keycode: Keycode) -> bool {
    matches!(
        keycode,
        Keycode::Back
            | Keycode::Home
            | Keycode::Menu
            | Keycode::AppSwitch
            | Keycode::Power
            | Keycode::Camera
            | Keycode::Call
            | Keycode::Endcall
            | Keycode::VolumeUp
            | Keycode::VolumeDown
            | Keycode::VolumeMute
            | Keycode::Mute
            | Keycode::MediaPlayPause
            | Keycode::MediaStop
            | Keycode::MediaNext
            | Keycode::MediaPrevious
            | Keycode::MediaRewind
            | Keycode::MediaFastForward
            | Keycode::MediaPlay
            | Keycode::MediaPause
            | Keycode::MediaRecord
    )
}

fn map_modifiers(meta_state: MetaState) -> Modifiers {
    Modifiers {
        shift: meta_state.shift_on(),
        ctrl: meta_state.ctrl_on(),
        alt: meta_state.alt_on(),
        meta: meta_state.meta_on(),
    }
}

/// Maps Android keycodes onto framework physical key codes.
///
/// Only keys the text pipeline reacts to need mapping; everything else falls
/// through to [`KeyCode::Unknown`] and relies on the character-map text (the
/// focused field inserts `KeyEvent::text` for unknown key codes).
fn map_keycode(keycode: Keycode) -> KeyCode {
    match keycode {
        Keycode::A => KeyCode::A,
        Keycode::B => KeyCode::B,
        Keycode::C => KeyCode::C,
        Keycode::D => KeyCode::D,
        Keycode::E => KeyCode::E,
        Keycode::F => KeyCode::F,
        Keycode::G => KeyCode::G,
        Keycode::H => KeyCode::H,
        Keycode::I => KeyCode::I,
        Keycode::J => KeyCode::J,
        Keycode::K => KeyCode::K,
        Keycode::L => KeyCode::L,
        Keycode::M => KeyCode::M,
        Keycode::N => KeyCode::N,
        Keycode::O => KeyCode::O,
        Keycode::P => KeyCode::P,
        Keycode::Q => KeyCode::Q,
        Keycode::R => KeyCode::R,
        Keycode::S => KeyCode::S,
        Keycode::T => KeyCode::T,
        Keycode::U => KeyCode::U,
        Keycode::V => KeyCode::V,
        Keycode::W => KeyCode::W,
        Keycode::X => KeyCode::X,
        Keycode::Y => KeyCode::Y,
        Keycode::Z => KeyCode::Z,
        Keycode::Keycode0 => KeyCode::Digit0,
        Keycode::Keycode1 => KeyCode::Digit1,
        Keycode::Keycode2 => KeyCode::Digit2,
        Keycode::Keycode3 => KeyCode::Digit3,
        Keycode::Keycode4 => KeyCode::Digit4,
        Keycode::Keycode5 => KeyCode::Digit5,
        Keycode::Keycode6 => KeyCode::Digit6,
        Keycode::Keycode7 => KeyCode::Digit7,
        Keycode::Keycode8 => KeyCode::Digit8,
        Keycode::Keycode9 => KeyCode::Digit9,
        Keycode::Del => KeyCode::Backspace,
        Keycode::ForwardDel => KeyCode::Delete,
        Keycode::Enter | Keycode::NumpadEnter => KeyCode::Enter,
        Keycode::Tab => KeyCode::Tab,
        Keycode::Space => KeyCode::Space,
        Keycode::Escape => KeyCode::Escape,
        Keycode::DpadUp => KeyCode::ArrowUp,
        Keycode::DpadDown => KeyCode::ArrowDown,
        Keycode::DpadLeft => KeyCode::ArrowLeft,
        Keycode::DpadRight => KeyCode::ArrowRight,
        Keycode::MoveHome => KeyCode::Home,
        Keycode::MoveEnd => KeyCode::End,
        _ => KeyCode::Unknown,
    }
}
