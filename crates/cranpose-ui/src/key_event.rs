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

/// Physical key codes for keyboard input.
///
/// These represent physical keys on the keyboard, independent of
/// the character they produce (which depends on keyboard layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
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
    Digit0,
    /// The 1 key on the main number row.
    Digit1,
    /// The 2 key on the main number row.
    Digit2,
    /// The 3 key on the main number row.
    Digit3,
    /// The 4 key on the main number row.
    Digit4,
    /// The 5 key on the main number row.
    Digit5,
    /// The 6 key on the main number row.
    Digit6,
    /// The 7 key on the main number row.
    Digit7,
    /// The 8 key on the main number row.
    Digit8,
    /// The 9 key on the main number row.
    Digit9,

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

    /// The up arrow key.
    ArrowUp,
    /// The down arrow key.
    ArrowDown,
    /// The left arrow key.
    ArrowLeft,
    /// The right arrow key.
    ArrowRight,
    /// The Home key.
    Home,
    /// The End key.
    End,
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
    /// The Tab key.
    Tab,
    /// The space bar.
    Space,
    /// The Escape key.
    Escape,

    /// The left Shift key.
    ShiftLeft,
    /// The right Shift key.
    ShiftRight,
    /// The left Control key.
    ControlLeft,
    /// The right Control key.
    ControlRight,
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
    Equal,
    /// The left square bracket key on US layouts.
    BracketLeft,
    /// The right square bracket key on US layouts.
    BracketRight,
    /// The backslash key on US layouts.
    Backslash,
    /// The semicolon and colon key on US layouts.
    Semicolon,
    /// The apostrophe and quotation mark key on US layouts.
    Quote,
    /// The comma key on US layouts.
    Comma,
    /// The period key on US layouts.
    Period,
    /// The forward slash key on US layouts.
    Slash,
    /// The grave accent and tilde key on US layouts, left of the 1 key.
    Backquote,

    /// Key not recognized or not mapped.
    Unknown,
}

const DOM_KEY_CODES: [(&str, KeyCode); 82] = [
    ("KeyA", KeyCode::A),
    ("KeyB", KeyCode::B),
    ("KeyC", KeyCode::C),
    ("KeyD", KeyCode::D),
    ("KeyE", KeyCode::E),
    ("KeyF", KeyCode::F),
    ("KeyG", KeyCode::G),
    ("KeyH", KeyCode::H),
    ("KeyI", KeyCode::I),
    ("KeyJ", KeyCode::J),
    ("KeyK", KeyCode::K),
    ("KeyL", KeyCode::L),
    ("KeyM", KeyCode::M),
    ("KeyN", KeyCode::N),
    ("KeyO", KeyCode::O),
    ("KeyP", KeyCode::P),
    ("KeyQ", KeyCode::Q),
    ("KeyR", KeyCode::R),
    ("KeyS", KeyCode::S),
    ("KeyT", KeyCode::T),
    ("KeyU", KeyCode::U),
    ("KeyV", KeyCode::V),
    ("KeyW", KeyCode::W),
    ("KeyX", KeyCode::X),
    ("KeyY", KeyCode::Y),
    ("KeyZ", KeyCode::Z),
    ("Digit0", KeyCode::Digit0),
    ("Digit1", KeyCode::Digit1),
    ("Digit2", KeyCode::Digit2),
    ("Digit3", KeyCode::Digit3),
    ("Digit4", KeyCode::Digit4),
    ("Digit5", KeyCode::Digit5),
    ("Digit6", KeyCode::Digit6),
    ("Digit7", KeyCode::Digit7),
    ("Digit8", KeyCode::Digit8),
    ("Digit9", KeyCode::Digit9),
    ("F1", KeyCode::F1),
    ("F2", KeyCode::F2),
    ("F3", KeyCode::F3),
    ("F4", KeyCode::F4),
    ("F5", KeyCode::F5),
    ("F6", KeyCode::F6),
    ("F7", KeyCode::F7),
    ("F8", KeyCode::F8),
    ("F9", KeyCode::F9),
    ("F10", KeyCode::F10),
    ("F11", KeyCode::F11),
    ("F12", KeyCode::F12),
    ("ArrowUp", KeyCode::ArrowUp),
    ("ArrowDown", KeyCode::ArrowDown),
    ("ArrowLeft", KeyCode::ArrowLeft),
    ("ArrowRight", KeyCode::ArrowRight),
    ("Home", KeyCode::Home),
    ("End", KeyCode::End),
    ("PageUp", KeyCode::PageUp),
    ("PageDown", KeyCode::PageDown),
    ("Backspace", KeyCode::Backspace),
    ("Delete", KeyCode::Delete),
    ("Enter", KeyCode::Enter),
    ("NumpadEnter", KeyCode::Enter),
    ("Tab", KeyCode::Tab),
    ("Space", KeyCode::Space),
    ("Escape", KeyCode::Escape),
    ("ShiftLeft", KeyCode::ShiftLeft),
    ("ShiftRight", KeyCode::ShiftRight),
    ("ControlLeft", KeyCode::ControlLeft),
    ("ControlRight", KeyCode::ControlRight),
    ("AltLeft", KeyCode::AltLeft),
    ("AltRight", KeyCode::AltRight),
    ("MetaLeft", KeyCode::MetaLeft),
    ("MetaRight", KeyCode::MetaRight),
    ("Minus", KeyCode::Minus),
    ("Equal", KeyCode::Equal),
    ("BracketLeft", KeyCode::BracketLeft),
    ("BracketRight", KeyCode::BracketRight),
    ("Backslash", KeyCode::Backslash),
    ("Semicolon", KeyCode::Semicolon),
    ("Quote", KeyCode::Quote),
    ("Comma", KeyCode::Comma),
    ("Period", KeyCode::Period),
    ("Slash", KeyCode::Slash),
    ("Backquote", KeyCode::Backquote),
];

