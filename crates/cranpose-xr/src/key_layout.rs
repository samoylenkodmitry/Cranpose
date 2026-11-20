//! The keys of the keyboard that a headset draws and where each lies on it.
//!
//! The keyboard has two pages. The letters page holds the digits, the 26 letters of the Latin
//! alphabet in the QWERTY order, shift, backspace, enter, space, a key for the other page and a key
//! that hides the keyboard. The symbols page holds the digits and the common symbols of the US
//! layout. There is no other language yet.
//!
//! A key lies at a place on the panel, in dp from the panel's top left corner. The composition
//! draws a key where this module says, and a scripted hand aims at the same place.

use cranpose::prelude::Key;

// The side of a key of one unit, in dp.
const KEY: f32 = 44.0;
// The space between two keys, in dp.
const GAP: f32 = 4.0;
// The space between the panel's edge and its keys, in dp.
const PAD: f32 = 12.0;
// The units in a row. A row that holds fewer leaves its ends free.
const ROW_UNITS: f32 = 10.0;

// The size of the keyboard's panel in dp.
pub(crate) const SIZE: [f32; 2] = [
    2.0 * PAD + ROW_UNITS * (KEY + GAP) - GAP,
    2.0 * PAD + 5.0 * KEY + 4.0 * GAP,
];

// A page of keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Page {
    #[default]
    Letters,
    Symbols,
}

// What a key does when it is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyKind {
    // Types `ch`. `key` is the physical key of a US keyboard that types it, and `shifted` says
    // whether that keyboard needs shift for it.
    Char { ch: char, key: Key, shifted: bool },
    Shift,
    Backspace,
    Enter,
    Space,
    // Shows another page
    Page(Page),
    // Closes the keyboard. The text field keeps its focus.
    Hide,
}

impl KeyKind {
    // The text on the key's cap. A letter shows in capitals while shift is on.
    pub(crate) fn label(self, capitals: bool) -> String {
        match self {
            Self::Char { ch, .. } if capitals => ch.to_ascii_uppercase().to_string(),
            Self::Char { ch, .. } => ch.to_string(),
            Self::Shift => "Shift".to_owned(),
            Self::Backspace => "Back".to_owned(),
            Self::Enter => "Enter".to_owned(),
            Self::Space => "Space".to_owned(),
            Self::Page(Page::Letters) => "abc".to_owned(),
            Self::Page(Page::Symbols) => "?123".to_owned(),
            Self::Hide => "Hide".to_owned(),
        }
    }
}

// A place in a row: a key of `units` units, or a free space of that width.
#[derive(Clone, Copy)]
enum Slot {
    Key(KeyKind, f32),
    Gap(f32),
}

const fn ch(ch: char, key: Key) -> Slot {
    Slot::Key(
        KeyKind::Char {
            ch,
            key,
            shifted: false,
        },
        1.0,
    )
}

const fn shifted(ch: char, key: Key) -> Slot {
    Slot::Key(
        KeyKind::Char {
            ch,
            key,
            shifted: true,
        },
        1.0,
    )
}

const DIGITS: [Slot; 10] = [
    ch('1', Key::One),
    ch('2', Key::Two),
    ch('3', Key::Three),
    ch('4', Key::Four),
    ch('5', Key::Five),
    ch('6', Key::Six),
    ch('7', Key::Seven),
    ch('8', Key::Eight),
    ch('9', Key::Nine),
    ch('0', Key::Zero),
];

const TOP_LETTERS: [Slot; 10] = [
    ch('q', Key::Q),
    ch('w', Key::W),
    ch('e', Key::E),
    ch('r', Key::R),
    ch('t', Key::T),
    ch('y', Key::Y),
    ch('u', Key::U),
    ch('i', Key::I),
    ch('o', Key::O),
    ch('p', Key::P),
];

const HOME_LETTERS: [Slot; 11] = [
    Slot::Gap(0.5),
    ch('a', Key::A),
    ch('s', Key::S),
    ch('d', Key::D),
    ch('f', Key::F),
    ch('g', Key::G),
    ch('h', Key::H),
    ch('j', Key::J),
    ch('k', Key::K),
    ch('l', Key::L),
    Slot::Gap(0.5),
];

const BOTTOM_LETTERS: [Slot; 9] = [
    Slot::Key(KeyKind::Shift, 1.5),
    ch('z', Key::Z),
    ch('x', Key::X),
    ch('c', Key::C),
    ch('v', Key::V),
    ch('b', Key::B),
    ch('n', Key::N),
    ch('m', Key::M),
    Slot::Key(KeyKind::Backspace, 1.5),
];

