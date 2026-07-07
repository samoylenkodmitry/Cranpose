//! Android soft-keyboard visibility and key-event translation.
//!
//! Two halves of Android text input live here:
//!
//! 1. [`AndroidSoftKeyboard`] implements the framework's
//!    [`PlatformTextInputHandler`] hook: when a `BasicTextField` gains focus
//!    the framework calls `show_keyboard` and we open an IME editor session
//!    through the `CranposeTextInput` Java helper (a real `InputConnection`,
//!    see [`crate::android_text_input`]); when text-field focus is cleared
//!    (or goes stale) the session is closed again. Apps that do not compile
//!    the `cranpose/android/java` sources fall back to the plain
//!    [`AndroidApp::show_soft_input`] path.
//!
//! 2. [`AndroidKeyTranslator`] converts [`android_activity`] `KeyEvent`s into
//!    the framework [`KeyEvent`] consumed by the focused text field. Printable
//!    characters are resolved through the device [`KeyCharacterMap`]
//!    (including dead-key/combining-accent composition), while editing keys
//!    (backspace, enter, arrows, ...) are mapped to framework [`KeyCode`]s.
//!    This remains the path for hardware keyboards, and the whole-app
//!    fallback when the Java IME helper is unavailable.

use crate::android_text_input::{
    hide_android_text_input, show_android_text_input, update_android_text_input_state,
    AndroidImeEventQueue,
};
use android_activity::input::{KeyAction, KeyCharacterMap, KeyMapChar, Keycode, MetaState};
use android_activity::AndroidApp;
use cranpose_app_shell::{
    ImeEditorState, KeyCode, KeyEvent, KeyEventType, Modifiers, PlatformTextInputHandler,
};
use std::cell::{Cell, RefCell};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Android `KeyEvent` action constants (`ACTION_DOWN` / `ACTION_UP`).
const ANDROID_KEY_ACTION_DOWN: i32 = 0;
const ANDROID_KEY_ACTION_UP: i32 = 1;

/// State of the IME editor session shared between the focus-driven
/// [`AndroidSoftKeyboard`] handler and the main loop (which drains IME events
/// and pushes editor-state updates back to the Java mirror).
///
/// Main-thread only (`Rc`); the cross-thread part is the event queue.
pub(crate) struct AndroidImeSession {
    app: AndroidApp,
    queue: Arc<AndroidImeEventQueue>,
    /// A Java editor session is currently open.
    active: Cell<bool>,
    /// The Java helper class could not be used; stick to the plain
    /// `show_soft_input` path for the rest of the process lifetime.
    bridge_failed: Cell<bool>,
    /// Editor state last pushed to (or seeded into) the Java mirror. Used to
    /// skip redundant JNI round-trips on every frame.
    last_synced: RefCell<Option<ImeEditorState>>,
}

impl AndroidImeSession {
    pub(crate) fn new(app: AndroidApp, queue: Arc<AndroidImeEventQueue>) -> Rc<Self> {
        Rc::new(Self {
            app,
            queue,
            active: Cell::new(false),
            bridge_failed: Cell::new(false),
            last_synced: RefCell::new(None),
        })
    }

    /// Whether the Java editor session is open (and therefore editor-state
    /// syncing is worthwhile).
    pub(crate) fn is_active(&self) -> bool {
        self.active.get() && !self.bridge_failed.get()
    }

    /// Pushes the current editor state to the Java mirror when it changed
    /// since the last push. Called from the main loop after input handling.
    pub(crate) fn sync_editor_state(&self, state: Option<ImeEditorState>) {
        if !self.is_active() {
            return;
        }
        let Some(state) = state else {
            // No focused field: the session is about to be closed by the
            // focus-loss notification; nothing to sync.
            return;
        };
        if self.last_synced.borrow().as_ref() == Some(&state) {
            return;
        }
        match update_android_text_input_state(&self.app, &state) {
            Ok(()) => {
                *self.last_synced.borrow_mut() = Some(state);
            }
            Err(error) => {
                log::warn!("Android IME state sync failed: {error}");
            }
        }
    }

