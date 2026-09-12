#![allow(non_snake_case)]

use std::{cell::Cell, rc::Rc};

use cranpose::{
    composable,
    liquid::prelude::*,
    mutableStateOf, remember,
    text::{FontWeight, SpanStyle, TextStyle, TextUnit},
    widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text},
    Brush, Color, CornerRadii, GraphicsLayer, Modifier, MutableState, Point, PointerEventKind,
    Size,
};
use cranpose_animation::{
    animateFloatAsState, infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, RepeatMode,
    StartOffset,
};
use cranpose_core::State;
use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui::{Alignment, HorizontalAlignment, LinearArrangement, PointerEvent};
use cranpose_ui_graphics::{BlendMode, DrawScope, Rect, Stroke};

const SLAB_WIDTH: f32 = 640.0;
const SLAB_HEIGHT: f32 = 560.0;
const SLAB_RADIUS: f32 = 36.0;
const TILE_WIDTH: f32 = 236.0;
const TILE_HEIGHT: f32 = 204.0;
const TILE_RADIUS: f32 = 24.0;
const TILE_GAP: f32 = 30.0;
const CAMERA_DISTANCE: f32 = 12.0;
const TILT_DEGREES: f32 = 13.0;
const PRESS_TILT_DEGREES: f32 = 4.0;
const SWAY_DEGREES: f32 = 3.0;
const TILE_PARALLAX: f32 = 0.35;
const TILE_SHIFT_DP_PER_DEGREE: f32 = 0.9;
const CLOCK_PERIOD_MS: u64 = 48_000;
const SWAY_CYCLES: f32 = 9.0;
const BREATHE_CYCLES: f32 = 16.0;
const SWEEP_CYCLES: f32 = 7.0;
const STAGE_GRID_DP: f32 = 56.0;
const STAGE_BEAM_HALF_WIDTH: f32 = 90.0;
const SLAB_SWEEP_HALF_WIDTH: f32 = 130.0;
const TILE_BEAM_HALF_WIDTH: f32 = 22.0;
const TILE_STREAK_HALF_WIDTH: f32 = 3.5;
const TILE_BEVEL_INSET: f32 = 8.0;
const TILE_BLOOM_BANDS: usize = 8;
const TILE_BLOOM_BAND_DP: f32 = 6.0;
const TILE_GLINT_RADIUS: f32 = 22.0;
const TILE_SMOKE_ALPHA: f32 = 0.5;
const SLAB_BEVEL_INSET: f32 = 6.0;
const TAU: f32 = std::f32::consts::TAU;

/// One tile: a language pair and the colour of its glass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileSpec {
    /// The source language code.
    pub from: &'static str,
    /// The target language code.
    pub to: &'static str,
    /// The tile's glass colour.
    pub color: Color,
}

/// The four tiles, top-left to bottom-right.
pub const TILES: [TileSpec; 4] = [
    TileSpec {
        from: "AU",
        to: "FR",
        color: Color::from_rgb_u8(64, 186, 255),
    },
    TileSpec {
        from: "FR",
        to: "AU",
        color: Color::from_rgb_u8(92, 255, 118),
    },
    TileSpec {
        from: "AU",
        to: "DE",
        color: Color::from_rgb_u8(255, 138, 48),
    },
    TileSpec {
        from: "DE",
        to: "AU",
        color: Color::from_rgb_u8(232, 78, 255),
    },
];

const STAGE_BEAMS: [(Color, f32, f32, f32); 6] = [
    (Color::from_rgb_u8(255, 70, 110), 3.0, 0.05, 0.9),
    (Color::from_rgb_u8(60, 130, 255), 5.0, 0.4, -0.7),
    (Color::from_rgb_u8(40, 230, 210), 4.0, 0.7, 1.4),
    (Color::from_rgb_u8(255, 180, 60), 6.0, 0.2, -1.1),
    (Color::from_rgb_u8(190, 90, 255), 2.0, 0.55, 0.5),
    (Color::from_rgb_u8(220, 235, 255), 7.0, 0.85, -0.4),
];

