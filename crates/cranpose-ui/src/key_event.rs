//! Keyboard input event types for Cranpose.
//!
//! This module provides platform-independent keyboard event types
//! that are used to route keyboard input to focused components.

use std::fmt;

pub use cranpose_foundation::Modifiers;

/// Type of keyboard event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventType {
    /// Key was pressed down.
    KeyDown,
    /// Key was released.
    KeyUp,
}

/// A key on a keyboard, remote control or game controller, named as in
/// Jetpack Compose's `androidx.compose.ui.input.key.Key`.
///
/// Letters and digits name the physical key, independent of the character
/// the keyboard layout gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// The A key.
    A,
    /// The B key.
    B,
    /// The C key.
    C,
    /// The D key.
    D,
    /// The E key.
    E,
    /// The F key.
    F,
    /// The G key.
    G,
    /// The H key.
    H,
    /// The I key.
    I,
    /// The J key.
    J,
    /// The K key.
    K,
    /// The L key.
    L,
    /// The M key.
    M,
    /// The N key.
    N,
    /// The O key.
    O,
    /// The P key.
    P,
    /// The Q key.
    Q,
    /// The R key.
    R,
    /// The S key.
    S,
    /// The T key.
    T,
    /// The U key.
    U,
    /// The V key.
    V,
    /// The W key.
    W,
    /// The X key.
    X,
    /// The Y key.
    Y,
    /// The Z key.
    Z,

    /// The 0 key on the main number row.
    Zero,
    /// The 1 key on the main number row.
    One,
    /// The 2 key on the main number row.
    Two,
    /// The 3 key on the main number row.
    Three,
    /// The 4 key on the main number row.
    Four,
    /// The 5 key on the main number row.
    Five,
    /// The 6 key on the main number row.
    Six,
    /// The 7 key on the main number row.
    Seven,
    /// The 8 key on the main number row.
    Eight,
    /// The 9 key on the main number row.
    Nine,

    /// The F1 function key.
    F1,
    /// The F2 function key.
    F2,
    /// The F3 function key.
    F3,
    /// The F4 function key.
    F4,
    /// The F5 function key.
    F5,
    /// The F6 function key.
    F6,
    /// The F7 function key.
    F7,
    /// The F8 function key.
    F8,
    /// The F9 function key.
    F9,
    /// The F10 function key.
    F10,
    /// The F11 function key.
    F11,
    /// The F12 function key.
    F12,

    /// The up arrow key, or up on a D-pad.
    DirectionUp,
    /// The down arrow key, or down on a D-pad.
    DirectionDown,
    /// The left arrow key, or left on a D-pad.
    DirectionLeft,
    /// The right arrow key, or right on a D-pad.
    DirectionRight,
    /// The center button of a D-pad: OK or Select on a remote.
    DirectionCenter,
    /// The Home key, which moves to the start of a line or list.
    MoveHome,
    /// The End key, which moves to the end of a line or list.
    MoveEnd,
    /// The Page Up key.
    PageUp,
    /// The Page Down key.
    PageDown,

    /// The Backspace key, which deletes backward.
    Backspace,
    /// The Delete key, which deletes forward.
    Delete,
    /// The main Enter or Return key.
    Enter,
    /// The Enter key on the number pad.
    NumPadEnter,
    /// The Tab key.
    Tab,
    /// The space bar.
    Spacebar,
    /// The Escape key.
    Escape,

    /// The left Shift key.
    ShiftLeft,
    /// The right Shift key.
    ShiftRight,
    /// The left Control key.
    CtrlLeft,
    /// The right Control key.
    CtrlRight,
    /// The left Alt key, called Option on macOS.
    AltLeft,
    /// The right Alt key, called Option on macOS.
    AltRight,
    /// The left Meta key: Command on macOS, the Windows key elsewhere.
    MetaLeft,
    /// The right Meta key: Command on macOS, the Windows key elsewhere.
    MetaRight,

    /// The minus and underscore key on US layouts.
    Minus,
    /// The equals and plus key on US layouts.
    Equals,
    /// The left square bracket key on US layouts.
    LeftBracket,
    /// The right square bracket key on US layouts.
    RightBracket,
    /// The backslash key on US layouts.
    Backslash,
    /// The semicolon and colon key on US layouts.
    Semicolon,
    /// The apostrophe and quotation mark key on US layouts.
    Apostrophe,
    /// The comma key on US layouts.
    Comma,
    /// The period key on US layouts.
    Period,
    /// The forward slash key on US layouts.
    Slash,
    /// The grave accent and tilde key on US layouts, left of the 1 key.
    Grave,

    /// The Back key of a remote or of the Android system navigation.
    Back,
    /// The Menu key.
    Menu,
    /// The Search key of a remote.
    Search,
    /// The Info key of a remote.
    Info,
    /// The Guide key of a remote, which opens the program guide.
    Guide,
    /// The Settings key of a remote.
    Settings,
    /// The Channel Up key of a remote.
    ChannelUp,
    /// The Channel Down key of a remote.
    ChannelDown,
    /// The red program key of a remote.
    ProgramRed,
    /// The green program key of a remote.
    ProgramGreen,
    /// The yellow program key of a remote.
    ProgramYellow,
    /// The blue program key of a remote.
    ProgramBlue,

    /// The Play media key.
    MediaPlay,
    /// The Pause media key.
    MediaPause,
    /// The Play/Pause media key.
    MediaPlayPause,
    /// The Stop media key.
    MediaStop,
    /// The Next track media key.
    MediaNext,
    /// The Previous track media key.
    MediaPrevious,
    /// The Rewind media key.
    MediaRewind,
    /// The Fast Forward media key.
    MediaFastForward,
    /// The Record media key.
    MediaRecord,

    /// The A button of a game controller.
    ButtonA,
    /// The B button of a game controller.
    ButtonB,
    /// The X button of a game controller.
    ButtonX,
    /// The Y button of a game controller.
    ButtonY,
    /// The Select button of a game controller.
    ButtonSelect,
    /// The Start button of a game controller.
    ButtonStart,

    /// Key not recognized or not mapped.
    Unknown,
}