    fn show(&self) {
        if self.bridge_failed.get() {
            self.show_soft_input_only();
            return;
        }

        // Seed the Java mirror with the focused field's state. This runs
        // inside the app context (the focus manager invokes the platform
        // handler synchronously), so the focused handler is accessible.
        let state = cranpose_ui::text_field_focus::focused_editor_state().unwrap_or_else(|| {
            // Focus notifications only fire for text fields; an absent state
            // still opens a session so the keyboard appears.
            ImeEditorState {
                text: String::new(),
                selection_start: 0,
                selection_end: 0,
                composition: None,
                single_line: true,
            }
        });

        match show_android_text_input(&self.app, &self.queue, &state) {
            Ok(()) => {
                self.active.set(true);
                *self.last_synced.borrow_mut() = Some(state);
            }
            Err(error) => {
                log::warn!(
                    "Android IME editor bridge unavailable, falling back to key-event input \
                     (include cranpose/android/java sources for full IME support): {error}"
                );
                self.bridge_failed.set(true);
                self.show_soft_input_only();
            }
        }
    }

    fn hide(&self) {
        if self.bridge_failed.get() {
            // `false` = do not restrict to implicitly-shown keyboards; always hide.
            self.app.hide_soft_input(false);
            return;
        }
        self.active.set(false);
        *self.last_synced.borrow_mut() = None;
        if let Err(error) = hide_android_text_input(&self.app) {
            log::warn!("Android IME editor hide failed: {error}");
            self.app.hide_soft_input(false);
        }
    }

    fn show_soft_input_only(&self) {
        // `true` requests SHOW_IMPLICIT: the system may coordinate visibility
        // with hardware keyboards / display modes instead of forcing the IME.
        self.app.show_soft_input(true);
    }

    /// Closes the IME editor session and hides the keyboard if either is still
    /// live. Called on app pause (and defensively on resume with no focused
    /// field) so the invisible editor view is detached and the OS cannot
    /// restore the soft keyboard when the activity returns to the foreground.
    /// Idempotent: a no-op once the session is already closed.
    pub(crate) fn ensure_hidden(&self) {
        if self.bridge_failed.get() {
            self.app.hide_soft_input(false);
            return;
        }
        if self.active.get() {
            self.hide();
        }
    }
}

/// Shows/hides the Android soft keyboard in response to text-field focus
/// changes. Installed on the `AppShell` via `set_platform_text_input`.
pub(crate) struct AndroidSoftKeyboard {
    session: Rc<AndroidImeSession>,
}

impl AndroidSoftKeyboard {
    pub(crate) fn new(session: Rc<AndroidImeSession>) -> Self {
        Self { session }
    }
}

impl PlatformTextInputHandler for AndroidSoftKeyboard {
    fn show_keyboard(&self) {
        self.session.show();
    }

    fn hide_keyboard(&self) {
        self.session.hide();
    }
}

/// Translates a raw key event forwarded by the IME through
/// `InputConnection.sendKeyEvent` (some keyboards deliver backspace/enter
/// this way even with a real editor attached) into a framework [`KeyEvent`].
///
/// Returns `None` for events the text pipeline cannot use; system keys are
/// never expected through this path and are dropped defensively.
pub(crate) fn ime_key_event(
    action: i32,
    key_code: i32,
    meta_state: i32,
    unicode_char: i32,
) -> Option<KeyEvent> {
    let event_type = match action {
        ANDROID_KEY_ACTION_DOWN => KeyEventType::KeyDown,
        ANDROID_KEY_ACTION_UP => KeyEventType::KeyUp,
        _ => return None,
    };

    let keycode = Keycode::from(key_code.max(0) as u32);
    if is_system_key(keycode) {
        return None;
    }
    let key_code = map_keycode(keycode);
    let modifiers = map_modifiers(MetaState(meta_state.max(0) as u32));

    // Characters commit on key-down only, mirroring the hardware-key path.
    let text = if event_type == KeyEventType::KeyDown {
        u32::try_from(unicode_char)
            .ok()
            .and_then(char::from_u32)
            .filter(|ch| *ch != '\0' && !ch.is_control())
            .map(|ch| ch.to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };

    if key_code == KeyCode::Unknown && text.is_empty() {
        return None;
    }

    Some(KeyEvent::new(key_code, text, modifiers, event_type))
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
    /// producing no character, and the `Multiple` batch action of Android 2.x
    /// IMEs); the caller should report those as unhandled so the system can
    /// process them.
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
