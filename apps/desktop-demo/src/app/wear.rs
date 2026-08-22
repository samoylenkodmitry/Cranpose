//! A round-watch settings screen, built from the Wear widget set.
//!
//! This replaces a 2675-line vendored copy of an app's own list, draw, font,
//! layout and theme stack that used to live here as a frame-pacing fixture. The
//! copy had drifted: it and the app it came from agreed with each other and
//! disagreed with this framework's own golden-tested scroll-indicator arc by
//! 3.90 degrees of sweep. What is left is the framework's widgets drawing the
//! same screen.
//!
//! It keeps the fixture's purpose. The list still scrolls on its own, still by
//! elapsed time rather than per frame — anything paced per frame travels faster
//! the faster the app runs, which would make a frame-rate reading partly a
//! measurement of itself — and it is still the heaviest text page a watch app
//! draws. What has changed is the shape of the load: this is a widget tree with
//! a graphics layer per row, where the old one was a single `Canvas` redrawing
//! several hundred glyphs a frame. Comparing the two under the demo's pacing
//! modes is the whole point of keeping it.

use cranpose::LazyItems;
use cranpose_core::{remember, LaunchedEffectAsync};
use cranpose_ui::round_scaling_list::CentreAnchor;
use cranpose_ui::widgets::wear::{
    rememberWearScalingListState, ListHeader, ListHeaderSpec, ScreenScaffold, ScreenScaffoldSpec,
    ScrollIndicatorSpec, SwitchButton, SwitchButtonSpec, WearButton, WearButtonSpec, WearColors,
    WearScalingLazyColumn, WearScalingLazyColumnSpec, WearScalingListState, WearTextStyle,
};
use cranpose_ui::{Alignment, Box as CranposeBox, BoxSpec, Color, Modifier, Size, Text};
use std::cell::RefCell;
use std::rc::Rc;

/// The display the screen is laid out for, in layout points.
const WATCH: f32 = 454.0;
/// The Settings screen's own side padding.
const SIDE: f32 = 18.0;
/// Both Wear screens' vertical content padding.
const VERTICAL: f32 = 34.0;
/// Points per second the list travels, so the workload is identical at every
/// pacing mode.
const POINTS_PER_SECOND: f32 = 260.0;

/// The palette the widget geometry was measured against.
fn colors() -> WearColors {
    WearColors {
        primary: Color::from_rgb_u8(0xB9, 0xF2, 0xFF),
        primary_container: Color::from_rgb_u8(0x0F, 0x36, 0x4E),
        on_primary: Color::from_rgb_u8(0x00, 0x00, 0x00),
        on_primary_container: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
        surface_container: Color::from_rgb_u8(0x0A, 0x16, 0x22),
        on_surface: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
        on_surface_variant: Color::from_rgb_u8(0x5E, 0x7E, 0x93),
        outline: Color::from_rgb_u8(0x1D, 0x4D, 0x69),
        background: Color::from_rgb_u8(0x00, 0x00, 0x00),
        on_background: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
        content: Color::WHITE,
        indicator_thumb: Color::from_rgb_u8(0xB4, 0xCA, 0xD3),
        indicator_track: Color::from_rgb_u8(0x1E, 0x33, 0x3A),
    }
}

