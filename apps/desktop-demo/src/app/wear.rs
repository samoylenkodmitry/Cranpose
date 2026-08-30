use std::{cell::RefCell, rc::Rc};

use cranpose::LazyItems;
use cranpose_core::{remember, LaunchedEffectAsync};
use cranpose_ui::{
    round_scaling_list::CentreAnchor,
    widgets::wear::{
        rememberWearScalingListState, ListHeader, ListHeaderSpec, ScreenScaffold,
        ScreenScaffoldSpec, ScrollIndicatorSpec, SwitchButton, SwitchButtonSpec, WearButton,
        WearButtonSpec, WearColors, WearScalingLazyColumn, WearScalingLazyColumnSpec,
        WearScalingListState, WearTextStyle,
    },
    Alignment, Box as CranposeBox, BoxSpec, Color, Modifier, Size, Text,
};

const WATCH: f32 = 454.0;
const SIDE: f32 = 18.0;
const VERTICAL: f32 = 34.0;
const POINTS_PER_SECOND: f32 = 260.0;

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
    LaunchedEffectAsync((), move |scope| {
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
