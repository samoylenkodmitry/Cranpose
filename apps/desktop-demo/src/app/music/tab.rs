// The Music tab: "Clair de lune" as a tree of composables. Each note is a
// `NoteBar`: a rectangle on the roll that sounds while it is composed and
// the clock stands in it. Each hand is a `Part` that composes its notes.
// `Instrument`, `Transpose` and `Volume` wrap the parts and change how they
// sound. The `Roll` is a scrolling column of the song's seconds: the piece
// moves it. A hand that moves it plays the notes that cross its lower edge.
// The `Keyboard` is a row of `PianoKey`s: each lights under the note that
// holds it down and plays under a finger.

use cranpose::liquid::prelude::*;
use cranpose_core::{remember, with_key, LaunchedEffect};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, Color, Column, ColumnSpec, FocusRequester, Key,
    KeyEvent, KeyEventType, Modifier,
};

use super::{
    clair_de_lune::clair_de_lune,
    controls::ControlPanel,
    key_range::KeyRange,
    keyboard::Keyboard,
    night_sky::NightSky,
    part::Part,
    performance::{rememberPerformance, Perform, Performance},
    piano_key::KeyboardColors,
    roll::Roll,
    settings::{rememberSettings, Settings},
    song::Song,
    typed_keys::piano_key,
    voicing::{Instrument, Transpose, Voicing, Volume},
};

const KEYS: KeyRange = KeyRange::PIANO;
const KEYBOARD_TAG: &str = "MusicKeyboard";
const ROLL_TAG: &str = "MusicRoll";
const PANEL_MARGIN: f32 = 12.0;
const KEYBOARD_HEIGHT: f32 = 104.0;
// Song seconds an arrow key moves the piece by.
const ARROW_SECONDS: f64 = 5.0;
// The light of the moon, for the controls and for a key under a finger.
const ACCENT: Color = Color::from_rgb_u8(0x7C, 0xB7, 0xFF);
// One color for each part of the song, in the order of its parts.
const PART_COLORS: [Color; 4] = [
    Color::from_rgb_u8(0x6C, 0xC4, 0xFF),
    Color::from_rgb_u8(0xFF, 0xC8, 0x57),
    Color::from_rgb_u8(0x5E, 0xE0, 0xA8),
    Color::from_rgb_u8(0xF7, 0x8F, 0xC6),
];
const KEYBOARD: KeyboardColors = KeyboardColors {
    white: Color::from_rgb_u8(0xFB, 0xFC, 0xFF),
    white_edge: Color::from_rgb_u8(0xC9, 0xD3, 0xE6),
    black: Color::from_rgb_u8(0x0B, 0x10, 0x1F),
    black_edge: Color::from_rgb_u8(0x33, 0x3D, 0x57),
    hand: ACCENT,
    gap: Color::from_rgb_u8(0x05, 0x08, 0x12),
};

#[composable]
pub(crate) fn MusicTab() {
    let song = remember(clair_de_lune).with(Song::clone);
    let performance = rememberPerformance(song.seconds());
    let settings = rememberSettings();
    let focus = remember(FocusRequester::new).with(FocusRequester::clone);
    // The keyboard holds the focus, so the texts of the stage stay nodes of
    // their own for a screen reader.
    let takes_keys = Modifier::empty().focus_requester(&focus).focus_target();
    {
        LaunchedEffect((), async move |_| {
            if let Err(error) = focus.request_focus() {
                log::warn!("the music tab could not take the keys: {error:?}");
            }
        });
    }
    // The night keeps its colors in a light system theme too.
    LiquidTheme(
        LiquidThemeSpec::system_with_accent(SchemeMode::Dark, ACCENT),
        move || {
            let stage = performance.clone();
            Perform(performance, move || {
                let performance = stage.clone();
                let song = song.clone();
                let typist = stage.clone();
                let takes_keys = takes_keys.clone();
                CranposeBox(
                    Modifier::empty()
                        .fill_max_size()
                        .clip_to_bounds()
                        // The keys bubble up to here from the keyboard, which
                        // holds the focus, and from a control that took it.
                        .on_key_event(move |event| take_key(event, &typist, settings)),
                    BoxSpec::default(),
                    move || {
                        NightSky(Modifier::empty().fill_max_size());
                        Score(song.clone(), settings, takes_keys.clone());
                        ControlPanel(
                            song.clone(),
                            performance.clone(),
                            settings,
                            Modifier::empty().fill_max_width().padding(PANEL_MARGIN),
                        );
                    },
                );
            });
        },
    );
}

// Takes a key of a computer's keyboard: a key of the home row or of the row
// above it plays a note, the space bar starts and stops the piece, and the
// arrows move it by a few seconds. Returns whether the key was taken.
fn take_key(event: &KeyEvent, performance: &Performance, settings: Settings) -> bool {
    if event.is_ctrl_pressed() || event.is_alt_pressed() || event.is_meta_pressed() {
        return false;
    }
    let down = event.type_() == KeyEventType::KeyDown;
    if let Some(key) = piano_key(event.key()) {
        if down {
            let voicing = Voicing {
                timbre: settings.timbre.get(),
                ..Voicing::default()
            };
            performance.key_down(key, voicing);
        } else {
            performance.key_up(key);
        }
        return true;
    }
    let step = match event.key() {
        Key::Spacebar if down => {
            performance.toggle();
            return true;
        }
        Key::DirectionLeft if down => -ARROW_SECONDS,
        Key::DirectionRight if down => ARROW_SECONDS,
        _ => return false,
    };
    let length = f64::from(performance.player().length());
    performance.seek((performance.seconds() + step).clamp(0.0, length));
    true
}

// The roll of the song over the keyboard its notes fall onto. The parts are
// the content of the roll: it composes them in each of its seconds, and
// they compose their notes there.
#[composable]
fn Score(song: Song, settings: Settings, keyboard: Modifier) {
    Instrument(settings.timbre.get(), move || {
        let song = song.clone();
        let keyboard = keyboard.clone();
        Column(
            Modifier::empty().fill_max_size(),
            ColumnSpec::new(),
            move || {
                let song = song.clone();
                Roll(
                    KEYS,
                    Modifier::empty()
                        .fill_max_width()
                        .weight(1.0)
                        .test_tag(ROLL_TAG)
                        .content_description("Notes of the piece"),
                    move || Parts(song.clone(), settings),
                );
                Keyboard(
                    KEYS,
                    KEYBOARD,
                    keyboard
                        .clone()
                        .fill_max_width()
                        .height(KEYBOARD_HEIGHT)
                        .test_tag(KEYBOARD_TAG),
                );
            },
        );
    });
}

// The parts of the song that play, each at its pitch and volume.
#[composable]
fn Parts(song: Song, settings: Settings) {
    Transpose(settings.transpose.get(), move || {
        for (index, part) in song.parts.iter().enumerate() {
            if !settings.plays(index) {
                continue;
            }
            with_key(&index, || {
                Volume(settings.part_volume(index), {
                    let notes = part.notes.clone();
                    move || Part(notes.clone(), PART_COLORS[index % PART_COLORS.len()])
                });
            });
        }
    });
}