#[derive(Clone)]
enum Row {
    Header(&'static str),
    Switch {
        label: &'static str,
        index: usize,
    },
    Button {
        label: &'static str,
        secondary: Option<&'static str>,
    },
    Body(&'static str),
}

impl Row {
    /// Which rows may reuse each other's composition slots. A header reused as
    /// a switch throws away the whole subtree, so the four shapes are pooled
    /// apart.
    fn content_type(&self) -> u64 {
        match self {
            Row::Header(_) => 0,
            Row::Switch { .. } => 1,
            Row::Button { .. } => 2,
            Row::Body(_) => 3,
        }
    }
}

fn rows() -> Vec<Row> {
    vec![
        Row::Header("SETTINGS"),
        Row::Switch {
            label: "Haptics",
            index: 0,
        },
        Row::Switch {
            label: "Sound effects",
            index: 1,
        },
        Row::Switch {
            label: "Ambient loop",
            index: 2,
        },
        Row::Header("CROWN"),
        Row::Button {
            label: "Sensitivity",
            secondary: Some("NORMAL"),
        },
        Row::Switch {
            label: "Invert rotation",
            index: 3,
        },
        Row::Header("DISPLAY"),
        Row::Button {
            label: "Palette",
            secondary: Some("DEFAULT"),
        },
        Row::Switch {
            label: "Reduced motion",
            index: 4,
        },
        Row::Switch {
            label: "High contrast",
            index: 5,
        },
        Row::Header("MORE"),
        Row::Body("Every graphic and sound in this build was made for it, on a watch that fits in a pocket."),
        Row::Button {
            label: "Credits",
            secondary: None,
        },
        Row::Body("Version 1.0.0-debug"),
    ]
}

struct WearState {
    checked: [bool; 6],
    down: bool,
    last_nanos: u64,
}

impl WearState {
    fn new() -> Self {
        Self {
            checked: [true, true, false, false, true, false],
            down: true,
            last_nanos: 0,
        }
    }

    /// Walks the list up and down by elapsed time, reversing at both ends so a
    /// measurement window of any length stays in motion.
    fn advance(&mut self, now_nanos: u64, list: &WearScalingListState) -> bool {
        let last = std::mem::replace(&mut self.last_nanos, now_nanos);
        if last == 0 || now_nanos <= last {
            return false;
        }
        let seconds = ((now_nanos - last) as f32 / 1_000_000_000.0).min(0.25);
        let info = list.layout_info();
        let travel = info.travel();
        if travel <= 0.0 {
            return false;
        }
        let scrolled = info.scrolled();
        if self.down && scrolled >= travel {
            self.down = false;
        } else if !self.down && scrolled <= 0.0 {
            self.down = true;
        }
        let step = POINTS_PER_SECOND * seconds;
        list.scroll_by(if self.down { step } else { -step });
        true
    }
}

#[cranpose::composable]
pub fn wear_tab() {
    let state = remember(|| Rc::new(RefCell::new(WearState::new()))).with(|value| value.clone());
    let list = rememberWearScalingListState(CentreAnchor::default());

    let tick_state = state.clone();
    LaunchedEffectAsync!((), move |scope| {
        Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            loop {
                if !scope.is_active() {
                    break;
                }
                let now = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                tick_state.borrow_mut().advance(now, &list);
            }
        })
    });

    let toggle_state = state.clone();
    CranposeBox(
        Modifier::empty().fill_max_size(),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            let toggle_state = toggle_state.clone();
            ScreenScaffold(
                Modifier::empty()
                    .size(Size::new(WATCH, WATCH))
                    .background(colors().background),
                list,
                ScreenScaffoldSpec::default()
                    .indicator(ScrollIndicatorSpec::default().colors(colors())),
                move || {
                    let toggle_state = toggle_state.clone();
                    WearScalingLazyColumn(
                        Modifier::empty().fill_max_size(),
                        list,
                        WearScalingLazyColumnSpec::default().content_padding(SIDE, VERTICAL),
                        move |scope| {
                            let rows = rows();
                            let toggle_state = toggle_state.clone();
                            let types = rows.iter().map(Row::content_type).collect::<Vec<_>>();
                            scope.items(
                                LazyItems::new(rows.len())
                                    .key(|index: usize| index as u64)
                                    .content_type(move |index: usize| types[index]),
                                move |index| {
                                    let row = rows[index].clone();
                                    let toggle_state = toggle_state.clone();
                                    match row {
                                        Row::Header(label) => {
                                            ListHeader(
                                                Modifier::empty(),
                                                ListHeaderSpec::default().colors(colors()),
                                                label.to_string(),
                                            );
                                        }
                                        Row::Switch { label, index } => {
                                            let checked = toggle_state.borrow().checked[index];
                                            let sink = toggle_state.clone();
                                            SwitchButton(
                                                Modifier::empty().fill_max_width(),
                                                SwitchButtonSpec::default()
                                                    .colors(colors())
                                                    .progress(if checked { 1.0 } else { 0.0 }),
                                                checked,
                                                label.to_string(),
                                                None,
                                                move |next| {
                                                    sink.borrow_mut().checked[index] = next;
                                                },
                                            );
                                        }
                                        Row::Button { label, secondary } => {
                                            WearButton(
                                                Modifier::empty().fill_max_width(),
                                                WearButtonSpec::default().colors(colors()),
                                                label.to_string(),
                                                secondary.map(str::to_string),
                                                || {},
                                            );
                                        }
                                        Row::Body(text) => {
                                            Text(
                                                text.to_string(),
                                                Modifier::empty().fill_max_width(),
                                                WearTextStyle::BODY_LARGE
                                                    .at_size(12.0)
                                                    .with_line_height(16.0)
                                                    .resolve(colors().content),
                                            );
                                        }
                                    }
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}
