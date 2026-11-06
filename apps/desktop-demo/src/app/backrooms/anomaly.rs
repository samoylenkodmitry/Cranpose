// The things that should not be in a room, each a mesh whose content is
// live UI: an eye that turns to watch the eye, a television the entity
// types through, and switches that flip by themselves. Their motion comes
// from infinite transitions that their draw closures read, so it draws
// each frame and composes nothing.

use std::{sync::Arc, time::Duration};

use cranpose::liquid::prelude::{LiquidTheme, LiquidThemeSpec, LiquidToggle, SchemeMode};
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_core::{delay, rememberMutableStateOf, LaunchedEffect, MutableState, State};
use cranpose_ui::{
    composable,
    space::{Mat4, Material, MeshSurface, Vec3},
    text::{FontFamily, TextUnit},
    Alignment, Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec, DrawScope,
    Modifier, Point, Rect, Row, RowSpec, Size, Text, TextStyle, TileMode,
};

use super::{
    finish::{Finish, Sight},
    journey::Journey,
    notes,
    place::{facing, spot},
    plan::Thing,
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
// How far the watcher bobs up and down, and how long it takes one way.
const BOB: f32 = 8.0;
const BOB_MILLIS: u64 = 1300;
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
// How long a letter takes to type.
const LETTER: Duration = Duration::from_millis(83);
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
// How far the poltergeist's panel sways each way, in radians, and how long
// it takes one way.
const SWAY: f32 = 0.35;
const SWAY_MILLIS: u64 = 2100;

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

// A float that runs from `from` to `to` and back over and over, `millis`
// each way, easing at both ends.
#[composable]
fn rememberSwing(label: &str, (from, to): (f32, f32), millis: u64) -> State<f32> {
    rememberInfiniteTransition(label).animateFloat(
        from,
        to,
        infiniteRepeatable(
            AnimationSpec::tween(millis, Easing::EaseInOut),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        label,
    )
}

// The thing standing in the middle of a room, its floor point `middle`.
#[composable(no_skip)]
pub(super) fn Anomaly(sight: Sight, thing: Thing, middle: Vec3) {
    match thing {
        Thing::Watcher => Watcher(sight, middle),
        Thing::Television => Television(sight, middle),
        Thing::Poltergeist => Poltergeist(sight, middle),
    }
}

// An eye that floats over the floor and turns to watch the eye.
#[composable]
fn Watcher(sight: Sight, middle: Vec3) {
    let shut = rememberMutableStateOf(|| false);
    LaunchedEffect(shut.get(), async move |_| {
        if shut.get_non_reactive() {
            delay(BLINK).await;
            shut.set(false);
        }
    });
    let bob = rememberSwing("watcher_bob", (-BOB, BOB), BOB_MILLIS);
    let sphere = Arc::clone(&sight.level.meshes.sphere);
    Box(
        spot(EYEBALL).mesh_surface(move || {
            let center = middle - DOWN * (WATCHER_HEIGHT + bob.value());
            let transform = facing(
                center,
                sight.eye_in_room(),
                Vec3::new(EYEBALL_RADIUS, EYEBALL_RADIUS, EYEBALL_RADIUS),
            );
            MeshSurface {
                mesh: Arc::clone(&sphere),
                transform,
                material: sight.material(Finish::Content, &transform),
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
fn Television(sight: Sight, middle: Vec3) {
    let center = middle - DOWN * TELEVISION_HEIGHT;
    let body = Mat4::scaling(TELEVISION).then(&Mat4::translation(center));
    let lit = sight.clone();
    let cuboid = Arc::clone(&sight.level.meshes.cuboid);
    Box(
        spot(Size {
            width: 4.0,
            height: 4.0,
        })
        .mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&cuboid),
            transform: body,
            material: lit.material(Finish::Content, &body),
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
    let plane = Arc::clone(&sight.level.meshes.plane);
    let (journey, film) = (sight.level.journey, sight.level.film);
    Box(
        spot(SCREEN).mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&plane),
            transform: screen,
            material: Material::Emissive,
            casts_shadow: false,
            solid: false,
        }),
        BoxSpec::default(),
        move || Broadcast(journey, film),
    );
}

// The television's screen: static, and the entity's words typed over it,
// a letter at a time; ANSWER starts the next line. The last line, typed in
// full, sets the entity on the eye.
#[composable]
fn Broadcast(journey: Journey, film: State<f32>) {
    let line = rememberMutableStateOf(|| 0_u32);
    let letters = rememberMutableStateOf(|| 0_usize);
    let words = notes::screen(line.get(), &journey);
    let length = words.chars().count();
    let haunts = notes::haunts(line.get());
    LaunchedEffect(line.get(), async move |_| {
        letters.set(0);
        for typed in 1..=length {
            delay(LETTER).await;
            letters.set(typed);
        }
        if haunts {
            journey.haunt();
        }
    });
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.05, 0.06, 0.07, 1.0))
            .draw_behind(move |scope| static_noise(scope, film.get().to_bits())),
        BoxSpec::default(),
        move || {
            let words = words.clone();
            Column(
                Modifier::empty().fill_max_size().padding(14.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        words.chars().take(letters.get()).collect::<String>(),
                        Modifier::empty().weight(1.0),
                        mono(17.0, Color(0.85, 0.95, 0.88, 1.0)),
                    );
                    Button(
                        Modifier::empty()
                            .rounded_corners(6.0)
                            .background(Color(0.85, 0.95, 0.88, 0.9))
                            .padding(8.0),
                        ButtonSpec::default(),
                        move || line.update(|line| *line += 1),
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
fn Poltergeist(sight: Sight, middle: Vec3) {
    let on = rememberMutableStateOf(|| 0b01_0110_1001_u32);
    LaunchedEffect((), async move |_| {
        let mut flips = 0_u32;
        loop {
            delay(FLIP).await;
            flips = flips.wrapping_add(1);
            let bit = noise(flips, 7) % SWITCHES;
            on.update(|on| *on ^= 1 << bit);
        }
    });
    let sway = rememberSwing("poltergeist_sway", (-SWAY, SWAY), SWAY_MILLIS);
    let plane = Arc::clone(&sight.level.meshes.plane);
    Box(
        spot(PANEL).mesh_surface(move || {
            let foot = middle - DOWN * PANEL_FOOT;
            let (width, height) = (PANEL.width * PANEL_SCALE, PANEL.height * PANEL_SCALE);
            let transform = Mat4::scaling(Vec3::new(width, height, 1.0))
                .then(&Mat4::translation(Vec3::new(-width * 0.5, -height, 0.0)))
                .then(&Mat4::rotation_y(sway.value()))
                .then(&Mat4::translation(foot));
            MeshSurface {
                mesh: Arc::clone(&plane),
                transform,
                material: sight.material(Finish::Content, &transform),
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
