// The panel of a performance, on glass over the roll: the transport, and
// under it what the listener sets.

use cranpose::liquid::prelude::*;
use cranpose_core::{derivedStateOf, with_key, SideEffect};
use cranpose_ui::{
    composable,
    text::FontWeight,
    widgets::{FlowRow, FlowSpec},
    Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Text, VerticalAlignment,
};
use cranpose_ui_graphics::whole;

use super::{
    clair_de_lune::{COMPOSER, CREDIT, TITLE},
    performance::Performance,
    settings::{Settings, FASTEST_TEMPO, SLOWEST_TEMPO, TRANSPOSE_LIMIT},
    song::{note_seconds, Song},
    timbre::Timbre,
};
use crate::app::demo_text::text_style;

const PANEL_CORNER: f32 = 26.0;
const PANEL_PADDING: f32 = 16.0;
const ROW_SPACING: f32 = 12.0;
const GROUP_SPACING: f32 = 22.0;
const SLIDER_WIDTH: f32 = 170.0;
const TIMBRES_WIDTH: f32 = 330.0;
const ICON_SIZE: f32 = 18.0;
// The position slider moves in this many steps, so it composes again a few
// times a second and not each frame.
const SEEK_STEPS: f64 = 400.0;
const PLAY_ICON: &str = "M8 5v14l11-7z";
const PAUSE_ICON: &str = "M6 19h4V5H6v14zm8-14v14h4V5h-4z";
const RESTART_ICON: &str = "M6 6h2v12H6zm3.5 6l8.5 6V6z";

fn clock(seconds: i32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[composable]
pub(super) fn ControlPanel(
    song: Song,
    performance: Performance,
    settings: Settings,
    modifier: Modifier,
) {
    let tempo = settings.tempo.get();
    let volume = settings.volume.get();
    let player = performance.player().clone();
    SideEffect(move || {
        player.set_tempo(tempo);
        player.set_volume(volume);
    });
    let shown = settings.shown.get();

    GlassSurface(
        modifier,
        Glass::regular().shape(LiquidShape::RoundedRect(PANEL_CORNER)),
        move || {
            let song = song.clone();
            let performance = performance.clone();
            Column(
                Modifier::empty().fill_max_width().padding(PANEL_PADDING),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(ROW_SPACING)),
                move || {
                    Heading(settings, shown);
                    Transport(performance.clone(), song.seconds());
                    if shown {
                        Sound(song.clone(), settings);
                    }
                },
            );
        },
    );
}

#[composable]
fn Heading(settings: Settings, shown: bool) {
    let colors = liquid_colors();
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::new(),
                move || {
                    Text(
                        format!("{TITLE}, {COMPOSER}"),
                        Modifier::empty(),
                        text_style(20.0, colors.label, Some(FontWeight::BOLD)),
                    );
                    Text(
                        CREDIT,
                        Modifier::empty(),
                        text_style(11.0, colors.secondary_label, None),
                    );
                },
            );
            LiquidChip(
                Modifier::empty(),
                shown,
                move || settings.shown.set(!shown),
                "Sound".into(),
            );
        },
    );
}

#[composable]
fn Transport(performance: Performance, length: f32) {
    let colors = liquid_colors();
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
            IconButton(
                if playing { PAUSE_ICON } else { PLAY_ICON },
                if playing { "Pause" } else { "Play" },
                GlassButtonSpec::prominent(),
                move || toggle.toggle(),
            );
            let restart = performance.clone();
            IconButton(
                RESTART_ICON,
                "Restart",
                GlassButtonSpec::glass(),
                move || {
                    restart.seek(0.0);
                },
            );
            Text(
                format!("{} / {}", clock(shown.get()), clock(whole::to_i32(length))),
                Modifier::empty(),
                text_style(14.0, colors.label, Some(FontWeight::MEDIUM)),
            );
            let seek = performance.clone();
            LiquidSlider(
                Modifier::empty()
                    .weight(1.0)
                    .content_description("Position in the song"),
                share.get(),
                move |share| seek.seek(f64::from(share * length)),
            );
        },
    );
}