const STAGE_BOKEH: [(Color, f32, f32, f32, f32); 7] = [
    (Color::from_rgb_u8(255, 90, 140), 1.0, 0.1, 0.2, 26.0),
    (Color::from_rgb_u8(80, 150, 255), 1.0, 0.8, 0.5, 34.0),
    (Color::from_rgb_u8(255, 200, 90), 2.0, 0.35, 0.85, 20.0),
    (Color::from_rgb_u8(120, 255, 200), 1.0, 0.6, 0.15, 24.0),
    (Color::from_rgb_u8(255, 120, 60), 2.0, 0.9, 0.7, 30.0),
    (Color::from_rgb_u8(200, 120, 255), 1.0, 0.25, 0.6, 22.0),
    (Color::from_rgb_u8(255, 255, 255), 3.0, 0.5, 0.35, 14.0),
];

const STAGE_GLOWS: [(Color, f32, f32, f32); 3] = [
    (Color::from_rgb_u8(255, 60, 120), 1.0, 0.0, 1.9),
    (Color::from_rgb_u8(50, 110, 255), 1.0, 2.1, 0.4),
    (Color::from_rgb_u8(255, 160, 50), 2.0, 4.0, 3.1),
];

fn slab_glass() -> Glass {
    Glass::clear()
        .shape(LiquidShape::RoundedRect(SLAB_RADIUS))
        .tint(Color::rgba(0.02, 0.03, 0.06, 0.32))
        .blur_radius(0.0)
        .adaptive_frost(Color::WHITE, 0.0)
        .lift(0.0)
        .refraction_depth(0.2)
        .refraction_curve(0.6)
        .dispersion(0.3)
        .transmission_refraction(1.0)
        .highlight(1.3)
        .shadow_style(GlassShadow::new(
            Color::BLACK.with_alpha(0.7),
            50.0,
            26.0,
            0.0,
        ))
}

fn tile_glass(color: Color) -> Glass {
    Glass::lens()
        .shape(LiquidShape::RoundedRect(TILE_RADIUS))
        .tint(color.with_alpha(0.05))
        .ink_recolor(color, 0.12)
        .lift(0.0)
        .refraction_depth(0.9)
        .refraction_curve(0.5)
        .dispersion(0.55)
        .transmission_refraction(1.0)
        .saturation(1.3)
        .highlight(0.9)
        .contrast(1.1)
        .shadow_style(GlassShadow::new(color.with_alpha(0.85), 34.0, 0.0, 4.0))
}

/// The semantics description of a tile, which robot runners find it by.
pub fn tile_description(spec: &TileSpec) -> String {
    format!("Glass tile {} → {}", spec.from, spec.to)
}

fn label_style(size_sp: f32, weight: Option<u16>) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color::WHITE),
            font_size: TextUnit::Sp(size_sp),
            font_weight: weight.map(FontWeight::new),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn mix(color: Color, toward: Color, amount: f32) -> Color {
    Color::rgba(
        color.r() + (toward.r() - color.r()) * amount,
        color.g() + (toward.g() - color.g()) * amount,
        color.b() + (toward.b() - color.b()) * amount,
        color.a(),
    )
}

fn draw_beam(
    scope: &mut dyn DrawScope,
    color: Color,
    alpha: f32,
    x: f32,
    half_thickness: f32,
    slope: f32,
) {
    let length = (1.0 + slope * slope).sqrt();
    let normal = Point::new(-slope / length * half_thickness, half_thickness / length);
    let clear = color.with_alpha(0.0);
    scope.draw_rect_blend(
        Brush::linear_gradient_range(
            vec![clear, color.with_alpha(alpha), clear],
            Point::new(x - normal.x, -normal.y),
            Point::new(x + normal.x, normal.y),
        ),
        BlendMode::Plus,
    );
}

