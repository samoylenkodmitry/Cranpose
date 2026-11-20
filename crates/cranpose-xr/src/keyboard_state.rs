//! What the keyboard that a headset draws shows, and the key event that each key press makes.

use cranpose::prelude::{Key, KeyEvent, Modifiers};

use crate::key_layout::{KeyKind, Page};

// Whether the next letters come in capitals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Shift {
    #[default]
    Off,
    // The next letter comes in a capital, then shift goes off.
    Once,
    // Every letter comes in a capital until shift is pressed again.
    Locked,
}

// What a key press asks of the keyboard.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Pressed {
    // The key types or edits: the key event that a hardware keyboard sends for it
    Typed(KeyEvent),
    // The keyboard closes
    Hide,
    // Only the keyboard's own page or shift changed
    Nothing,
}

// The page that the keyboard shows and its shift.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyboardState {
    pub(crate) page: Page,
    pub(crate) shift: Shift,
}

impl KeyboardState {
    // Whether the letters on the caps show in capitals.
    pub(crate) fn capitals(&self) -> bool {
        self.shift != Shift::Off
    }

    // Applies a press of a key that lies on `self.page`.
    //
    // A letter types its character in a capital while shift is on, with shift in the event's
    // modifiers, as a hardware keyboard does. A symbol that a US keyboard reaches with shift
    // carries shift too. Backspace and enter type no text: the field reads them by their keys.
    pub(crate) fn press(&mut self, kind: KeyKind) -> Pressed {
        match kind {
            KeyKind::Char { ch, key, shifted } => {
                let capital = ch.is_ascii_lowercase() && self.capitals();
                if self.shift == Shift::Once {
                    self.shift = Shift::Off;
                }
                let (text, modifiers) = if capital {
                    (ch.to_ascii_uppercase(), Modifiers::SHIFT)
                } else if shifted {
                    (ch, Modifiers::SHIFT)
                } else {
                    (ch, Modifiers::NONE)
                };
                Pressed::Typed(KeyEvent::key_down_with_modifiers(
                    key,
                    text.to_string(),
                    modifiers,
                ))
            }
            KeyKind::Space => Pressed::Typed(KeyEvent::key_down(Key::Spacebar, " ")),
            KeyKind::Backspace => Pressed::Typed(KeyEvent::key_down(Key::Backspace, "")),
            KeyKind::Enter => Pressed::Typed(KeyEvent::key_down(Key::Enter, "")),
            KeyKind::Shift => {
                self.shift = match self.shift {
                    Shift::Off => Shift::Once,
                    Shift::Once => Shift::Locked,
                    Shift::Locked => Shift::Off,
                };
                Pressed::Nothing
            }
            KeyKind::Page(page) => {
                self.page = page;
                Pressed::Nothing
            }
            KeyKind::Hide => Pressed::Hide,
        }
    }
}