const DOM_KEY_CODES: [(&str, Key); 93] = [
    ("KeyA", Key::A),
    ("KeyB", Key::B),
    ("KeyC", Key::C),
    ("KeyD", Key::D),
    ("KeyE", Key::E),
    ("KeyF", Key::F),
    ("KeyG", Key::G),
    ("KeyH", Key::H),
    ("KeyI", Key::I),
    ("KeyJ", Key::J),
    ("KeyK", Key::K),
    ("KeyL", Key::L),
    ("KeyM", Key::M),
    ("KeyN", Key::N),
    ("KeyO", Key::O),
    ("KeyP", Key::P),
    ("KeyQ", Key::Q),
    ("KeyR", Key::R),
    ("KeyS", Key::S),
    ("KeyT", Key::T),
    ("KeyU", Key::U),
    ("KeyV", Key::V),
    ("KeyW", Key::W),
    ("KeyX", Key::X),
    ("KeyY", Key::Y),
    ("KeyZ", Key::Z),
    ("Digit0", Key::Zero),
    ("Digit1", Key::One),
    ("Digit2", Key::Two),
    ("Digit3", Key::Three),
    ("Digit4", Key::Four),
    ("Digit5", Key::Five),
    ("Digit6", Key::Six),
    ("Digit7", Key::Seven),
    ("Digit8", Key::Eight),
    ("Digit9", Key::Nine),
    ("F1", Key::F1),
    ("F2", Key::F2),
    ("F3", Key::F3),
    ("F4", Key::F4),
    ("F5", Key::F5),
    ("F6", Key::F6),
    ("F7", Key::F7),
    ("F8", Key::F8),
    ("F9", Key::F9),
    ("F10", Key::F10),
    ("F11", Key::F11),
    ("F12", Key::F12),
    ("ArrowUp", Key::DirectionUp),
    ("ArrowDown", Key::DirectionDown),
    ("ArrowLeft", Key::DirectionLeft),
    ("ArrowRight", Key::DirectionRight),
    ("Home", Key::MoveHome),
    ("End", Key::MoveEnd),
    ("PageUp", Key::PageUp),
    ("PageDown", Key::PageDown),
    ("Backspace", Key::Backspace),
    ("Delete", Key::Delete),
    ("Enter", Key::Enter),
    ("NumpadEnter", Key::NumPadEnter),
    ("Tab", Key::Tab),
    ("Space", Key::Spacebar),
    ("Escape", Key::Escape),
    ("ShiftLeft", Key::ShiftLeft),
    ("ShiftRight", Key::ShiftRight),
    ("ControlLeft", Key::CtrlLeft),
    ("ControlRight", Key::CtrlRight),
    ("AltLeft", Key::AltLeft),
    ("AltRight", Key::AltRight),
    ("MetaLeft", Key::MetaLeft),
    ("MetaRight", Key::MetaRight),
    ("Minus", Key::Minus),
    ("Equal", Key::Equals),
    ("BracketLeft", Key::LeftBracket),
    ("BracketRight", Key::RightBracket),
    ("Backslash", Key::Backslash),
    ("Semicolon", Key::Semicolon),
    ("Quote", Key::Apostrophe),
    ("Comma", Key::Comma),
    ("Period", Key::Period),
    ("Slash", Key::Slash),
    ("Backquote", Key::Grave),
    ("ContextMenu", Key::Menu),
    ("BrowserSearch", Key::Search),
    ("MediaPlay", Key::MediaPlay),
    ("MediaPause", Key::MediaPause),
    ("MediaPlayPause", Key::MediaPlayPause),
    ("MediaStop", Key::MediaStop),
    ("MediaTrackNext", Key::MediaNext),
    ("MediaTrackPrevious", Key::MediaPrevious),
    ("MediaRewind", Key::MediaRewind),
    ("MediaFastForward", Key::MediaFastForward),
    ("MediaRecord", Key::MediaRecord),
];

