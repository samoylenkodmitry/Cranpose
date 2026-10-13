use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
};

use cranpose::{liquid::prelude::*, prelude::*};

use crate::{Controls, MUTED, SceneSliders, WALK_HINT, text_style};

pub enum HudCommand {
    BackToStart,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Stats {
    pub fps: u32,
    pub altitude: f32,
    pub stamina: f32,
    pub size: (u32, u32),
}

#[derive(Clone, Copy)]
pub struct HudStates {
    pub controls: MutableState<Controls>,
    pub stats: MutableState<Stats>,
}

#[derive(Clone)]
pub struct HudBridge {
    pub engine: &'static str,
    pub states: Rc<OnceCell<HudStates>>,
    pub commands: Rc<RefCell<Vec<HudCommand>>>,
}

impl HudBridge {
    pub fn new(engine: &'static str) -> Self {
        Self {
            engine,
            states: Rc::default(),
            commands: Rc::default(),
        }
    }
}

impl PartialEq for HudBridge {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.states, &other.states) && Rc::ptr_eq(&self.commands, &other.commands)
    }
}

const PANEL: Color = Color::from_rgba_u8(14, 18, 30, 170);
fn panel(modifier: Modifier) -> Modifier {
    modifier
        .shadow_with(
            14.0,
            LayerShape::Rounded(RoundedCornerShape::uniform(20.0)),
            true,
            Color::BLACK,
            Color::BLACK,
        )
        .background(PANEL)
        .rounded_corners(20.0)
}

#[composable]
pub fn Hud(bridge: HudBridge) {
    let controls = rememberMutableStateOf(Controls::default);
    let stats = rememberMutableStateOf(Stats::default);
    bridge.states.get_or_init(|| HudStates { controls, stats });
    LiquidTheme(crate::dark_theme(), move || {
        let bridge = bridge.clone();
        Box(
            Modifier::empty().fill_max_size().padding(20.0),
            BoxSpec::default(),
            move || {
                StatusPanel(bridge.engine, stats);
                FrameChip(stats);
                SettingsPanel(controls);
                ActionBar(bridge.clone());
            },
        );
    });
}

#[composable]
fn StatusPanel(engine: &'static str, stats: MutableState<Stats>) {
    Column(
        panel(Modifier::empty().align(Alignment::TOP_START).width(300.0)).padding(18.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            Text(
                "Cranpose HUD",
                Modifier::empty(),
                text_style(20.0, Color::WHITE),
            );
            Text(
                format!("Drawn by Cranpose on {engine}'s GPU device, over {engine}'s 3D frame."),
                Modifier::empty(),
                text_style(12.0, MUTED),
            );
            Meter(
                "Stamina",
                stats.value().stamina,
                [
                    Color::from_rgb_u8(255, 94, 98),
                    Color::from_rgb_u8(255, 196, 92),
                ],
            );
            Text(
                format!("Altitude {:.0} m", stats.value().altitude),
                Modifier::empty(),
                text_style(13.0, MUTED),
            );
        },
    );
}

#[composable]
fn Meter(label: &'static str, fraction: f32, colors: [Color; 2]) {
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
        move || {
            Text(label, Modifier::empty(), text_style(13.0, Color::WHITE));
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(10.0)
                    .draw_behind(move |scope| {
                        let size = scope.size();
                        let radii = CornerRadii::uniform(size.height / 2.0);
                        scope.draw_round_rect(
                            Brush::solid(Color::from_rgba_u8(255, 255, 255, 40)),
                            radii,
                        );
                        scope.draw_round_rect_at(
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width: size.width * fraction.clamp(0.02, 1.0),
                                height: size.height,
                            },
                            Brush::horizontal_gradient(colors.to_vec(), 0.0, size.width),
                            radii,
                        );
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn FrameChip(stats: MutableState<Stats>) {
    Row(
        panel(Modifier::empty().align(Alignment::TOP_END)).padding_symmetric(16.0, 10.0),
        RowSpec::default().horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            let stats = stats.value();
            Text(
                format!("{} fps", stats.fps),
                Modifier::empty(),
                text_style(16.0, Color::from_rgb_u8(150, 230, 170)),
            );
            Text(
                format!("{} x {}", stats.size.0, stats.size.1),
                Modifier::empty(),
                text_style(16.0, MUTED),
            );
        },
    );
}

#[composable]
fn SettingsPanel(controls: MutableState<Controls>) {
    Column(
        panel(Modifier::empty().align(Alignment::CENTER_END).width(280.0)).padding(18.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            Text("Scene", Modifier::empty(), text_style(18.0, Color::WHITE));
            SceneSliders(controls);
        },
    );
}

#[composable]
fn ActionBar(bridge: HudBridge) {
    Row(
        panel(Modifier::empty().align(Alignment::BOTTOM_CENTER)).padding_symmetric(16.0, 12.0),
        RowSpec::default()
            .horizontal_arrangement(LinearArrangement::SpacedBy(14.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let commands = Rc::clone(&bridge.commands);
            GlassButton(
                Modifier::empty(),
                GlassButtonSpec::glass(),
                move || commands.borrow_mut().push(HudCommand::BackToStart),
                || {
                    Text(
                        "Back to start",
                        Modifier::empty(),
                        text_style(15.0, Color::WHITE),
                    );
                },
            );
            Text(WALK_HINT, Modifier::empty(), text_style(14.0, MUTED));
        },
    );
}
