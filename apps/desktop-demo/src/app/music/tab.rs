// The Music tab: "Clair de lune" as a tree of composables. Each hand is a
// `Part` that draws its notes and sounds them; `Instrument`, `Transpose` and
// `Volume` wrap the parts and change how they sound; the `Keyboard` lights
// the keys the parts hold and plays under a finger.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{
    derivedStateOf, remember, rememberMutableStateOf, with_key, MutableState, SideEffect,
};
use cranpose_ui::{
    composable,
    text::FontWeight,
    widgets::{FlowRow, FlowSpec, Slider, SliderSpec},
    Box as CranposeBox, BoxSpec, Brush, Canvas, Color, Column, ColumnSpec, CornerRadii,
    LinearArrangement, Modifier, Point, Rect, Row, RowSpec, Text, VerticalAlignment,
};
use cranpose_ui_graphics::whole;

use super::{
    clair_de_lune::{clair_de_lune, COMPOSER, CREDIT, TITLE},
    key_range::KeyRange,
    keyboard::{Keyboard, KeyboardColors},
    part::Part,
    performance::{rememberPerformance, Perform, Performance},
    song::{note_seconds, Song},
    timbre::Timbre,
    voicing::{Instrument, Transpose, Volume},
};
use crate::app::{demo_text::text_style, demo_theme::DemoPalette};

const KEYS: KeyRange = KeyRange::PIANO;
const KEYBOARD_TAG: &str = "MusicKeyboard";
const PAGE_PADDING: f32 = 16.0;
const ROW_SPACING: f32 = 12.0;
const CHIP_SPACING: f32 = 8.0;
const KEYBOARD_HEIGHT: f32 = 96.0;
const FADER_WIDTH: f32 = 200.0;
const FADER_HEIGHT: f32 = 28.0;
const FADER_THUMB: f32 = 16.0;
const FADER_TRACK: f32 = 4.0;
const SLOWEST_TEMPO: f32 = 0.5;
const FASTEST_TEMPO: f32 = 2.0;
const START_VOLUME: f32 = 0.8;
// The seek bar moves in this many steps, so it composes again a few times a
// second and not each frame.
const SEEK_STEPS: f64 = 400.0;
const TRANSPOSE_LIMIT: i8 = 12;
const ACCENT: Color = Color::from_rgb_u8(0x38, 0xBD, 0xF8);
const ACCENT_INK: Color = Color::from_rgb_u8(0x08, 0x2F, 0x49);
// One color for each part of the song, in the order of its parts.
const PART_COLORS: [Color; 4] = [
    Color::from_rgb_u8(0x60, 0xA5, 0xFA),
    Color::from_rgb_u8(0xFB, 0xBF, 0x24),
    Color::from_rgb_u8(0x34, 0xD3, 0x99),
    Color::from_rgb_u8(0xF4, 0x72, 0xB6),
];
const ROLL: Color = Color::from_rgb_u8(0x0B, 0x10, 0x20);
const WHITE_KEY: Color = Color::from_rgb_u8(0xF8, 0xFA, 0xFC);
const BLACK_KEY: Color = Color::from_rgb_u8(0x1E, 0x29, 0x3B);

fn clock(seconds: i32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

// The volume of a part under a balance from 0, the first part alone, to 1,
// the second part alone. Both play in full at the middle.
fn part_volume(index: usize, balance: f32) -> f32 {
    match index {
        0 => (2.0 * (1.0 - balance)).min(1.0),
        1 => (2.0 * balance).min(1.0),
        _ => 1.0,
    }
}

#[composable]
pub(crate) fn MusicTab() {
    let palette = DemoPalette::current();
    let song = remember(clair_de_lune).with(Song::clone);
    let performance = rememberPerformance(song.seconds());
    let tempo = rememberMutableStateOf(|| 1.0f32);
    let volume = rememberMutableStateOf(|| START_VOLUME);
    let timbre = rememberMutableStateOf(|| Timbre::Piano);
    let transpose = rememberMutableStateOf(|| 0i8);
    // One bit for each part of the song the listener turned off.
    let silenced = rememberMutableStateOf(|| 0u32);
    let balance = rememberMutableStateOf(|| 0.5f32);

    let stage = performance.clone();
    Perform(performance, move || {
        let performance = stage.clone();
        let song = song.clone();
        Column(
            Modifier::empty()
                .fill_max_size()
                .background(palette.background)
                .padding(PAGE_PADDING),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(ROW_SPACING)),
            move || {
                Heading(palette);
                TransportRow(performance.clone(), song.seconds(), palette);
                FlowRow(
                    Modifier::empty().fill_max_width(),
                    FlowSpec::default()
                        .main_axis_spacing(ROW_SPACING * 2.0)
                        .cross_axis_spacing(ROW_SPACING),
                    {
                        let performance = performance.clone();
                        let song = song.clone();
                        move || {
                            Faders(performance.clone(), tempo, volume, palette);
                            BalanceFader(balance, palette);
                            TimbreChips(timbre, palette);
                            TransposeChips(transpose, palette);
                            PartChips(song.clone(), silenced, palette);
                        }
                    },
                );
                Score(
                    song.clone(),
                    timbre.get(),
                    transpose.get(),
                    silenced.get(),
                    balance.get(),
                    Modifier::empty().fill_max_width().weight(1.0),
                );
            },
        );
    });
}

