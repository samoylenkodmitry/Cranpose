#![allow(non_snake_case)]

use std::{
    f32::consts::{PI, TAU},
    sync::{Arc, OnceLock},
};

use cranpose_animation::{animateFloatAsState, spring, Animatable, AnimationType, Spring};
use cranpose_core::{
    key, rememberMutableStateOf, with_current_composer, MutableState, Owned, State,
};
use cranpose_foundation::SemanticsWidgetRole;
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    Box, BoxSpec, Brush, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer, LinearArrangement,
    Modifier, Point, PointerEventKind, PointerInputScope, Row, RowSpec, SemanticsConfiguration,
    Size, Text, TextStyle, VerticalAlignment,
};
use cranpose_ui_graphics::{
    CompositingStrategy, RenderEffect, RuntimeShader, RUNTIME_SHADER_PRELUDE_WGSL,
};

const STAGE_HEIGHT: f32 = 208.0;
const CARD_RADIUS: f32 = 22.0;
const GRID_SPACING: f32 = 16.0;
const FOOTER_HORIZONTAL_PADDING: f32 = 18.0;
const FOOTER_VERTICAL_PADDING: f32 = 13.0;

const CAMERA_YAW: f32 = 0.46;
const CAMERA_PITCH: f32 = 0.60;
const CAMERA_DISTANCE: f32 = 4.05;
const CAMERA_FOCAL: f32 = 2.62;
const CAMERA_TARGET_Y: f32 = 0.19;

const SLIDER_TRAVEL: f32 = 0.66;
const SLIDER_LIFT: f32 = 0.148;
const DIAL_PIVOT_Y: f32 = 0.33;
const DIAL_SWEEP: f32 = 4.72;
const DIAL_DEAD_ZONE_PX: f32 = 14.0;
const TILT_YAW: f32 = 0.17;
const TILT_PITCH: f32 = 0.11;
const STAGE_DESIGN_ASPECT: f32 = 1.78;
const WIDE_GRID_MIN_WIDTH: f32 = 900.0;
const MEDIUM_GRID_MIN_WIDTH: f32 = 620.0;