impl KeyCode {
    /// The key a W3C `KeyboardEvent.code` value names, such as `"KeyA"`,
    /// `"ArrowUp"` or `"ShiftLeft"`; `"NumpadEnter"` is [`KeyCode::Enter`]. A
    /// code with no counterpart here is [`KeyCode::Unknown`].
    pub fn from_dom_code(code: &str) -> Self {
        DOM_KEY_CODES
            .iter()
            .find(|(name, _)| *name == code)
            .map_or(Self::Unknown, |(_, key)| *key)
    }
}

/// A keyboard input event.
///
/// Contains information about which key was pressed/released,
/// the text it produces (if any), and modifier state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    /// The physical key that was pressed.
    pub key_code: KeyCode,
    /// The text produced by this key press (may be empty for non-character keys).
    /// This accounts for keyboard layout and modifiers (e.g., Shift+A = "A").
    pub text: String,
    /// Current state of modifier keys.
    pub modifiers: Modifiers,
    /// Type of event (down or up).
    pub event_type: KeyEventType,
}

impl KeyEvent {
    /// Creates a new key event.
    pub fn new(
        key_code: KeyCode,
        text: impl Into<String>,
        modifiers: Modifiers,
        event_type: KeyEventType,
    ) -> Self {
        Self {
            key_code,
            text: text.into(),
            modifiers,
            event_type,
        }
    }

    /// Creates a key down event with the given key code and text.
    pub fn key_down(key_code: KeyCode, text: impl Into<String>) -> Self {
        Self::new(key_code, text, Modifiers::NONE, KeyEventType::KeyDown)
    }

    /// Creates a key down event with modifiers.
    pub fn key_down_with_modifiers(
        key_code: KeyCode,
        text: impl Into<String>,
        modifiers: Modifiers,
    ) -> Self {
        Self::new(key_code, text, modifiers, KeyEventType::KeyDown)
    }

    /// Returns true if this is a key down event.
    pub fn is_key_down(&self) -> bool {
        self.event_type == KeyEventType::KeyDown
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
            self.key_code, self.text, self.event_type
        )
    }
}

#[cfg(test)]
#[path = "tests/key_event_tests.rs"]
mod tests;