#[composable]
fn Heading(palette: DemoPalette) {
    Column(Modifier::empty(), ColumnSpec::new(), move || {
        Text(
            format!("{TITLE}, {COMPOSER}"),
            Modifier::empty(),
            text_style(22.0, palette.text, Some(FontWeight::BOLD)),
        );
        Text(
            CREDIT,
            Modifier::empty(),
            text_style(12.0, palette.muted, None),
        );
    });
}

#[composable]
fn Chip(label: String, selected: bool, palette: DemoPalette, on_click: impl Fn() + 'static) {
    let (fill, ink) = if selected {
        (ACCENT, ACCENT_INK)
    } else {
        (palette.track, palette.text)
    };
    CranposeBox(
        Modifier::empty()
            .background(fill)
            .rounded_corners(16.0)
            .clickable(move |_| on_click())
            .padding_symmetric(14.0, 8.0),
        BoxSpec::default(),
        move || {
            Text(
                label.clone(),
                Modifier::empty(),
                text_style(14.0, ink, Some(FontWeight::MEDIUM)),
            );
        },
    );
}

#[composable]
fn ChipGroup(label: String, palette: DemoPalette, chips: impl FnMut() + 'static) {
    let chips = Rc::new(RefCell::new(chips));
    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            Text(
                label.clone(),
                Modifier::empty(),
                text_style(12.0, palette.muted, None),
            );
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::spaced_by(CHIP_SPACING))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                {
                    let chips = Rc::clone(&chips);
                    move || (chips.borrow_mut())()
                },
            );
        },
    );
}

// A slider over 0 to 1 drawn as a track with a round thumb.
#[composable]
fn Fader(
    share: f32,
    description: String,
    palette: DemoPalette,
    modifier: Modifier,
    on_change: impl Fn(f32) + 'static,
) {
    Slider(
        modifier
            .height(FADER_HEIGHT)
            .content_description(description),
        share,
        on_change,
        || {},
        SliderSpec::new().thumb_extent(FADER_THUMB),
        move |slider| {
            let thumb = slider.thumb_offset() + FADER_THUMB * 0.5;
            Canvas(Modifier::empty().fill_max_size(), move |scope| {
                let size = scope.size();
                let middle = size.height * 0.5;
                let track = |width: f32| Rect {
                    x: 0.0,
                    y: middle - FADER_TRACK * 0.5,
                    width,
                    height: FADER_TRACK,
                };
                let round = CornerRadii::uniform(FADER_TRACK * 0.5);
                scope.draw_round_rect_at(track(size.width), Brush::solid(palette.track), round);
                scope.draw_round_rect_at(track(thumb), Brush::solid(ACCENT), round);
                scope.draw_circle(
                    Brush::solid(ACCENT),
                    Point {
                        x: thumb,
                        y: middle,
                    },
                    FADER_THUMB * 0.5,
                );
            });
        },
    );
}

#[composable]
fn TransportRow(performance: Performance, length: f32, palette: DemoPalette) {
    let playing = performance.is_playing();
    let clock_source = performance.clone();
    let shown = derivedStateOf(move || whole::to_i32(note_seconds(clock_source.seconds())));
    let seek_source = performance.clone();
    let share = derivedStateOf(move || {
        let share = seek_source.seconds() / f64::from(length.max(1.0));
        note_seconds((share * SEEK_STEPS).round() / SEEK_STEPS)
    });
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(ROW_SPACING))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let toggle = performance.clone();
            Chip(
                if playing { "Pause" } else { "Play" }.to_string(),
                true,
                palette,
                move || {
                    if toggle.is_playing() {
                        toggle.pause();
                    } else {
                        if toggle.seconds() >= f64::from(length) {
                            toggle.seek(0.0);
                        }
                        toggle.play();
                    }
                },
            );
            let restart = performance.clone();
            Chip("Restart".to_string(), false, palette, move || {
                restart.seek(0.0);
            });
            Text(
                format!("{} / {}", clock(shown.get()), clock(whole::to_i32(length))),
                Modifier::empty(),
                text_style(14.0, palette.text, None),
            );
            let seek = performance.clone();
            Fader(
                share.get(),
                "Position in the song".to_string(),
                palette,
                Modifier::empty().weight(1.0),
                move |share| seek.seek(f64::from(share * length)),
            );
        },
    );
}