const fn last_row(page: Page) -> [Slot; 6] {
    [
        Slot::Key(KeyKind::Page(page), 1.5),
        ch(',', Key::Comma),
        Slot::Key(KeyKind::Space, 4.0),
        ch('.', Key::Period),
        Slot::Key(KeyKind::Enter, 1.5),
        Slot::Key(KeyKind::Hide, 1.0),
    ]
}

const LETTERS_LAST: [Slot; 6] = last_row(Page::Symbols);
const SYMBOLS_LAST: [Slot; 6] = last_row(Page::Letters);

const SYMBOLS_TOP: [Slot; 10] = [
    shifted('@', Key::Two),
    shifted('#', Key::Three),
    shifted('$', Key::Four),
    shifted('%', Key::Five),
    shifted('&', Key::Seven),
    shifted('*', Key::Eight),
    shifted('(', Key::Nine),
    shifted(')', Key::Zero),
    ch('-', Key::Minus),
    shifted('+', Key::Equals),
];

const SYMBOLS_HOME: [Slot; 11] = [
    Slot::Gap(0.5),
    shifted('!', Key::One),
    shifted('"', Key::Apostrophe),
    ch('\'', Key::Apostrophe),
    shifted(':', Key::Semicolon),
    ch(';', Key::Semicolon),
    ch('/', Key::Slash),
    shifted('?', Key::Slash),
    ch('=', Key::Equals),
    shifted('_', Key::Minus),
    Slot::Gap(0.5),
];

const SYMBOLS_BOTTOM: [Slot; 9] = [
    ch('[', Key::LeftBracket),
    ch(']', Key::RightBracket),
    shifted('{', Key::LeftBracket),
    shifted('}', Key::RightBracket),
    shifted('<', Key::Comma),
    shifted('>', Key::Period),
    ch('\\', Key::Backslash),
    shifted('|', Key::Backslash),
    Slot::Key(KeyKind::Backspace, 2.0),
];

fn rows(page: Page) -> [&'static [Slot]; 5] {
    match page {
        Page::Letters => [
            &DIGITS,
            &TOP_LETTERS,
            &HOME_LETTERS,
            &BOTTOM_LETTERS,
            &LETTERS_LAST,
        ],
        Page::Symbols => [
            &DIGITS,
            &SYMBOLS_TOP,
            &SYMBOLS_HOME,
            &SYMBOLS_BOTTOM,
            &SYMBOLS_LAST,
        ],
    }
}

// A key and the rectangle it covers on the panel: left, top, width and height in dp from the
// panel's top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacedKey {
    pub(crate) kind: KeyKind,
    pub(crate) rect: [f32; 4],
}

impl PlacedKey {
    #[cfg_attr(
        not(any(test, all(feature = "scripted-hand", headset))),
        expect(dead_code, reason = "a scripted hand aims at the keys")
    )]
    // The key's center in dp from the panel's top left corner.
    pub(crate) fn center(&self) -> [f32; 2] {
        let [left, top, width, height] = self.rect;
        [left + width / 2.0, top + height / 2.0]
    }
}

// The keys of `page` with their places, row by row from the top.
pub(crate) fn keys(page: Page) -> impl Iterator<Item = PlacedKey> {
    rows(page).into_iter().enumerate().flat_map(|(row, slots)| {
        let top = PAD + row as f32 * (KEY + GAP);
        let mut left = PAD;
        slots.iter().filter_map(move |slot| {
            let (units, kind) = match *slot {
                Slot::Key(kind, units) => (units, Some(kind)),
                Slot::Gap(units) => (units, None),
            };
            let at = left;
            left += units * (KEY + GAP);
            kind.map(|kind| PlacedKey {
                kind,
                rect: [at, top, units * (KEY + GAP) - GAP, KEY],
            })
        })
    })
}

#[cfg_attr(
    not(any(test, all(feature = "scripted-hand", headset))),
    expect(dead_code, reason = "a scripted hand aims at the keys")
)]
// The page and the key that type `ch`, if the keyboard has one.
pub(crate) fn find(ch: char) -> Option<(Page, PlacedKey)> {
    let wanted = ch.to_ascii_lowercase();
    [Page::Letters, Page::Symbols].into_iter().find_map(|page| {
        keys(page)
            .find(|key| {
                matches!(key.kind, KeyKind::Char { ch, .. } if ch == wanted)
                    || (ch == ' ' && key.kind == KeyKind::Space)
            })
            .map(|key| (page, key))
    })
}