static CONTROL_KINDS: [ControlKind; 6] = [
    ControlKind::Checkmark,
    ControlKind::Slider,
    ControlKind::Toggle,
    ControlKind::PushButton,
    ControlKind::VolumeDial,
    ControlKind::LeverSwitch,
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum ControlKind {
    Checkmark,
    Slider,
    Toggle,
    PushButton,
    VolumeDial,
    LeverSwitch,
}

impl ControlKind {
    fn title(self) -> &'static str {
        match self {
            ControlKind::Checkmark => "Checkmark",
            ControlKind::Slider => "Slider",
            ControlKind::Toggle => "Toggle",
            ControlKind::PushButton => "Push button",
            ControlKind::VolumeDial => "Volume dial",
            ControlKind::LeverSwitch => "Lever switch",
        }
    }

    fn scene_index(self) -> f32 {
        match self {
            ControlKind::Checkmark => 0.0,
            ControlKind::Slider => 1.0,
            ControlKind::Toggle => 2.0,
            ControlKind::PushButton => 3.0,
            ControlKind::VolumeDial => 4.0,
            ControlKind::LeverSwitch => 5.0,
        }
    }

    fn is_switch(self) -> bool {
        matches!(
            self,
            ControlKind::Checkmark | ControlKind::Toggle | ControlKind::LeverSwitch
        )
    }

    /// What a screen reader announces this control as, mirroring Compose's
    /// `Role`.
    fn role(self) -> Option<SemanticsWidgetRole> {
        match self {
            ControlKind::Checkmark => Some(SemanticsWidgetRole::Checkbox),
            ControlKind::Toggle | ControlKind::LeverSwitch => Some(SemanticsWidgetRole::Switch),
            ControlKind::PushButton => Some(SemanticsWidgetRole::Button),
            ControlKind::Slider | ControlKind::VolumeDial => None,
        }
    }

    /// Whether a drag on the stage moves this control's value.
    fn is_continuous(self) -> bool {
        matches!(self, ControlKind::Slider | ControlKind::VolumeDial)
    }

    fn value_animation(self) -> AnimationType {
        if self.is_switch() {
            spring(Spring::DampingRatioMediumBouncy, Spring::StiffnessMedium)
        } else {
            spring(Spring::DampingRatioNoBouncy, Spring::StiffnessHigh)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct ControlState {
    value: f32,
    on: bool,
    presses: u32,
}

impl ControlState {
    fn initial(kind: ControlKind) -> Self {
        match kind {
            ControlKind::Checkmark => Self {
                value: 0.0,
                on: true,
                presses: 0,
            },
            ControlKind::Slider => Self {
                value: 0.35,
                on: false,
                presses: 0,
            },
            ControlKind::VolumeDial => Self {
                value: 0.40,
                on: false,
                presses: 0,
            },
            _ => Self {
                value: 0.0,
                on: false,
                presses: 0,
            },
        }
    }

    fn scene_value(self, kind: ControlKind) -> f32 {
        if kind.is_switch() {
            if self.on {
                1.0
            } else {
                0.0
            }
        } else {
            self.value
        }
    }

    fn toggled(self, on: bool) -> Self {
        Self { on, ..self }
    }

    fn pressed_once(self) -> Self {
        Self {
            presses: self.presses.saturating_add(1),
            ..self
        }
    }

    fn with_value(self, value: f32) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
            ..self
        }
    }

    fn label(self, kind: ControlKind) -> String {
        match kind {
            ControlKind::Checkmark => {
                if self.on {
                    "Checked".to_string()
                } else {
                    "Unchecked".to_string()
                }
            }
            ControlKind::Slider | ControlKind::VolumeDial => {
                format!("{} %", (self.value * 100.0).round() as i32)
            }
            ControlKind::PushButton => {
                if self.presses == 1 {
                    "1 press".to_string()
                } else {
                    format!("{} presses", self.presses)
                }
            }
            ControlKind::Toggle | ControlKind::LeverSwitch => {
                if self.on {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct DragTracking {
    active: bool,
    angle: f32,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let length = dot(a, a).sqrt().max(1e-6);
    [a[0] / length, a[1] / length, a[2] / length]
}

fn camera_eye() -> [f32; 3] {
    [
        CAMERA_YAW.sin() * CAMERA_PITCH.cos() * CAMERA_DISTANCE,
        CAMERA_TARGET_Y + CAMERA_PITCH.sin() * CAMERA_DISTANCE,
        CAMERA_YAW.cos() * CAMERA_PITCH.cos() * CAMERA_DISTANCE,
    ]
}

fn stage_unit(size: Size) -> f32 {
    size.height.min(size.width / STAGE_DESIGN_ASPECT)
}

fn grid_columns(width: f32) -> usize {
    if width <= 1.0 || width >= WIDE_GRID_MIN_WIDTH {
        3
    } else if width >= MEDIUM_GRID_MIN_WIDTH {
        2
    } else {
        1
    }
}

fn project(point: [f32; 3], size: Size) -> Point {
    let eye = camera_eye();
    let forward = normalize(sub([0.0, CAMERA_TARGET_Y, 0.0], eye));
    let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
    let up = cross(right, forward);
    let offset = sub(point, eye);
    let depth = dot(offset, forward).max(1e-3);
    let unit = stage_unit(size);
    let x = dot(offset, right) * CAMERA_FOCAL / depth;
    let y = dot(offset, up) * CAMERA_FOCAL / depth;
    Point {
        x: x * unit + size.width * 0.5,
        y: -y * unit + size.height * 0.5,
    }
}

fn slider_value_at(position: Point, size: Size) -> f32 {
    let left = project([-SLIDER_TRAVEL, SLIDER_LIFT, 0.0], size).x;
    let right = project([SLIDER_TRAVEL, SLIDER_LIFT, 0.0], size).x;
    let span = right - left;
    if span.abs() < 1e-3 {
        return 0.0;
    }
    ((position.x - left) / span).clamp(0.0, 1.0)
}

fn pointer_angle(position: Point, size: Size) -> Option<f32> {
    let pivot = project([0.0, DIAL_PIVOT_Y, 0.0], size);
    let dx = position.x - pivot.x;
    let dy = position.y - pivot.y;
    if dx * dx + dy * dy < DIAL_DEAD_ZONE_PX * DIAL_DEAD_ZONE_PX {
        return None;
    }
    Some(dy.atan2(dx))
}

fn wrap_angle(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

fn dial_value_after(value: f32, previous: f32, current: f32) -> f32 {
    (value + wrap_angle(current - previous) / DIAL_SWEEP).clamp(0.0, 1.0)
}

fn pointer_tilt(position: Point, size: Size) -> (f32, f32) {
    if size.width <= 1.0 || size.height <= 1.0 {
        return (0.0, 0.0);
    }
    let x = ((position.x / size.width) - 0.5).clamp(-0.5, 0.5) * 2.0;
    let y = ((position.y / size.height) - 0.5).clamp(-0.5, 0.5) * 2.0;
    (-x * TILT_YAW, -y * TILT_PITCH)
}

/// The value a press at `position` starts a drag from, for the controls a
/// drag moves. Compose's `Slider` jumps to the tapped position the same way.
fn drag_start_value(
    kind: ControlKind,
    state: ControlState,
    position: Point,
    size: Size,
) -> ControlState {
    match kind {
        ControlKind::Slider => state.with_value(slider_value_at(position, size)),
        _ => state,
    }
}

fn drag_state(
    kind: ControlKind,
    state: ControlState,
    tracking: DragTracking,
    position: Point,
    size: Size,
) -> (ControlState, DragTracking) {
    match kind {
        ControlKind::Slider => (state.with_value(slider_value_at(position, size)), tracking),
        ControlKind::VolumeDial => match pointer_angle(position, size) {
            Some(angle) => (
                state.with_value(dial_value_after(state.value, tracking.angle, angle)),
                DragTracking {
                    active: tracking.active,
                    angle,
                },
            ),
            None => (state, tracking),
        },
        _ => (state, tracking),
    }
}

fn page_color(dark: bool) -> Color {
    if dark {
        Color(0.043, 0.047, 0.058, 1.0)
    } else {
        Color(0.898, 0.898, 0.890, 1.0)
    }
}

fn card_color(dark: bool) -> Color {
    if dark {
        Color(0.086, 0.092, 0.106, 1.0)
    } else {
        Color(0.851, 0.851, 0.843, 1.0)
    }
}

fn divider_color(dark: bool) -> Color {
    if dark {
        Color(1.0, 1.0, 1.0, 0.10)
    } else {
        Color(0.0, 0.0, 0.0, 0.10)
    }
}

fn ink_color(dark: bool) -> Color {
    if dark {
        Color(0.94, 0.95, 0.97, 1.0)
    } else {
        Color(0.09, 0.10, 0.11, 1.0)
    }
}

fn muted_color(dark: bool) -> Color {
    if dark {
        Color(0.62, 0.65, 0.70, 1.0)
    } else {
        Color(0.38, 0.39, 0.41, 1.0)
    }
}

fn title_style(dark: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(ink_color(dark)),
            font_size: TextUnit::Sp(30.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn card_title_style(dark: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(ink_color(dark)),
            font_size: TextUnit::Sp(15.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn card_value_style(dark: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(muted_color(dark)),
            font_size: TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn controls_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    SOURCE
        .get_or_init(|| {
            Arc::<str>::from(format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}{}",
                include_str!("controls_ui.wgsl")
            ))
        })
        .clone()
}

struct StageUniforms {
    kind: f32,
    value: f32,
    press: f32,
    dark: f32,
    tilt_yaw: f32,
    tilt_pitch: f32,
    background: Color,
}

fn stage_effect(uniforms: &StageUniforms) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(controls_wgsl());
    shader.set_float(0, uniforms.kind);
    shader.set_float(1, uniforms.value);
    shader.set_float(2, uniforms.press);
    shader.set_float(4, uniforms.dark);
    shader.set_float(5, uniforms.tilt_yaw);
    shader.set_float(6, uniforms.tilt_pitch);
    shader.set_float(8, uniforms.background.0);
    shader.set_float(9, uniforms.background.1);
    shader.set_float(10, uniforms.background.2);
    shader.set_float(12, CAMERA_YAW);
    shader.set_float(13, CAMERA_PITCH);
    shader.set_float(14, CAMERA_DISTANCE);
    shader.set_float(15, CAMERA_FOCAL);
    shader.set_float(16, CAMERA_TARGET_Y);
    shader.set_float(17, SLIDER_TRAVEL);
    shader.set_float(18, SLIDER_LIFT);
    shader.set_float(19, STAGE_DESIGN_ASPECT);
    RenderEffect::runtime_shader(shader)
}

#[composable]
pub(crate) fn ControlsUiTab() {
    let dark = rememberMutableStateOf(|| false);
    let page_size = rememberMutableStateOf(Size::default);
    let is_dark = dark.get();

    Column(
        Modifier::empty()
            .fill_max_width()
            .report_size_state(page_size)
            .background(page_color(is_dark))
            .padding_symmetric(2.0, 6.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(GRID_SPACING)),
        move || {
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .padding_symmetric(6.0, 4.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpaceBetween)
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text("Controls UI", Modifier::empty(), title_style(is_dark));
                    ThemeSwitch(dark, is_dark);
                },
            );

            for row in CONTROL_KINDS.chunks(grid_columns(page_size.get().width)) {
                key(row[0], || ControlGridRow(row, is_dark));
            }
        },
    );
}

#[composable]
fn ThemeSwitch(dark: MutableState<bool>, is_dark: bool) {
    let label = if is_dark { "Light" } else { "Dark" };
    let background = if is_dark {
        Color(1.0, 1.0, 1.0, 0.10)
    } else {
        Color(0.0, 0.0, 0.0, 0.06)
    };

    Box(
        Modifier::empty()
            .rounded_corners(16.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(background), CornerRadii::uniform(16.0));
            })
            .toggleable(
                is_dark,
                Some(format!(
                    "Dark theme, {}",
                    if is_dark { "on" } else { "off" }
                )),
                Some(SemanticsWidgetRole::Switch),
                move |next| dark.set(next),
            )
            .padding_symmetric(16.0, 9.0),
        BoxSpec::default(),
        move || {
            Text(label, Modifier::empty(), card_value_style(is_dark));
        },
    );
}

/// One row of the grid.
///
/// The rows and the cards inside them are emitted from a single call site, so
/// each is wrapped in Compose's `key`: identity comes from the control a card
/// draws rather than from where the loop happened to reach it, and a reflow
/// between column counts moves a card's state with it.
#[composable]
fn ControlGridRow(kinds: &'static [ControlKind], dark: bool) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(GRID_SPACING)),
        move || {
            for kind in kinds {
                key(*kind, || {
                    ControlCard(*kind, dark, Modifier::empty().weight(1.0))
                });
            }
        },
    );
}

/// What a pointer event does to the press depth.
///
/// `Down` snaps the depth to fully pressed instead of animating toward it:
/// a click whose press and release land in the same input batch would
/// otherwise leave a target that composition never observes as pressed, and
/// the control would never visibly move.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PressResponse {
    Snap,
    Release,
    Hold,
}

fn press_response(kind: PointerEventKind) -> PressResponse {
    match kind {
        PointerEventKind::Down => PressResponse::Snap,
        PointerEventKind::Up | PointerEventKind::Cancel | PointerEventKind::Exit => {
            PressResponse::Release
        }
        _ => PressResponse::Hold,
    }
}

fn press_release_animation() -> AnimationType {
    spring(Spring::DampingRatioNoBouncy, Spring::StiffnessMedium)
}

fn remember_press_depth() -> Owned<Animatable<f32>> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer.remember(|| Animatable::new(0.0, runtime))
    })
}