// W3C `KeyboardEvent.key` values of the keys a TV remote sends, which TV
// browsers report with no `code`.
const DOM_KEY_VALUES: [(&str, Key); 23] = [
    ("GoBack", Key::Back),
    ("BrowserBack", Key::Back),
    ("XF86Back", Key::Back),
    ("Enter", Key::Enter),
    ("ContextMenu", Key::Menu),
    ("BrowserSearch", Key::Search),
    ("Info", Key::Info),
    ("Guide", Key::Guide),
    ("Settings", Key::Settings),
    ("ChannelUp", Key::ChannelUp),
    ("ChannelDown", Key::ChannelDown),
    ("ColorF0Red", Key::ProgramRed),
    ("ColorF1Green", Key::ProgramGreen),
    ("ColorF2Yellow", Key::ProgramYellow),
    ("ColorF3Blue", Key::ProgramBlue),
    ("MediaPlay", Key::MediaPlay),
    ("MediaPause", Key::MediaPause),
    ("MediaPlayPause", Key::MediaPlayPause),
    ("MediaStop", Key::MediaStop),
    ("MediaTrackNext", Key::MediaNext),
    ("MediaTrackPrevious", Key::MediaPrevious),
    ("MediaRewind", Key::MediaRewind),
    ("MediaFastForward", Key::MediaFastForward),
];

// The key codes Samsung's Tizen and LG's webOS give their remotes' Back key.
const TV_BACK_KEY_CODES: [u32; 2] = [10009, 461];

impl Key {
    /// The key a W3C `KeyboardEvent.code` value names, such as `"KeyA"`,
    /// `"ArrowUp"` or `"MediaPlayPause"`. A code with no counterpart here is
    /// [`Key::Unknown`].
    pub fn from_dom_code(code: &str) -> Self {
        DOM_KEY_CODES
            .iter()
            .find(|(name, _)| *name == code)
            .map_or(Self::Unknown, |(_, key)| *key)
    }

