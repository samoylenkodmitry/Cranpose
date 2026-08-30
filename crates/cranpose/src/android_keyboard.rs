use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, hash_map::Entry},
    rc::Rc,
    sync::Arc,
};

use android_activity::{
    AndroidApp,
    input::{KeyAction, KeyCharacterMap, KeyMapChar, Keycode, MetaState},
};
use cranpose_app_shell::{
    ImeEditorState, KeyCode, KeyEvent, KeyEventType, Modifiers, PlatformTextInputHandler,
};

use crate::android_text_input::{
    AndroidImeEventQueue, hide_android_text_input, show_android_text_input,
    update_android_text_input_state,
};

const ANDROID_KEY_ACTION_DOWN: i32 = 0;
const ANDROID_KEY_ACTION_UP: i32 = 1;

pub(crate) struct AndroidImeSession {
    app: AndroidApp,
    queue: Arc<AndroidImeEventQueue>,
    active: Cell<bool>,
    bridge_failed: Cell<bool>,
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

    pub(crate) fn is_active(&self) -> bool {
        self.active.get() && !self.bridge_failed.get()
    }

    pub(crate) fn sync_editor_state(&self, state: Option<ImeEditorState>) {
        if !self.is_active() {
            return;
        }
        let Some(state) = state else {
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

        let state = cranpose_ui::text_field_focus::focused_editor_state().unwrap_or_else(|| {
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
        self.app.show_soft_input(true);
    }

    pub(crate) fn ensure_hidden(&self) {
        if !self.bridge_failed.get() && self.active.get() {
            self.hide();
        } else {
            self.app.hide_soft_input(false);
        }
    }
}

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

pub(crate) struct AndroidKeyTranslator {
    app: AndroidApp,
    key_maps: HashMap<i32, Option<KeyCharacterMap>>,
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
                    String::new()
                } else {
                    ch.to_string()
                }
            }
            Ok(KeyMapChar::CombiningAccent(accent)) => {
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