// A glass button with an icon before its label.
#[composable]
fn IconButton(
    icon: &'static str,
    label: &'static str,
    spec: GlassButtonSpec,
    on_click: impl Fn() + 'static,
) {
    let colors = liquid_colors();
    let ink = spec.icon_color(&colors);
    GlassButton(Modifier::empty(), spec, on_click, move || {
        Row(
            Modifier::empty(),
            RowSpec::new()
                .horizontal_arrangement(LinearArrangement::spaced_by(6.0))
                .vertical_alignment(VerticalAlignment::CenterVertically),
            move || {
                icons::Icon(icon, None, ICON_SIZE, ink);
                Text(
                    label,
                    Modifier::empty(),
                    text_style(14.0, ink, Some(FontWeight::MEDIUM)),
                );
            },
        );
    });
}

// What the listener sets: tempo, volume, the balance of the hands, the
// instrument, the pitch, and which parts play.
#[composable]
fn Sound(song: Song, settings: Settings) {
    let tempo = settings.tempo.get();
    let volume = settings.volume.get();
    let semitones = settings.transpose.get();
    FlowRow(
        Modifier::empty().fill_max_width(),
        FlowSpec::default()
            .main_axis_spacing(GROUP_SPACING)
            .cross_axis_spacing(ROW_SPACING),
        move || {
            LabelledSlider(
                format!("Tempo {tempo:.2}x"),
                "Tempo",
                (tempo - SLOWEST_TEMPO) / (FASTEST_TEMPO - SLOWEST_TEMPO),
                move |share| {
                    settings
                        .tempo
                        .set(SLOWEST_TEMPO + share * (FASTEST_TEMPO - SLOWEST_TEMPO));
                },
            );
            LabelledSlider(
                format!("Volume {:.0}%", volume * 100.0),
                "Volume",
                volume,
                move |share| settings.volume.set(share),
            );
            LabelledSlider(
                "Balance of the hands".to_string(),
                "Balance of the hands",
                settings.balance.get(),
                move |share| settings.balance.set(share),
            );
            TimbrePicker(settings);
            Labelled(format!("Transpose {semitones:+}"), move || {
                let transpose = settings.transpose;
                LiquidActionChip(
                    Modifier::empty(),
                    false,
                    move || transpose.set((transpose.get() - 1).max(-TRANSPOSE_LIMIT)),
                    "Lower".into(),
                );
                LiquidActionChip(
                    Modifier::empty(),
                    false,
                    move || transpose.set((transpose.get() + 1).min(TRANSPOSE_LIMIT)),
                    "Higher".into(),
                );
            });
            let song = song.clone();
            Labelled("Parts".to_string(), move || {
                for (index, part) in song.parts.iter().enumerate() {
                    let bit = 1u32 << index;
                    let silenced = settings.silenced;
                    with_key(&index, || {
                        LiquidChip(
                            Modifier::empty(),
                            settings.plays(index),
                            move || silenced.set(silenced.get() ^ bit),
                            part.name.clone().into(),
                        );
                    });
                }
            });
        },
    );
}

// A slider over 0 to 1 under a small label.
#[composable]
fn LabelledSlider(
    label: String,
    description: &'static str,
    share: f32,
    on_change: impl Fn(f32) + 'static,
) {
    let on_change = std::rc::Rc::new(on_change);
    Labelled(label, move || {
        let on_change = std::rc::Rc::clone(&on_change);
        LiquidSlider(
            Modifier::empty()
                .width(SLIDER_WIDTH)
                .content_description(description),
            share,
            move |share| on_change(share),
        );
    });
}

#[composable]
fn TimbrePicker(settings: Settings) {
    let timbre = settings.timbre.get();
    Labelled("Instrument".to_string(), move || {
        LiquidSegmentedControl(
            Modifier::empty().width(TIMBRES_WIDTH),
            Timbre::ALL
                .iter()
                .position(|choice| *choice == timbre)
                .unwrap_or(0),
            move |index| {
                if let Some(choice) = Timbre::ALL.get(index) {
                    settings.timbre.set(*choice);
                }
            },
            |scope| {
                for choice in Timbre::ALL {
                    scope.segment(choice.label());
                }
            },
        );
    });
}

// A row of controls under a small label.
#[composable]
fn Labelled(label: String, controls: impl FnMut() + 'static) {
    let colors = liquid_colors();
    let controls = std::rc::Rc::new(std::cell::RefCell::new(controls));
    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            Text(
                label.clone(),
                Modifier::empty(),
                text_style(12.0, colors.secondary_label, None),
            );
            let controls = std::rc::Rc::clone(&controls);
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || (controls.borrow_mut())(),
            );
        },
    );
}