    /// The key a browser event names, as a TV browser reports a remote: by
    /// its `code` when it has one, else by its `key` value (`"GoBack"`,
    /// `"MediaPlayPause"`, `"ChannelUp"`), else by the `keyCode` Tizen
    /// (10009) and webOS (461) give the Back key.
    pub fn from_dom_event(code: &str, key: &str, key_code: u32) -> Self {
        match Self::from_dom_code(code) {
            Self::Unknown if TV_BACK_KEY_CODES.contains(&key_code) => Self::Back,
            Self::Unknown => DOM_KEY_VALUES
                .iter()
                .find(|(name, _)| *name == key)
                .map_or(Self::Unknown, |(_, found)| *found),
            known => known,
        }
    }
}

/// What the platform reported about a key event beyond the key, as
/// `KeyEvent.nativeKeyEvent` holds in Compose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeKeyEvent {
    /// How many times the key repeated while held: 0 for the press itself,
    /// 1 for the first repeat, and so on on Android. The desktop and the web
    /// report only whether an event repeats, so every repeat there is 1.
    pub repeat_count: u32,
}

/// A keyboard input event.
///
/// Holds the key, whether it went down or up, the modifier keys held with it
/// and the text it typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    key: Key,
    text: String,
    modifiers: Modifiers,
    type_: KeyEventType,
    native: NativeKeyEvent,
}

impl KeyEvent {
    /// Creates a key event. `text` is what the key typed with the keyboard
    /// layout and modifiers applied (Shift+A types "A"), empty for keys that
    /// type nothing.
    pub fn new(
        key: Key,
        text: impl Into<String>,
        modifiers: Modifiers,
        type_: KeyEventType,
    ) -> Self {
        Self {
            key,
            text: text.into(),
            modifiers,
            type_,
            native: NativeKeyEvent::default(),
        }
    }

    /// Creates a key down event with the given key and text.
    pub fn key_down(key: Key, text: impl Into<String>) -> Self {
        Self::new(key, text, Modifiers::NONE, KeyEventType::KeyDown)
    }

    /// Creates a key up event for `key`.
    pub fn key_up(key: Key) -> Self {
        Self::new(key, "", Modifiers::NONE, KeyEventType::KeyUp)
    }

    /// Creates a key down event with modifiers.
    pub fn key_down_with_modifiers(
        key: Key,
        text: impl Into<String>,
        modifiers: Modifiers,
    ) -> Self {
        Self::new(key, text, modifiers, KeyEventType::KeyDown)
    }

    /// This event as the `repeat_count`th repeat of a held key.
    #[must_use]
    pub fn with_repeat_count(mut self, repeat_count: u32) -> Self {
        self.native.repeat_count = repeat_count;
        self
    }

    /// The key that went down or up.
    pub fn key(&self) -> Key {
        self.key
    }

    /// Whether the key went down or up.
    pub fn type_(&self) -> KeyEventType {
        self.type_
    }

    /// The text this key typed, empty for keys that type nothing.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The modifier keys held with this key.
    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Whether a Shift key was held.
    pub fn is_shift_pressed(&self) -> bool {
        self.modifiers.shift
    }

    /// Whether a Control key was held.
    pub fn is_ctrl_pressed(&self) -> bool {
        self.modifiers.ctrl
    }

    /// Whether an Alt key was held.
    pub fn is_alt_pressed(&self) -> bool {
        self.modifiers.alt
    }

    /// Whether a Meta key was held.
    pub fn is_meta_pressed(&self) -> bool {
        self.modifiers.meta
    }

    /// The Unicode code point of the character this key typed, if it typed
    /// one.
    pub fn utf16_code_point(&self) -> Option<u32> {
        self.text.chars().next().map(u32::from)
    }

    /// What the platform reported beyond the key, such as the repeat count.
    pub fn native_key_event(&self) -> NativeKeyEvent {
        self.native
    }

    /// Returns true if this is a key down event.
    pub fn is_key_down(&self) -> bool {
        self.type_ == KeyEventType::KeyDown
    }

    /// Returns true if this key produces printable text.
    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
}

impl fmt::Display for KeyEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "KeyEvent({:?}, text=\"{}\", {:?})",
            self.key, self.text, self.type_
        )
    }
}

#[cfg(test)]
#[path = "tests/key_event_tests.rs"]
mod tests;
