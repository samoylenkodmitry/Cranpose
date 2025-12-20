//! Keyboard input event types for Compose-RS.
//!
//! This module provides platform-independent keyboard event types
//! that are used to route keyboard input to focused components.

use std::fmt;

/// Type of keyboard event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventType {
    /// Key was pressed down.
    KeyDown,
    /// Key was released.
    KeyUp,
}

/// Modifier keys state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Shift key is pressed.
    pub shift: bool,
    /// Control key is pressed (Cmd on macOS).
    pub ctrl: bool,
    /// Alt key is pressed (Option on macOS).
    pub alt: bool,
    /// Meta/Super key is pressed (Windows key, Cmd on macOS).
    pub meta: bool,
}

impl Modifiers {
    /// No modifiers pressed.
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };

    /// Returns true if any modifier is pressed.
    pub fn any(&self) -> bool {
        self.shift || self.ctrl || self.alt || self.meta
    }

    /// Returns true if Ctrl (or Cmd on macOS) is pressed.
    pub fn command_or_ctrl(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.meta
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.ctrl
        }
    }
}

/// Physical key codes for keyboard input.
///
/// These represent physical keys on the keyboard, independent of
/// the character they produce (which depends on keyboard layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    // Letters
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,

    // Numbers
    Digit0, Digit1, Digit2, Digit3, Digit4,
    Digit5, Digit6, Digit7, Digit8, Digit9,

    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,

    // Navigation
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Home, End, PageUp, PageDown,

    // Editing
    Backspace,
    Delete,
    Enter,
    Tab,
    Space,
    Escape,

    // Modifiers (for completeness, usually detected via Modifiers struct)
    ShiftLeft, ShiftRight,
    ControlLeft, ControlRight,
    AltLeft, AltRight,
    MetaLeft, MetaRight,

    // Punctuation and symbols
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Backquote,

    /// Key not recognized or not mapped.
    Unknown,
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

    /// Returns true if this is a key up event.
    pub fn is_key_up(&self) -> bool {
        self.event_type == KeyEventType::KeyUp
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
mod tests {
    use super::*;

    #[test]
    fn key_event_creation() {
        let event = KeyEvent::key_down(KeyCode::A, "a");
        assert_eq!(event.key_code, KeyCode::A);
        assert_eq!(event.text, "a");
        assert!(event.is_key_down());
        assert!(event.has_text());
    }

    #[test]
    fn key_event_with_modifiers() {
        let modifiers = Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
            meta: false,
        };
        let event = KeyEvent::key_down_with_modifiers(KeyCode::A, "A", modifiers);
        assert_eq!(event.text, "A");
        assert!(event.modifiers.shift);
    }

    #[test]
    fn backspace_has_no_text() {
        let event = KeyEvent::key_down(KeyCode::Backspace, "");
        assert!(!event.has_text());
    }

    #[test]
    fn modifiers_any() {
        assert!(!Modifiers::NONE.any());
        assert!(Modifiers { shift: true, ..Modifiers::NONE }.any());
    }
}