fn draw_stage(scope: &mut dyn DrawScope, t: f32) {
    let size = scope.size();
    scope.draw_rect(Brush::solid(Color::from_rgb_u8(6, 7, 11)));
    let line = Brush::solid(Color::WHITE.with_alpha(0.035));
    let mut x = STAGE_GRID_DP;
    while x < size.width {
        scope.draw_rect_at(
            Rect {
                x,
                y: 0.0,
                width: 1.0,
                height: size.height,
            },
            line.clone(),
        );
        x += STAGE_GRID_DP;
    }
    let mut y = STAGE_GRID_DP;
    while y < size.height {
        scope.draw_rect_at(
            Rect {
                x: 0.0,
                y,
                width: size.width,
                height: 1.0,
            },
            line.clone(),
        );
        y += STAGE_GRID_DP;
    }
    let reach = size.width.max(size.height) * 0.42;
    for (color, cycles, phase_x, phase_y) in STAGE_GLOWS {
        let angle = t * TAU * cycles;
        let center = Point::new(
            size.width * (0.5 + 0.42 * (angle + phase_x).sin()),
            size.height * (0.5 + 0.42 * (angle * 0.8 + phase_y).cos()),
        );
        scope.draw_circle_blend(
            Brush::radial_gradient(
                vec![color.with_alpha(0.16), color.with_alpha(0.0)],
                center,
                reach,
            ),
            center,
            reach,
            BlendMode::Plus,
        );
    }
    for (color, cycles, phase, slope) in STAGE_BEAMS {
        let cycle = (t * cycles + phase).fract();
        let x = -size.width * 0.8 + size.width * 2.6 * cycle;
        draw_beam(scope, color, 0.2, x, STAGE_BEAM_HALF_WIDTH, slope);
    }
    for (color, cycles, phase_x, phase_y, radius) in STAGE_BOKEH {
        let angle = t * TAU * cycles;
        let center = Point::new(
            size.width * (phase_x + 0.06 * angle.sin()),
            size.height * (phase_y + 0.05 * (angle * 1.3).cos()),
        );
        scope.draw_circle_blend(
            Brush::radial_gradient(
                vec![color.with_alpha(0.9), color.with_alpha(0.0)],
                center,
                radius,
            ),
            center,
            radius,
            BlendMode::Plus,
        );
    }
}

fn draw_slab_light(scope: &mut dyn DrawScope, t: f32) {
    let size = scope.size();
    let cycle = (t * SWEEP_CYCLES).fract();
    let x = -size.width * 0.6 + size.width * 2.2 * cycle;
    draw_beam(scope, Color::WHITE, 0.07, x, SLAB_SWEEP_HALF_WIDTH, 0.6);
    let radii = CornerRadii::uniform(SLAB_RADIUS);
    scope.draw_round_rect_stroked_blend(
        Brush::solid(Color::WHITE.with_alpha(0.05)),
        radii,
        Stroke::new(16.0),
        BlendMode::Plus,
    );
    draw_rim(
        scope,
        Brush::solid(Color::WHITE.with_alpha(0.12)),
        1.0,
        SLAB_BEVEL_INSET,
        SLAB_RADIUS,
        BlendMode::SrcOver,
    );
    scope.draw_round_rect_stroked(
        Brush::vertical_gradient(
            vec![Color::WHITE.with_alpha(0.85), Color::WHITE.with_alpha(0.25)],
            0.0,
            size.height,
        ),
        radii,
        Stroke::new(1.5),
    );
}

