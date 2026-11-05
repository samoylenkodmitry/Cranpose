// The things that should not be in a room, each a mesh whose content is
// live UI: an eye that turns to watch the eye, a television the entity
// types through, and switches that flip by themselves.

use std::{sync::Arc, time::Duration};

use cranpose::liquid::prelude::{LiquidTheme, LiquidThemeSpec, LiquidToggle, SchemeMode};
use cranpose_core::{
    delay, derivedStateOf, rememberMutableStateOf, LaunchedEffectAsync, MutableState,
};
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Vec3},
    text::{FontFamily, TextUnit},
    Alignment, Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec, DrawScope,
    Modifier, Point, Rect, Row, RowSpec, Size, Text, TextStyle, TileMode,
};

use super::{
    finish::{Finish, Lamps, Sight},
    journey::Journey,
    notes,
    place::{facing, spot},
    plan::Thing,
    room::Level,
};

const DOWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);
// The watcher: its content, wrapped once around it, its radius and how
// high its middle floats.
const EYEBALL: Size = Size {
    width: 512.0,
    height: 256.0,
};
const EYEBALL_RADIUS: f32 = 45.0;
const WATCHER_HEIGHT: f32 = 150.0;
const BLINK: Duration = Duration::from_millis(1400);
// The television: its body, how high its middle floats, and its screen's
// content, shown at half its size.
const TELEVISION: Vec3 = Vec3::new(150.0, 115.0, 95.0);
const TELEVISION_HEIGHT: f32 = 120.0;
const SCREEN: Size = Size {
    width: 240.0,
    height: 180.0,
};
const SCREEN_SCALE: f32 = 0.5;
// The camcorder's frames a letter takes to type.
const FRAMES_PER_LETTER: u32 = 2;
// The poltergeist's panel, shown at a little over half its size, how high
// its foot floats and how often a switch flips.
const PANEL: Size = Size {
    width: 400.0,
    height: 180.0,
};
const PANEL_SCALE: f32 = 0.6;
const PANEL_FOOT: f32 = 45.0;
const FLIP: Duration = Duration::from_millis(320);
const SWITCHES: u32 = 10;

fn mono(size: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style.span_style.font_family = Some(FontFamily::Monospace);
    style
}

// A hash of two numbers, from 0 to 255.
fn noise(a: u32, b: u32) -> u32 {
    (a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b)).wrapping_mul(0xc2b2_ae35) >> 24
}

// The thing standing in the middle of a room, its floor point `middle`.
#[composable(no_skip)]
pub(super) fn Anomaly(level: Level, sight: Sight, lamps: Lamps, thing: Thing, middle: Vec3) {
    match thing {
        Thing::Watcher => Watcher(Arc::clone(&level.meshes.sphere), sight, lamps, middle),
        Thing::Television => Television(level, sight, lamps, middle),
        Thing::Poltergeist => Poltergeist(Arc::clone(&level.meshes.plane), sight, lamps, middle),
    }
}

// An eye that floats over the floor and turns to watch the eye.
#[composable(no_skip)]
fn Watcher(sphere: Arc<Mesh>, sight: Sight, lamps: Lamps, middle: Vec3) {
    let shut = rememberMutableStateOf(|| false);
    LaunchedEffectAsync(shut.get(), move |scope| {
        Box::pin(async move {
            if shut.get_non_reactive() {
                delay(BLINK).await;
                if scope.is_active() {
                    shut.set(false);
                }
            }
        })
    });
    let frames = sight.journey.frames;
    Box(
        spot(EYEBALL).mesh_surface(move || {
            let bob = (frames.get() as f32 * 0.08).sin() * 8.0;
            let center = middle - DOWN * (WATCHER_HEIGHT + bob);
            let transform = facing(
                center,
                sight.eye_in_room(),
                Vec3::new(EYEBALL_RADIUS, EYEBALL_RADIUS, EYEBALL_RADIUS),
            );
            MeshSurface {
                mesh: Arc::clone(&sphere),
                transform,
                material: sight.material(Finish::Content, &transform, lamps),
                casts_shadow: false,
                solid: true,
            }
        }),
        BoxSpec::default(),
        move || EyeballFace(shut),
    );
}