#[composable]
fn Faders(
    performance: Performance,
    tempo: MutableState<f32>,
    volume: MutableState<f32>,
    palette: DemoPalette,
) {
    let tempo_now = tempo.get();
    let volume_now = volume.get();
    let player = performance.player().clone();
    SideEffect(move || {
        player.set_tempo(tempo_now);
        player.set_volume(volume_now);
    });
    ChipGroup(format!("Tempo {tempo_now:.2}x"), palette, move || {
        Fader(
            (tempo_now - SLOWEST_TEMPO) / (FASTEST_TEMPO - SLOWEST_TEMPO),
            "Tempo".to_string(),
            palette,
            Modifier::empty().width(FADER_WIDTH),
            move |share| tempo.set(SLOWEST_TEMPO + share * (FASTEST_TEMPO - SLOWEST_TEMPO)),
        );
    });
    ChipGroup(
        format!("Volume {:.0}%", volume_now * 100.0),
        palette,
        move || {
            Fader(
                volume_now,
                "Volume".to_string(),
                palette,
                Modifier::empty().width(FADER_WIDTH),
                move |share| volume.set(share),
            );
        },
    );
}

#[composable]
fn BalanceFader(balance: MutableState<f32>, palette: DemoPalette) {
    let share = balance.get();
    ChipGroup("Balance of the hands".to_string(), palette, move || {
        Fader(
            share,
            "Balance of the hands".to_string(),
            palette,
            Modifier::empty().width(FADER_WIDTH),
            move |share| balance.set(share),
        );
    });
}

#[composable]
fn TimbreChips(timbre: MutableState<Timbre>, palette: DemoPalette) {
    let chosen = timbre.get();
    ChipGroup("Instrument".to_string(), palette, move || {
        for choice in Timbre::ALL {
            with_key(&choice, || {
                Chip(
                    choice.label().to_string(),
                    choice == chosen,
                    palette,
                    move || {
                        timbre.set(choice);
                    },
                );
            });
        }
    });
}

#[composable]
fn TransposeChips(transpose: MutableState<i8>, palette: DemoPalette) {
    let semitones = transpose.get();
    ChipGroup(format!("Transpose {semitones:+}"), palette, move || {
        Chip("Lower".to_string(), false, palette, move || {
            transpose.set((transpose.get() - 1).max(-TRANSPOSE_LIMIT));
        });
        Chip("Higher".to_string(), false, palette, move || {
            transpose.set((transpose.get() + 1).min(TRANSPOSE_LIMIT));
        });
    });
}

#[composable]
fn PartChips(song: Song, silenced: MutableState<u32>, palette: DemoPalette) {
    let off = silenced.get();
    ChipGroup("Parts".to_string(), palette, move || {
        for (index, part) in song.parts.iter().enumerate() {
            let bit = 1u32 << index;
            with_key(&index, || {
                Chip(part.name.clone(), off & bit == 0, palette, move || {
                    silenced.set(silenced.get() ^ bit);
                });
            });
        }
    });
}

// The roll of the parts over the keyboard they fall onto.
#[composable]
fn Score(
    song: Song,
    timbre: Timbre,
    transpose: i8,
    silenced: u32,
    balance: f32,
    modifier: Modifier,
) {
    Instrument(timbre, move || {
        let song = song.clone();
        Column(modifier.clone(), ColumnSpec::new(), move || {
            let song = song.clone();
            CranposeBox(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .background(ROLL),
                BoxSpec::default(),
                move || {
                    let song = song.clone();
                    Transpose(transpose, move || {
                        for (index, part) in song.parts.iter().enumerate() {
                            if silenced & (1 << index) != 0 {
                                continue;
                            }
                            with_key(&index, || {
                                Volume(part_volume(index, balance), {
                                    let notes = part.notes.clone();
                                    let name = part.name.clone();
                                    move || {
                                        Part(
                                            notes.clone(),
                                            KEYS,
                                            PART_COLORS[index % PART_COLORS.len()],
                                            Modifier::empty()
                                                .fill_max_size()
                                                .content_description(format!("Notes of {name}")),
                                        );
                                    }
                                });
                            });
                        }
                    });
                },
            );
            Keyboard(
                KEYS,
                KeyboardColors {
                    white: WHITE_KEY,
                    black: BLACK_KEY,
                    lit: ACCENT,
                    gap: ROLL,
                },
                Modifier::empty()
                    .fill_max_width()
                    .height(KEYBOARD_HEIGHT)
                    .test_tag(KEYBOARD_TAG)
                    .content_description("Piano keyboard"),
            );
        });
    });
}