fn draw_tile_light(
    scope: &mut dyn DrawScope,
    color: Color,
    glow: f32,
    flood: f32,
    t: f32,
    index: usize,
) {
    let size = scope.size();
    scope.draw_rect(Brush::solid(
        Color::BLACK.with_alpha(TILE_SMOKE_ALPHA * (1.0 - 0.6 * flood)),
    ));
    scope.draw_rect_blend(
        Brush::solid(color.with_alpha(0.32 * flood)),
        BlendMode::Plus,
    );
    let phase = index as f32 * 0.31;
    let sweep = |cycle: f32| -size.width * 1.2 + size.width * 2.8 * cycle;
    draw_beam(
        scope,
        color,
        0.3 * glow,
        sweep((t * 3.0 + phase).fract()),
        TILE_BEAM_HALF_WIDTH,
        -0.6,
    );
    let streak_colour = mix(color, Color::WHITE, 0.7);
    for (streak, slope) in [0.9, 1.15, 0.7].into_iter().enumerate() {
        let cycle = (t * (2.0 + streak as f32) + phase + streak as f32 * 0.37).fract();
        draw_beam(
            scope,
            streak_colour,
            0.9 * glow,
            sweep(cycle),
            TILE_STREAK_HALF_WIDTH,
            slope,
        );
    }
    for band in 0..TILE_BLOOM_BANDS {
        let falloff = 1.0 - band as f32 / TILE_BLOOM_BANDS as f32;
        draw_rim(
            scope,
            Brush::solid(color.with_alpha(0.22 * falloff * falloff * glow)),
            TILE_BLOOM_BAND_DP,
            TILE_BLOOM_BAND_DP * (band as f32 + 0.5),
            TILE_RADIUS,
            BlendMode::Plus,
        );
    }
    draw_rim(
        scope,
        Brush::solid(color.with_alpha(0.7 * glow)),
        5.0,
        2.5,
        TILE_RADIUS,
        BlendMode::Plus,
    );
    draw_rim(
        scope,
        Brush::solid(mix(color, Color::WHITE, 0.3).with_alpha(0.35 * glow)),
        1.0,
        TILE_BEVEL_INSET,
        TILE_RADIUS,
        BlendMode::Plus,
    );
    draw_rim(
        scope,
        Brush::solid(mix(color, Color::WHITE, 0.55).with_alpha(0.5 + 0.5 * glow)),
        1.5,
        0.75,
        TILE_RADIUS,
        BlendMode::SrcOver,
    );
    draw_rim(
        scope,
        Brush::linear_gradient_range(
            vec![
                Color::WHITE.with_alpha(0.8 * glow),
                Color::WHITE.with_alpha(0.0),
            ],
            Point::new(0.0, 0.0),
            Point::new(size.width * 0.6, size.height * 0.8),
        ),
        2.0,
        1.0,
        TILE_RADIUS,
        BlendMode::Plus,
    );
    let glint = Point::new(
        size.width - TILE_GLINT_RADIUS - 8.0,
        TILE_GLINT_RADIUS + 8.0,
    );
    scope.draw_circle_blend(
        Brush::radial_gradient(
            vec![Color::WHITE.with_alpha(glow), Color::WHITE.with_alpha(0.0)],
            glint,
            TILE_GLINT_RADIUS,
        ),
        glint,
        TILE_GLINT_RADIUS,
        BlendMode::Plus,
    );
}

fn draw_rim(
    scope: &mut dyn DrawScope,
    brush: Brush,
    width: f32,
    inset: f32,
    radius: f32,
    blend_mode: BlendMode,
) {
    let size = scope.size();
    scope.draw_round_rect_at_stroked_blend(
        Rect {
            x: inset,
            y: inset,
            width: size.width - inset * 2.0,
            height: size.height - inset * 2.0,
        },
        brush,
        CornerRadii::uniform((radius - inset).max(1.0)),
        Stroke::new(width),
        blend_mode,
    );
}

fn breath(t: f32, index: usize) -> f32 {
    0.5 + 0.5 * (t * TAU * BREATHE_CYCLES + index as f32 * 0.25 * TAU).sin()
}

fn tile_layer(hover: f32, press: f32, rotation_x: f32, rotation_y: f32) -> GraphicsLayer {
    let scale = 1.0 + 0.05 * hover - 0.03 * press;
    GraphicsLayer {
        scale_x: scale,
        scale_y: scale,
        rotation_x: -rotation_x * TILE_PARALLAX,
        rotation_y: -rotation_y * TILE_PARALLAX,
        translation_x: rotation_y * TILE_SHIFT_DP_PER_DEGREE,
        translation_y: -rotation_x * TILE_SHIFT_DP_PER_DEGREE,
        camera_distance: CAMERA_DISTANCE,
        ..Default::default()
    }
}

fn tile_dynamics(hover: f32, glow: f32, press: f32, finger: (f32, f32)) -> GlassDynamics {
    GlassDynamics {
        highlight_boost: 0.3 * hover + 0.15 * glow,
        saturation_boost: 0.5 * hover + 0.1 * glow,
        tint_alpha_multiplier: Some(1.0 + 3.2 * hover + 0.25 * glow),
        ..Default::default()
    }
    .touched_up(press, Some(finger), (TILE_WIDTH * 0.5, TILE_HEIGHT * 0.5))
}