fn apply_press(depth: &Owned<Animatable<f32>>, kind: PointerEventKind) {
    match press_response(kind) {
        PressResponse::Snap => depth.update(|animatable| animatable.snapTo(1.0)),
        PressResponse::Release => {
            depth.update(|animatable| animatable.animateTo(0.0, press_release_animation()))
        }
        PressResponse::Hold => {}
    }
}

/// The click behaviour and accessibility of one control: the switches are
/// `Modifier.toggleable`, the button is `Modifier.clickable`, and every card
/// names itself and publishes its reading the way Compose's
/// `contentDescription` / `stateDescription` pair does.
fn control_action(kind: ControlKind, state: MutableState<ControlState>) -> Modifier {
    let current = state.get();
    let action = match kind {
        ControlKind::Checkmark | ControlKind::Toggle | ControlKind::LeverSwitch => {
            Modifier::empty().toggleable(current.on, None, kind.role(), move |next| {
                state.set(state.get().toggled(next))
            })
        }
        ControlKind::PushButton => {
            Modifier::empty().clickable(move |_position| state.set(state.get().pressed_once()))
        }
        ControlKind::Slider | ControlKind::VolumeDial => Modifier::empty(),
    };
    let reading = current.label(kind);
    action.semantics(move |config: &mut SemanticsConfiguration| {
        config.content_description = Some(kind.title().to_string());
        config.state_description = Some(reading.clone());
        if let Some(role) = kind.role() {
            config.role = Some(role);
        }
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct StageTargets {
    value: f32,
    hovered: bool,
    tilt: (f32, f32),
}

#[derive(Clone, Copy)]
struct StageAnimation {
    value: State<f32>,
    press: State<f32>,
    yaw: State<f32>,
    pitch: State<f32>,
}

impl StageAnimation {
    fn uniforms(self, kind: ControlKind, dark: bool, background: Color) -> StageUniforms {
        StageUniforms {
            kind: kind.scene_index(),
            value: self.value.value(),
            press: self.press.value().clamp(0.0, 1.0),
            dark: if dark { 1.0 } else { 0.0 },
            tilt_yaw: self.yaw.value(),
            tilt_pitch: self.pitch.value(),
            background,
        }
    }
}

#[allow(non_snake_case)]
#[composable]
fn animateStageAsState(
    kind: ControlKind,
    targets: StageTargets,
    press: State<f32>,
) -> StageAnimation {
    let settle = spring(Spring::DampingRatioNoBouncy, Spring::StiffnessLow);
    let (yaw, pitch) = if targets.hovered {
        targets.tilt
    } else {
        (0.0, 0.0)
    };
    StageAnimation {
        value: animateFloatAsState(targets.value, kind.value_animation(), "controls_value"),
        press,
        yaw: animateFloatAsState(yaw, settle, "controls_yaw"),
        pitch: animateFloatAsState(pitch, settle, "controls_pitch"),
    }
}

#[composable]
fn ControlCard(kind: ControlKind, dark: bool, modifier: Modifier) {
    let state = rememberMutableStateOf(move || ControlState::initial(kind));
    let hovered = rememberMutableStateOf(|| false);
    let tilt = rememberMutableStateOf(|| (0.0f32, 0.0f32));
    let depth = remember_press_depth();

    let current = state.get();
    let animation = animateStageAsState(
        kind,
        StageTargets {
            value: current.scene_value(kind),
            hovered: hovered.get(),
            tilt: tilt.get(),
        },
        depth.with(|animatable| animatable.state()),
    );
    let background = card_color(dark);

    Column(
        modifier
            .rounded_corners(CARD_RADIUS)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(background), CornerRadii::uniform(CARD_RADIUS));
            }),
        ColumnSpec::default(),
        move || {
            let depth = depth.clone();
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(STAGE_HEIGHT)
                    .then(control_action(kind, state))
                    .graphics_layer(move || GraphicsLayer {
                        render_effect: Some(stage_effect(
                            &animation.uniforms(kind, dark, background),
                        )),
                        compositing_strategy: CompositingStrategy::Offscreen,
                        ..Default::default()
                    })
                    .pointer_input((), move |scope: PointerInputScope| {
                        let depth = depth.clone();
                        async move {
                            scope
                                .await_pointer_event_scope(|events| async move {
                                    let mut drag = DragTracking::default();
                                    loop {
                                        let event = events.await_pointer_event().await;
                                        let size = events.size();
                                        apply_press(&depth, event.kind);
                                        match event.kind {
                                            PointerEventKind::Enter => hovered.set(true),
                                            PointerEventKind::Exit => {
                                                hovered.set(false);
                                                drag = DragTracking::default();
                                            }
                                            PointerEventKind::Down => {
                                                hovered.set(true);
                                                drag = DragTracking {
                                                    active: kind.is_continuous(),
                                                    angle: pointer_angle(event.position, size)
                                                        .unwrap_or_default(),
                                                };
                                                if kind.is_continuous() {
                                                    state.set(drag_start_value(
                                                        kind,
                                                        state.get(),
                                                        event.position,
                                                        size,
                                                    ));
                                                    event.consume();
                                                }
                                            }
                                            PointerEventKind::Move => {
                                                tilt.set(pointer_tilt(event.position, size));
                                                if drag.active {
                                                    let (next, tracked) = drag_state(
                                                        kind,
                                                        state.get(),
                                                        drag,
                                                        event.position,
                                                        size,
                                                    );
                                                    state.set(next);
                                                    drag = tracked;
                                                    event.consume();
                                                }
                                            }
                                            PointerEventKind::Up | PointerEventKind::Cancel => {
                                                drag = DragTracking::default();
                                            }
                                            _ => {}
                                        }
                                    }
                                })
                                .await;
                        }
                    }),
                BoxSpec::default(),
                || {},
            );

            ControlFooter(kind, current, dark);
        },
    );
}

