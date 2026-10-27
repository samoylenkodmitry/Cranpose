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
    ImeEditorState, Key, KeyEvent, KeyEventType, Modifiers, PlatformTextInputHandler,
};

use super::text_input::{
    AndroidImeEventQueue, hide_android_text_input, place_android_text_input,
    show_android_text_input, update_android_text_input_state,
};

const ANDROID_KEY_ACTION_DOWN: i32 = 0;
const ANDROID_KEY_ACTION_UP: i32 = 1;

pub(crate) struct AndroidImeSession {
    app: AndroidApp,
    queue: Arc<AndroidImeEventQueue>,
    active: Cell<bool>,
    bridge_failed: Cell<bool>,
    last_synced: RefCell<Option<ImeEditorState>>,
    /// The field bounds last sent to the editor view; `None` after the
    /// editor shows, so it is placed again.
    last_placed: Cell<Option<[i32; 4]>>,
}

impl AndroidImeSession {
    pub(crate) fn new(app: AndroidApp, queue: Arc<AndroidImeEventQueue>) -> Rc<Self> {
        Rc::new(Self {
            app,
            queue,
            active: Cell::new(false),
            bridge_failed: Cell::new(false),
            last_synced: RefCell::new(None),
            last_placed: Cell::new(None),
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
        // The keyboard reads its options when its session starts, so a
        // change of them starts it again.
        let keyboard_changed = self.last_synced.borrow().as_ref().is_some_and(|last| {
            last.keyboard_options != state.keyboard_options || last.single_line != state.single_line
        });
        let synced = if keyboard_changed {
            self.last_placed.set(None);
            show_android_text_input(&self.app, &self.queue, &state)
        } else {
            update_android_text_input_state(&self.app, &state)
        };
        match synced {
            Ok(()) => {
                *self.last_synced.borrow_mut() = Some(state);
            }
            Err(error) => {
                log::warn!("Android IME state sync failed: {error}");
            }
        }
    }

    /// Moves the editor view over the focused field when its physical
    /// `bounds` changed.
    pub(crate) fn sync_field_bounds(&self, bounds: Option<[i32; 4]>) {
        let Some(bounds) = bounds.filter(|_| self.is_active()) else {
            return;
        };
        if self.last_placed.get() == Some(bounds) {
            return;
        }
        match place_android_text_input(&self.app, bounds) {
            Ok(()) => self.last_placed.set(Some(bounds)),
            Err(error) => log::warn!("Android IME editor placement failed: {error}"),
        }
    }

    fn show(&self) {
        if self.bridge_failed.get() {
            self.show_soft_input_only();
            return;
        }
        self.last_placed.set(None);

        let state = cranpose_ui::text_field_focus::focused_editor_state().unwrap_or_else(|| {
            ImeEditorState {
                text: String::new(),
                selection_start: 0,
                selection_end: 0,
                composition: None,
                single_line: true,
                keyboard_options: cranpose_ui::KeyboardOptions::default(),
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

    let key = map_keycode(Keycode::from(key_code.max(0) as u32));
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

    if key == Key::Unknown && text.is_empty() {
        return None;
    }

    Some(KeyEvent::new(key, text, modifiers, event_type))
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

        let key = map_keycode(event.key_code());
        let modifiers = map_modifiers(event.meta_state());

        let text = if event_type == KeyEventType::KeyDown {
            self.key_character(event)
        } else {
            String::new()
        };

        if key == Key::Unknown && text.is_empty() {
            return None;
        }

        let repeat_count = u32::try_from(event.repeat_count()).unwrap_or(0);
        Some(KeyEvent::new(key, text, modifiers, event_type).with_repeat_count(repeat_count))
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

fn map_modifiers(meta_state: MetaState) -> Modifiers {
    Modifiers {
        shift: meta_state.shift_on(),
        ctrl: meta_state.ctrl_on(),
        alt: meta_state.alt_on(),
        meta: meta_state.meta_on(),
    }
}

const ANDROID_KEYS: [(Keycode, Key); 110] = [
    (Keycode::A, Key::A),
    (Keycode::B, Key::B),
    (Keycode::C, Key::C),
    (Keycode::D, Key::D),
    (Keycode::E, Key::E),
    (Keycode::F, Key::F),
    (Keycode::G, Key::G),
    (Keycode::H, Key::H),
    (Keycode::I, Key::I),
    (Keycode::J, Key::J),
    (Keycode::K, Key::K),
    (Keycode::L, Key::L),
    (Keycode::M, Key::M),
    (Keycode::N, Key::N),
    (Keycode::O, Key::O),
    (Keycode::P, Key::P),
    (Keycode::Q, Key::Q),
    (Keycode::R, Key::R),
    (Keycode::S, Key::S),
    (Keycode::T, Key::T),
    (Keycode::U, Key::U),
    (Keycode::V, Key::V),
    (Keycode::W, Key::W),
    (Keycode::X, Key::X),
    (Keycode::Y, Key::Y),
    (Keycode::Z, Key::Z),
    (Keycode::Keycode0, Key::Zero),
    (Keycode::Keycode1, Key::One),
    (Keycode::Keycode2, Key::Two),
    (Keycode::Keycode3, Key::Three),
    (Keycode::Keycode4, Key::Four),
    (Keycode::Keycode5, Key::Five),
    (Keycode::Keycode6, Key::Six),
    (Keycode::Keycode7, Key::Seven),
    (Keycode::Keycode8, Key::Eight),
    (Keycode::Keycode9, Key::Nine),
    (Keycode::F1, Key::F1),
    (Keycode::F2, Key::F2),
    (Keycode::F3, Key::F3),
    (Keycode::F4, Key::F4),
    (Keycode::F5, Key::F5),
    (Keycode::F6, Key::F6),
    (Keycode::F7, Key::F7),
    (Keycode::F8, Key::F8),
    (Keycode::F9, Key::F9),
    (Keycode::F10, Key::F10),
    (Keycode::F11, Key::F11),
    (Keycode::F12, Key::F12),
    (Keycode::Del, Key::Backspace),
    (Keycode::ForwardDel, Key::Delete),
    (Keycode::Enter, Key::Enter),
    (Keycode::NumpadEnter, Key::NumPadEnter),
    (Keycode::Tab, Key::Tab),
    (Keycode::Space, Key::Spacebar),
    (Keycode::Escape, Key::Escape),
    (Keycode::ShiftLeft, Key::ShiftLeft),
    (Keycode::ShiftRight, Key::ShiftRight),
    (Keycode::CtrlLeft, Key::CtrlLeft),
    (Keycode::CtrlRight, Key::CtrlRight),
    (Keycode::AltLeft, Key::AltLeft),
    (Keycode::AltRight, Key::AltRight),
    (Keycode::MetaLeft, Key::MetaLeft),
    (Keycode::MetaRight, Key::MetaRight),
    (Keycode::Minus, Key::Minus),
    (Keycode::Equals, Key::Equals),
    (Keycode::LeftBracket, Key::LeftBracket),
    (Keycode::RightBracket, Key::RightBracket),
    (Keycode::Backslash, Key::Backslash),
    (Keycode::Semicolon, Key::Semicolon),
    (Keycode::Apostrophe, Key::Apostrophe),
    (Keycode::Comma, Key::Comma),
    (Keycode::Period, Key::Period),
    (Keycode::Slash, Key::Slash),
    (Keycode::Grave, Key::Grave),
    (Keycode::DpadUp, Key::DirectionUp),
    (Keycode::DpadDown, Key::DirectionDown),
    (Keycode::DpadLeft, Key::DirectionLeft),
    (Keycode::DpadRight, Key::DirectionRight),
    (Keycode::DpadCenter, Key::DirectionCenter),
    (Keycode::MoveHome, Key::MoveHome),
    (Keycode::MoveEnd, Key::MoveEnd),
    (Keycode::PageUp, Key::PageUp),
    (Keycode::PageDown, Key::PageDown),
    (Keycode::Back, Key::Back),
    (Keycode::Menu, Key::Menu),
    (Keycode::Search, Key::Search),
    (Keycode::Info, Key::Info),
    (Keycode::Guide, Key::Guide),
    (Keycode::Settings, Key::Settings),
    (Keycode::ChannelUp, Key::ChannelUp),
    (Keycode::ChannelDown, Key::ChannelDown),
    (Keycode::ProgRed, Key::ProgramRed),
    (Keycode::ProgGreen, Key::ProgramGreen),
    (Keycode::ProgYellow, Key::ProgramYellow),
    (Keycode::ProgBlue, Key::ProgramBlue),
    (Keycode::MediaPlay, Key::MediaPlay),
    (Keycode::MediaPause, Key::MediaPause),
    (Keycode::MediaPlayPause, Key::MediaPlayPause),
    (Keycode::MediaStop, Key::MediaStop),
    (Keycode::MediaNext, Key::MediaNext),
    (Keycode::MediaPrevious, Key::MediaPrevious),
    (Keycode::MediaRewind, Key::MediaRewind),
    (Keycode::MediaFastForward, Key::MediaFastForward),
    (Keycode::MediaRecord, Key::MediaRecord),
    (Keycode::ButtonA, Key::ButtonA),
    (Keycode::ButtonB, Key::ButtonB),
    (Keycode::ButtonX, Key::ButtonX),
    (Keycode::ButtonY, Key::ButtonY),
    (Keycode::ButtonSelect, Key::ButtonSelect),
    (Keycode::ButtonStart, Key::ButtonStart),
];

fn map_keycode(keycode: Keycode) -> Key {
    ANDROID_KEYS
        .iter()
        .find(|(code, _)| *code == keycode)
        .map_or(Key::Unknown, |(_, key)| *key)
}