#[composable]
fn EyeballFace(shut: MutableState<bool>) {
    let closed = shut.get();
    Box(
        Modifier::empty()
            .fill_max_size()
            .draw_behind(move |scope| eyeball(scope, closed)),
        BoxSpec::default(),
        move || {
            let ink = Color(0.45, 0.04, 0.03, 1.0);
            Text(
                if closed {
                    "I STILL SEE YOU"
                } else {
                    "DON'T BLINK"
                },
                Modifier::empty().align(Alignment::TOP_CENTER).padding(62.0),
                mono(20.0, ink),
            );
            if closed {
                return;
            }
            Button(
                Modifier::empty()
                    .align(Alignment::BOTTOM_CENTER)
                    .padding(48.0)
                    .rounded_corners(8.0)
                    .background(Color(0.35, 0.05, 0.04, 1.0))
                    .padding(10.0),
                ButtonSpec::default(),
                move || shut.set(true),
                || {
                    Text("BLINK", Modifier::empty(), mono(16.0, Color::WHITE));
                },
            );
        },
    );
}

// The eye's white, its iris and pupil in the middle, which faces the eye
// that it watches; or, shut, its lid.
fn eyeball(scope: &mut dyn DrawScope, shut: bool) {
    let size = scope.size();
    let middle = Point::new(size.width * 0.5, size.height * 0.5);
    if shut {
        scope.draw_rect(Brush::solid(Color(0.62, 0.45, 0.38, 1.0)));
        scope.draw_rect_at(
            Rect {
                x: middle.x - 70.0,
                y: middle.y - 2.0,
                width: 140.0,
                height: 4.0,
            },
            Brush::solid(Color(0.18, 0.08, 0.06, 1.0)),
        );
        return;
    }
    scope.draw_rect(Brush::solid(Color(0.90, 0.87, 0.80, 1.0)));
    let reach = size.width * 0.5;
    scope.draw_rect(Brush::radial_gradient_stops(
        vec![
            (0.0, Color(0.0, 0.0, 0.0, 0.0)),
            (0.35, Color(0.0, 0.0, 0.0, 0.0)),
            (1.0, Color(0.70, 0.10, 0.08, 0.75)),
        ],
        middle,
        reach,
        TileMode::Clamp,
    ));
    scope.draw_circle(Brush::solid(Color(0.12, 0.10, 0.06, 1.0)), middle, 44.0);
    scope.draw_circle(
        Brush::radial_gradient_stops(
            vec![
                (0.0, Color(0.55, 0.50, 0.18, 1.0)),
                (1.0, Color(0.20, 0.30, 0.12, 1.0)),
            ],
            middle,
            40.0,
            TileMode::Clamp,
        ),
        middle,
        40.0,
    );
    scope.draw_circle(Brush::solid(Color::BLACK), middle, 17.0);
    scope.draw_circle(
        Brush::solid(Color(1.0, 1.0, 1.0, 0.8)),
        Point::new(middle.x - 9.0, middle.y - 9.0),
        4.0,
    );
}

// A television that floats over the floor, facing the door the eye came
// in by, and types what the entity says; ANSWER makes it say more.
#[composable(no_skip)]
fn Television(level: Level, sight: Sight, lamps: Lamps, middle: Vec3) {
    let center = middle - DOWN * TELEVISION_HEIGHT;
    let body = Mat4::scaling(TELEVISION).then(&Mat4::translation(center));
    let lit = sight.clone();
    let cuboid = Arc::clone(&level.meshes.cuboid);
    Box(
        spot(Size {
            width: 4.0,
            height: 4.0,
        })
        .mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&cuboid),
            transform: body,
            material: lit.material(Finish::Content, &body, lamps),
            casts_shadow: false,
            solid: true,
        })
        .background(Color(0.10, 0.09, 0.08, 1.0)),
        BoxSpec::default(),
        || {},
    );
    let (width, height) = (SCREEN.width * SCREEN_SCALE, SCREEN.height * SCREEN_SCALE);
    let screen = Mat4::from_axes(
        Vec3::new(width, 0.0, 0.0),
        DOWN * height,
        Vec3::new(0.0, 0.0, 1.0),
        center + Vec3::new(-width * 0.5, -height * 0.5, -TELEVISION.z * 0.5 - 0.6),
    );
    let plane = Arc::clone(&level.meshes.plane);
    let journey = sight.journey;
    Box(
        spot(SCREEN).mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&plane),
            transform: screen,
            material: Material::Emissive,
            casts_shadow: false,
            solid: false,
        }),
        BoxSpec::default(),
        move || Broadcast(journey),
    );
}