#[composable]
fn ControlFooter(kind: ControlKind, state: ControlState, dark: bool) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(1.0)
            .background(divider_color(dark)),
        BoxSpec::default(),
        || {},
    );

    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_symmetric(FOOTER_HORIZONTAL_PADDING, FOOTER_VERTICAL_PADDING),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(kind.title(), Modifier::empty(), card_title_style(dark));
            Text(state.label(kind), Modifier::empty(), card_value_style(dark));
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: Size = Size {
        width: 360.0,
        height: STAGE_HEIGHT,
    };

    #[test]
    fn projection_maps_scene_origin_near_stage_centre() {
        let centre = project([0.0, CAMERA_TARGET_Y, 0.0], STAGE);
        assert!((centre.x - STAGE.width * 0.5).abs() < 0.01);
        assert!((centre.y - STAGE.height * 0.5).abs() < 0.01);
    }

    #[test]
    fn projection_keeps_rail_ends_inside_the_stage() {
        let left = project([-SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
        let right = project([SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
        assert!(left.x > 0.0 && left.x < STAGE.width);
        assert!(right.x > left.x && right.x < STAGE.width);
    }

    #[test]
    fn slider_value_follows_pointer_across_the_rail() {
        let left = project([-SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
        let right = project([SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
        assert_eq!(slider_value_at(left, STAGE), 0.0);
        assert_eq!(slider_value_at(right, STAGE), 1.0);
        let middle = Point {
            x: (left.x + right.x) * 0.5,
            y: left.y,
        };
        assert!((slider_value_at(middle, STAGE) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn slider_value_clamps_outside_the_rail() {
        assert_eq!(slider_value_at(Point { x: -400.0, y: 0.0 }, STAGE), 0.0);
        assert_eq!(slider_value_at(Point { x: 900.0, y: 0.0 }, STAGE), 1.0);
    }

    #[test]
    fn slider_value_is_zero_for_a_stage_that_was_never_laid_out() {
        assert_eq!(
            slider_value_at(Point { x: 0.0, y: 0.0 }, Size::default()),
            0.0
        );
    }

    #[test]
    fn a_press_snaps_down_so_a_click_inside_one_batch_still_shows() {
        assert_eq!(press_response(PointerEventKind::Down), PressResponse::Snap);
        assert_eq!(press_response(PointerEventKind::Up), PressResponse::Release);
        assert_eq!(
            press_response(PointerEventKind::Cancel),
            PressResponse::Release
        );
        assert_eq!(
            press_response(PointerEventKind::Exit),
            PressResponse::Release
        );
        assert_eq!(press_response(PointerEventKind::Move), PressResponse::Hold);
        assert_eq!(press_response(PointerEventKind::Enter), PressResponse::Hold);
        assert_eq!(
            press_response(PointerEventKind::Scroll),
            PressResponse::Hold
        );
    }

    #[test]
    fn grid_columns_narrow_to_one_and_assume_wide_before_the_first_layout() {
        assert_eq!(grid_columns(0.0), 3);
        assert_eq!(grid_columns(1200.0), 3);
        assert_eq!(grid_columns(WIDE_GRID_MIN_WIDTH), 3);
        assert_eq!(grid_columns(WIDE_GRID_MIN_WIDTH - 1.0), 2);
        assert_eq!(grid_columns(MEDIUM_GRID_MIN_WIDTH), 2);
        assert_eq!(grid_columns(MEDIUM_GRID_MIN_WIDTH - 1.0), 1);
        assert_eq!(grid_columns(320.0), 1);
    }

    #[test]
    fn every_grid_column_count_covers_all_six_controls() {
        for width in [320.0, 700.0, 1200.0] {
            let rows: Vec<&[ControlKind]> = CONTROL_KINDS.chunks(grid_columns(width)).collect();
            let placed: Vec<ControlKind> = rows.concat();
            assert_eq!(
                placed,
                CONTROL_KINDS.to_vec(),
                "width {width} drops a control"
            );
        }
    }

    #[test]
    fn a_stage_narrower_than_the_design_aspect_scales_the_scene_down() {
        let wide = Size {
            width: STAGE_HEIGHT * STAGE_DESIGN_ASPECT * 2.0,
            height: STAGE_HEIGHT,
        };
        assert_eq!(stage_unit(wide), STAGE_HEIGHT);
        let narrow = Size {
            width: STAGE_HEIGHT,
            height: STAGE_HEIGHT,
        };
        assert!(stage_unit(narrow) < STAGE_HEIGHT);
        let rail_end = project([-SLIDER_TRAVEL, SLIDER_LIFT, 0.0], narrow);
        assert!(
            rail_end.x > 0.0,
            "the rail must stay inside a square stage, got {}",
            rail_end.x
        );
    }

    #[test]
    fn wrap_angle_folds_full_turns() {
        assert!((wrap_angle(0.4) - 0.4).abs() < 1e-5);
        assert!((wrap_angle(0.4 + TAU) - 0.4).abs() < 1e-4);
        assert!((wrap_angle(PI + 0.2) - (-PI + 0.2)).abs() < 1e-4);
    }

    #[test]
    fn dial_value_turns_with_the_pointer_and_clamps() {
        let quarter = DIAL_SWEEP * 0.25;
        assert!((dial_value_after(0.4, 0.0, quarter) - 0.65).abs() < 1e-4);
        assert!((dial_value_after(0.4, quarter, 0.0) - 0.15).abs() < 1e-4);
        assert_eq!(dial_value_after(0.95, 0.0, quarter), 1.0);
        assert_eq!(dial_value_after(0.05, quarter, 0.0), 0.0);
    }

    #[test]
    fn pointer_angle_ignores_the_pivot_dead_zone() {
        let pivot = project([0.0, DIAL_PIVOT_Y, 0.0], STAGE);
        assert!(pointer_angle(pivot, STAGE).is_none());
        let away = Point {
            x: pivot.x + 60.0,
            y: pivot.y,
        };
        assert!(pointer_angle(away, STAGE).expect("angle").abs() < 1e-5);
    }

    #[test]
    fn pointer_tilt_is_centred_and_bounded() {
        let centre = Point {
            x: STAGE.width * 0.5,
            y: STAGE.height * 0.5,
        };
        assert_eq!(pointer_tilt(centre, STAGE), (0.0, 0.0));
        let corner = Point { x: 0.0, y: 0.0 };
        assert!((pointer_tilt(corner, STAGE).0 - TILT_YAW).abs() < 1e-5);
        assert!((pointer_tilt(corner, STAGE).1 - TILT_PITCH).abs() < 1e-5);
        assert_eq!(pointer_tilt(centre, Size::default()), (0.0, 0.0));
    }

    #[test]
    fn a_switch_takes_the_value_its_toggleable_hands_over() {
        let off = ControlState::initial(ControlKind::Toggle);
        assert!(!off.on);
        assert!(off.toggled(true).on);
        assert!(!off.toggled(true).toggled(false).on);
    }

    #[test]
    fn a_click_counts_one_press() {
        let button = ControlState::initial(ControlKind::PushButton);
        assert_eq!(button.presses, 0);
        assert_eq!(button.pressed_once().presses, 1);
        assert_eq!(button.pressed_once().pressed_once().presses, 2);
        assert_eq!(
            ControlState {
                presses: u32::MAX,
                ..button
            }
            .pressed_once()
            .presses,
            u32::MAX
        );
    }

    #[test]
    fn a_drag_starts_the_slider_at_the_pressed_position_and_leaves_the_dial() {
        let right = project([SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
        let slider = ControlState::initial(ControlKind::Slider);
        assert_eq!(
            drag_start_value(ControlKind::Slider, slider, right, STAGE).value,
            1.0
        );
        let dial = ControlState::initial(ControlKind::VolumeDial);
        assert_eq!(
            drag_start_value(ControlKind::VolumeDial, dial, right, STAGE),
            dial
        );
    }

    #[test]
    fn only_the_dragged_controls_are_continuous() {
        assert!(ControlKind::Slider.is_continuous());
        assert!(ControlKind::VolumeDial.is_continuous());
        for kind in [
            ControlKind::Checkmark,
            ControlKind::Toggle,
            ControlKind::LeverSwitch,
            ControlKind::PushButton,
        ] {
            assert!(!kind.is_continuous(), "{kind:?} is clicked, not dragged");
            assert!(kind.role().is_some(), "{kind:?} needs a semantics role");
        }
    }

    #[test]
    fn a_value_is_clamped_to_its_track() {
        let slider = ControlState::initial(ControlKind::Slider);
        assert_eq!(slider.with_value(1.4).value, 1.0);
        assert_eq!(slider.with_value(-0.3).value, 0.0);
        assert_eq!(slider.with_value(0.42).value, 0.42);
    }

    #[test]
    fn drag_turns_the_dial_and_records_the_new_angle() {
        let pivot = project([0.0, DIAL_PIVOT_Y, 0.0], STAGE);
        let below = Point {
            x: pivot.x,
            y: pivot.y + 60.0,
        };
        let state = ControlState::initial(ControlKind::VolumeDial);
        let tracking = DragTracking {
            active: true,
            angle: 0.0,
        };
        let (next, tracked) = drag_state(ControlKind::VolumeDial, state, tracking, below, STAGE);
        assert!(next.value > state.value);
        assert!((tracked.angle - PI * 0.5).abs() < 1e-4);
        assert!(tracked.active);
    }

    #[test]
    fn drag_leaves_switches_untouched() {
        let state = ControlState::initial(ControlKind::Checkmark);
        let tracking = DragTracking {
            active: true,
            angle: 0.2,
        };
        let (next, tracked) = drag_state(
            ControlKind::Checkmark,
            state,
            tracking,
            Point { x: 10.0, y: 10.0 },
            STAGE,
        );
        assert_eq!(next, state);
        assert_eq!(tracked.angle, 0.2);
    }

    #[test]
    fn labels_report_the_state_the_footer_shows() {
        assert_eq!(
            ControlState::initial(ControlKind::Checkmark).label(ControlKind::Checkmark),
            "Checked"
        );
        assert_eq!(
            ControlState::initial(ControlKind::Slider).label(ControlKind::Slider),
            "35 %"
        );
        assert_eq!(
            ControlState::initial(ControlKind::VolumeDial).label(ControlKind::VolumeDial),
            "40 %"
        );
        assert_eq!(
            ControlState::initial(ControlKind::Toggle).label(ControlKind::Toggle),
            "Off"
        );
        assert_eq!(
            ControlState::initial(ControlKind::PushButton).label(ControlKind::PushButton),
            "0 presses"
        );
        let once = ControlState {
            presses: 1,
            ..ControlState::initial(ControlKind::PushButton)
        };
        assert_eq!(once.label(ControlKind::PushButton), "1 press");
    }

    #[test]
    fn switch_scene_values_are_binary_and_dials_are_continuous() {
        let on = ControlState {
            on: true,
            value: 0.3,
            presses: 0,
        };
        assert_eq!(on.scene_value(ControlKind::Toggle), 1.0);
        assert_eq!(
            ControlState { on: false, ..on }.scene_value(ControlKind::Toggle),
            0.0
        );
        assert_eq!(on.scene_value(ControlKind::Slider), 0.3);
    }

    #[test]
    fn every_kind_has_a_distinct_scene_index() {
        let mut seen = Vec::new();
        for kind in CONTROL_KINDS {
            let index = kind.scene_index();
            assert!(!seen.contains(&index.to_bits()));
            seen.push(index.to_bits());
        }
        assert_eq!(seen.len(), 6);
    }
}