#[derive(Clone)]
struct TileTouch {
    index: usize,
    hovered: MutableState<bool>,
    pressed: MutableState<bool>,
    finger: Rc<Cell<(f32, f32)>>,
    pressed_tile: MutableState<Option<usize>>,
}

impl TileTouch {
    fn on_event(&self, event: &PointerEvent) {
        match event.kind {
            PointerEventKind::Enter => self.hovered.set(true),
            PointerEventKind::Exit => {
                self.hovered.set(false);
                self.release();
            }
            PointerEventKind::Move => self.finger.set((event.position.x, event.position.y)),
            PointerEventKind::Down => {
                self.finger.set((event.position.x, event.position.y));
                self.pressed.set(true);
                self.pressed_tile.set(Some(self.index));
            }
            PointerEventKind::Up | PointerEventKind::Cancel => self.release(),
            _ => {}
        }
    }

    fn release(&self) {
        self.pressed.set(false);
        self.pressed_tile.set(None);
    }
}

#[composable]
pub fn GlassTilesTab() {
    let clock = rememberInfiniteTransition("glass_tiles_clock").animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::linear(CLOCK_PERIOD_MS),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "glass_tiles_time",
    );
    let pointer = remember(|| mutableStateOf(None::<(f32, f32)>)).with(|state| *state);
    let pressed_tile = remember(|| mutableStateOf(None::<usize>)).with(|state| *state);
    Box(
        Modifier::empty()
            .fill_max_size()
            .draw_behind(move |scope| draw_stage(scope, clock.get()))
            .pointer_input((), move |scope| async move {
                scope
                    .await_pointer_event_scope(|events| async move {
                        loop {
                            let event = events.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Exit => pointer.set(None),
                                PointerEventKind::Enter
                                | PointerEventKind::Move
                                | PointerEventKind::Down
                                | PointerEventKind::Up => {
                                    let size = events.size();
                                    pointer.set(Some((
                                        event.position.x / size.width.max(1.0),
                                        event.position.y / size.height.max(1.0),
                                    )));
                                }
                                _ => {}
                            }
                        }
                    })
                    .await
            }),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || Slab(clock, pointer, pressed_tile),
    );
}

#[composable]
fn Slab(
    clock: State<f32>,
    pointer: MutableState<Option<(f32, f32)>>,
    pressed_tile: MutableState<Option<usize>>,
) {
    let (mut target_x, mut target_y, presence_target) = match pointer.get() {
        Some((fx, fy)) => (
            -(fy * 2.0 - 1.0) * TILT_DEGREES,
            (fx * 2.0 - 1.0) * TILT_DEGREES,
            1.0,
        ),
        None => (0.0, 0.0, 0.0),
    };
    if let Some(index) = pressed_tile.get() {
        target_x += if index < 2 {
            PRESS_TILT_DEGREES
        } else {
            -PRESS_TILT_DEGREES
        };
        target_y += if index % 2 == 0 {
            -PRESS_TILT_DEGREES
        } else {
            PRESS_TILT_DEGREES
        };
    }
    let rotation_x = animateFloatAsState(target_x, LiquidMotion::smooth(), "slab_rotation_x");
    let rotation_y = animateFloatAsState(target_y, LiquidMotion::smooth(), "slab_rotation_y");
    let presence = animateFloatAsState(presence_target, LiquidMotion::smooth(), "slab_presence");
    Box(
        Modifier::empty()
            .size(Size::new(SLAB_WIDTH, SLAB_HEIGHT))
            .graphics_layer(move || {
                let t = clock.get();
                let sway = 1.0 - presence.get();
                let scale = 1.0 + 0.02 * presence.get();
                GraphicsLayer {
                    rotation_x: rotation_x.get()
                        + sway * SWAY_DEGREES * (t * TAU * SWAY_CYCLES).sin(),
                    rotation_y: rotation_y.get()
                        + sway * SWAY_DEGREES * 1.3 * (t * TAU * SWAY_CYCLES * 0.7 + 1.0).cos(),
                    camera_distance: CAMERA_DISTANCE,
                    scale_x: scale,
                    scale_y: scale,
                    ..Default::default()
                }
            }),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .glass_effect_with(slab_glass(), GlassDynamics::default)
                    .draw_with_content(move |scope| {
                        scope.draw_content();
                        draw_slab_light(scope, clock.get());
                    }),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || SlabGrid(clock, rotation_x, rotation_y, pressed_tile),
            );
        },
    );
}