// The television's screen: static, and the entity's words typed over it.
#[composable]
fn Broadcast(journey: Journey) {
    let frames = journey.frames;
    let line = rememberMutableStateOf(|| 0_u32);
    let started = rememberMutableStateOf(move || frames.get_non_reactive());
    let typed = derivedStateOf(move || {
        let words = notes::screen(line.get(), &journey);
        let letters = frames.get().saturating_sub(started.get()) / FRAMES_PER_LETTER;
        let shown = usize::try_from(letters).unwrap_or(usize::MAX);
        words.chars().take(shown).collect::<String>()
    });
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.05, 0.06, 0.07, 1.0))
            .draw_behind(move |scope| static_noise(scope, frames.get())),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty().fill_max_size().padding(14.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        typed.get(),
                        Modifier::empty().weight(1.0),
                        mono(17.0, Color(0.85, 0.95, 0.88, 1.0)),
                    );
                    Button(
                        Modifier::empty()
                            .rounded_corners(6.0)
                            .background(Color(0.85, 0.95, 0.88, 0.9))
                            .padding(8.0),
                        ButtonSpec::default(),
                        move || {
                            line.update(|line| *line += 1);
                            started.set(frames.get_non_reactive());
                        },
                        || {
                            Text("ANSWER", Modifier::empty(), mono(14.0, Color::BLACK));
                        },
                    );
                },
            );
        },
    );
}

// Bands of grey that change every frame, and specks of white.
fn static_noise(scope: &mut dyn DrawScope, frame: u32) {
    const BANDS: u32 = 30;
    const SPECKS: u32 = 50;
    let size = scope.size();
    let band = size.height / BANDS as f32;
    for index in 0..BANDS {
        let grey = 0.06 + noise(frame, index) as f32 / 255.0 * 0.22;
        scope.draw_rect_at(
            Rect {
                x: 0.0,
                y: index as f32 * band,
                width: size.width,
                height: band,
            },
            Brush::solid(Color(grey, grey, grey * 1.05, 1.0)),
        );
    }
    for index in 0..SPECKS {
        let x = noise(frame, index + 1000) as f32 / 255.0 * size.width;
        let y = noise(frame, index + 2000) as f32 / 255.0 * size.height;
        scope.draw_rect_at(
            Rect {
                x,
                y,
                width: 3.0,
                height: 2.0,
            },
            Brush::solid(Color(0.9, 0.9, 0.9, 0.7)),
        );
    }
}

// A panel that floats over the floor, swaying, with switches on it that
// flip by themselves.
#[composable(no_skip)]
fn Poltergeist(plane: Arc<Mesh>, sight: Sight, lamps: Lamps, middle: Vec3) {
    let on = rememberMutableStateOf(|| 0b01_0110_1001_u32);
    LaunchedEffectAsync((), move |scope| {
        Box::pin(async move {
            let mut flips = 0_u32;
            while scope.is_active() {
                delay(FLIP).await;
                flips = flips.wrapping_add(1);
                let bit = noise(flips, 7) % SWITCHES;
                on.update(|on| *on ^= 1 << bit);
            }
        })
    });
    let frames = sight.journey.frames;
    Box(
        spot(PANEL).mesh_surface(move || {
            let sway = (frames.get() as f32 * 0.05).sin() * 0.35;
            let foot = middle - DOWN * PANEL_FOOT;
            let (width, height) = (PANEL.width * PANEL_SCALE, PANEL.height * PANEL_SCALE);
            let transform = Mat4::scaling(Vec3::new(width, height, 1.0))
                .then(&Mat4::translation(Vec3::new(-width * 0.5, -height, 0.0)))
                .then(&Mat4::rotation_y(sway))
                .then(&Mat4::translation(foot));
            MeshSurface {
                mesh: Arc::clone(&plane),
                transform,
                material: sight.material(Finish::Content, &transform, lamps),
                casts_shadow: false,
                solid: true,
            }
        }),
        BoxSpec::default(),
        move || Switchboard(on),
    );
}

#[composable]
fn Switchboard(on: MutableState<u32>) {
    LiquidTheme(
        LiquidThemeSpec::system_with_accent(SchemeMode::Dark, Color(0.9, 0.2, 0.15, 1.0)),
        move || {
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color(0.12, 0.11, 0.10, 1.0))
                    .padding(10.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        "WHO IS HERE?",
                        Modifier::empty().padding(4.0),
                        mono(18.0, Color(0.9, 0.85, 0.7, 1.0)),
                    );
                    for row in 0..2 {
                        Row(Modifier::empty(), RowSpec::default(), move || {
                            for column in 0..SWITCHES / 2 {
                                let bit = 1 << (row * (SWITCHES / 2) + column);
                                LiquidToggle(
                                    Modifier::empty().padding(3.0),
                                    on.get() & bit != 0,
                                    move |checked: bool| {
                                        on.update(|on| {
                                            *on = if checked { *on | bit } else { *on & !bit };
                                        });
                                    },
                                );
                            }
                        });
                    }
                },
            );
        },
    );
}