#[composable]
fn SlabGrid(
    clock: State<f32>,
    rotation_x: State<f32>,
    rotation_y: State<f32>,
    pressed_tile: MutableState<Option<usize>>,
) {
    Column(
        Modifier::empty(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(TILE_GAP)),
        move || {
            Row(
                Modifier::empty(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::SpacedBy(TILE_GAP)),
                move || {
                    Tile(0, clock, rotation_x, rotation_y, pressed_tile);
                    Tile(1, clock, rotation_x, rotation_y, pressed_tile);
                },
            );
            Row(
                Modifier::empty(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::SpacedBy(TILE_GAP)),
                move || {
                    Tile(2, clock, rotation_x, rotation_y, pressed_tile);
                    Tile(3, clock, rotation_x, rotation_y, pressed_tile);
                },
            );
        },
    );
}

#[composable]
fn Tile(
    index: usize,
    clock: State<f32>,
    rotation_x: State<f32>,
    rotation_y: State<f32>,
    pressed_tile: MutableState<Option<usize>>,
) {
    let spec = &TILES[index];
    let color = spec.color;
    let hovered = remember(|| mutableStateOf(false)).with(|state| *state);
    let pressed = remember(|| mutableStateOf(false)).with(|state| *state);
    let finger =
        remember(|| Rc::new(Cell::new((TILE_WIDTH * 0.5, TILE_HEIGHT * 0.5)))).with(Rc::clone);
    let hover = animateFloatAsState(
        if hovered.get() { 1.0 } else { 0.0 },
        LiquidMotion::smooth(),
        "tile_hover",
    );
    let press = animateFloatAsState(
        if pressed.get() { 1.0 } else { 0.0 },
        LiquidMotion::snappy(),
        "tile_press",
    );
    let touch = TileTouch {
        index,
        hovered,
        pressed,
        finger: Rc::clone(&finger),
        pressed_tile,
    };
    let description = tile_description(spec);
    Box(
        Modifier::empty()
            .size(Size::new(TILE_WIDTH, TILE_HEIGHT))
            .semantics(move |config: &mut SemanticsConfiguration| {
                config.content_description = Some(description.clone());
            })
            .graphics_layer(move || {
                tile_layer(hover.get(), press.get(), rotation_x.get(), rotation_y.get())
            })
            .pointer_input(index, move |scope| {
                let touch = touch.clone();
                async move {
                    scope
                        .await_pointer_event_scope(|events| async move {
                            loop {
                                touch.on_event(&events.await_pointer_event().await);
                            }
                        })
                        .await
                }
            }),
        BoxSpec::default(),
        move || {
            let finger = Rc::clone(&finger);
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .glass_effect_with(tile_glass(color), move || {
                        tile_dynamics(
                            hover.get(),
                            breath(clock.get(), index),
                            press.get(),
                            finger.get(),
                        )
                    })
                    .draw_with_content(move |scope| {
                        let t = clock.get();
                        let flood = hover.get();
                        let glow = 0.5 + 0.5 * flood + 0.12 * breath(t, index);
                        draw_tile_light(scope, color, glow, flood, t, index);
                        scope.draw_content();
                    }),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || TileLabel(spec),
            );
        },
    );
}

#[composable]
fn TileLabel(spec: &'static TileSpec) {
    Column(
        Modifier::empty(),
        ColumnSpec::default().horizontal_alignment(HorizontalAlignment::CenterHorizontally),
        move || {
            Text(spec.from, Modifier::empty(), label_style(26.0, Some(600)));
            Row(Modifier::empty(), RowSpec::default(), move || {
                Text("→ ", Modifier::empty(), label_style(26.0, None));
                Text(spec.to, Modifier::empty(), label_style(26.0, Some(600)));
            });
        },
    );
}
