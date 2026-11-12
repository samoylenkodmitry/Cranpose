//! A frame kept by a host and redrawn only in the areas
//! `PixelsRenderer::update_picture` reports stays equal to a full redraw while
//! content moves, resizes, changes color, text, alpha, shadow and stacking
//! order, scrolls under a clip, appears and disappears, arcs sweep, and one
//! public setter of the scene store changes one record alone.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_app_shell::AppShell;
use cranpose_core::{
    MutableState, RecordId, location_key, rememberMovableContentOf, rememberMutableStateOf,
};
use cranpose_macros::composable;
use cranpose_render_common::{
    Renderer,
    scene_store::{ItemRef, LayerRef, RecordKind, SceneStore},
};
use cranpose_render_pixels::{Picture, PixelRect, PixelsRenderer};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Modifier, Size, Text, TextStyle,
    space::{Camera, LayerSpace, Mat4, Material, Mesh, MeshSurface, Scene3d, Vec3},
};
use cranpose_ui_graphics::{
    Brush, Color, ColorFilter, DrawScope, GraphicsLayer, Point, ProjectiveTransform, Rect, Stroke,
    StrokeCap, TAU,
};

#[path = "support/kept_frames.rs"]
mod kept_frames;

use kept_frames::KeptFrames;

const WIDTH: u32 = 240;
const HEIGHT: u32 = 180;
const FRAMES: u32 = 24;

type Step = Rc<RefCell<Option<MutableState<u32>>>>;

fn color(step: u32, salt: u32) -> Color {
    let value = (step * 37 + salt * 91) % 255;
    Color::from_rgb_u8(value as u8, (255 - value) as u8, ((value * 3) % 255) as u8)
}

#[composable]
fn Scenes(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .offset((n * 7 % 150) as f32, 6.0)
                    .width(30.0)
                    .height(18.0)
                    .background(color(n, 1)),
                BoxSpec::default(),
                || {},
            );
            Text(
                format!("Count {n}"),
                Modifier::empty().offset(8.0, 32.0),
                TextStyle::default(),
            );
            Box(
                Modifier::empty()
                    .offset(124.0, 30.0)
                    .width(90.0)
                    .height(52.0)
                    .clip_to_bounds(),
                BoxSpec::default(),
                move || {
                    Column(
                        Modifier::empty().offset(0.0, -((n * 5 % 60) as f32)),
                        ColumnSpec::default(),
                        move || {
                            for row in 0..6 {
                                Text(
                                    format!("Row {row}"),
                                    Modifier::empty().background(color(row, 2)),
                                    TextStyle::default(),
                                );
                            }
                        },
                    );
                },
            );
            Box(
                Modifier::empty()
                    .offset(16.0, 96.0)
                    .width(64.0)
                    .height(40.0)
                    .graphics_layer(move || GraphicsLayer {
                        alpha: 0.3 + (n % 5) as f32 * 0.15,
                        shadow_elevation: if n % 3 == 0 { 8.0 } else { 2.0 },
                        ..GraphicsLayer::default()
                    })
                    .rounded_corners(10.0)
                    .background(color(n / 2, 3)),
                BoxSpec::default(),
                // A child that does not change takes the parent's new alpha.
                || {
                    Box(
                        Modifier::empty()
                            .offset(8.0, 8.0)
                            .width(24.0)
                            .height(12.0)
                            .background(color(4, 7)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            let order = if n % 2 == 0 { [0u32, 1] } else { [1, 0] };
            for which in order {
                Box(
                    Modifier::empty()
                        .offset(100.0 + which as f32 * 14.0, 100.0 + which as f32 * 10.0)
                        .width(36.0)
                        .height(30.0)
                        .background(color(which, 4)),
                    BoxSpec::default(),
                    || {},
                );
            }
            // A rounded fill that moves and resizes on fractional points in
            // both axes, cut by its parent's clip on the left and top.
            Box(
                Modifier::empty()
                    .offset(4.0, 140.0)
                    .width(120.0)
                    .height(36.0)
                    .clip_to_bounds(),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .offset((n % 5) as f32 * 1.7 - 3.0, (n % 3) as f32 * 2.3 - 2.0)
                            .width(40.0 + (n * 11 % 50) as f32 * 0.9)
                            .height(16.0 + (n % 4) as f32 * 1.6)
                            .rounded_corners(7.0)
                            .background(color(7, 6)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Column(
                Modifier::empty().offset(170.0, 100.0),
                ColumnSpec::default(),
                move || {
                    for item in 0..(n % 4) {
                        Box(
                            Modifier::empty()
                                .width(50.0)
                                .height(9.0)
                                .background(color(item, 5)),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
            MovedBox(n);
            GrowingBars(n);
        },
    );
}

/// A box with a text inside that only moves, as the box above it grows: the
/// frame before drew it, at another place.
#[composable]
fn MovedBox(n: u32) {
    Column(
        Modifier::empty().offset(4.0, 52.0),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .width(20.0)
                    .height(4.0 + (n % 5) as f32 * 3.0),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .width(44.0)
                    .height(16.0)
                    .background(color(3, 8)),
                BoxSpec::default(),
                || {
                    Text("moves", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

/// Bars that grow inside rounded bars: an opaque outer bar hides what lies
/// under the strip that changes, a translucent one does not.
#[composable]
fn GrowingBars(n: u32) {
    for (row, outer) in [
        (0u32, Color::from_rgb_u8(40, 44, 64)),
        (1, Color::from_rgba_u8(40, 44, 64, 128)),
    ] {
        let fill = 10.0 + ((n + row * 5) * 13 % 70) as f32;
        Box(
            Modifier::empty()
                .offset(120.0, 140.0 + row as f32 * 20.0)
                .width(100.0)
                .height(16.0)
                .background(outer)
                .rounded_corners(8.0),
            BoxSpec::default(),
            move || {
                Box(
                    Modifier::empty()
                        .width(fill)
                        .height(16.0)
                        .background(Color::from_rgb_u8(98, 160, 255))
                        .rounded_corners(8.0),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    }
}

/// A parent whose alpha only its layer block reads, so a new alpha changes
/// its layer and nothing else, over a child that never changes: the child
/// takes the parent's alpha.
#[composable]
fn FadingParent(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Box(
        Modifier::empty()
            .offset(20.0, 20.0)
            .width(80.0)
            .height(40.0)
            .graphics_layer(move || GraphicsLayer {
                alpha: 0.2 + (step.value() % 5) as f32 * 0.2,
                ..GraphicsLayer::default()
            })
            .background(Color::from_rgb_u8(30, 40, 50)),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .offset(10.0, 10.0)
                    .width(40.0)
                    .height(16.0)
                    .background(Color::from_rgb_u8(200, 120, 40)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// A parent whose layer block alone turns its clip on and off, over a wider
/// child that never changes: the child shows as far as the parent's clip
/// lets it.
#[composable]
fn ClippingParent(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Box(
        Modifier::empty()
            .offset(20.0, 20.0)
            .width(40.0)
            .height(30.0)
            .graphics_layer(move || GraphicsLayer {
                clip: step.value() % 2 == 0,
                ..GraphicsLayer::default()
            }),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .width(120.0)
                    .height(20.0)
                    .background(Color::from_rgb_u8(60, 160, 90)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// A parent that never changes, translucent, tinted and clipped, over child
/// layers that change one at a time: one fades, one moves across the
/// parent's clip, and one stays still over a child of its own that changes
/// colour. Each draws with the parent's alpha, tint and clip.
#[composable]
fn StillParent(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Box(
        Modifier::empty()
            .offset(20.0, 20.0)
            .width(120.0)
            .height(60.0)
            .graphics_layer(|| GraphicsLayer {
                alpha: 0.6,
                clip: true,
                color_filter: Some(ColorFilter::modulate(Color::from_rgb_u8(255, 200, 160))),
                ..GraphicsLayer::default()
            })
            .background(Color::from_rgb_u8(30, 40, 50)),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .offset(6.0, 6.0)
                    .width(30.0)
                    .height(20.0)
                    .graphics_layer(move || GraphicsLayer {
                        alpha: 0.2 + (step.value() % 5) as f32 * 0.2,
                        ..GraphicsLayer::default()
                    })
                    .background(Color::from_rgb_u8(200, 120, 40)),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(44.0, 10.0)
                    .width(40.0)
                    .height(24.0)
                    .graphics_layer(move || GraphicsLayer {
                        translation_x: (step.value() % 4) as f32 * 17.0,
                        translation_y: (step.value() % 3) as f32 * 13.0,
                        ..GraphicsLayer::default()
                    })
                    .background(Color::from_rgb_u8(60, 160, 90)),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(6.0, 32.0)
                    .width(50.0)
                    .height(24.0)
                    .graphics_layer(|| GraphicsLayer {
                        alpha: 0.8,
                        ..GraphicsLayer::default()
                    })
                    .background(Color::from_rgb_u8(90, 90, 160)),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .offset(8.0, 6.0)
                            .width(20.0)
                            .height(12.0)
                            .graphics_layer(GraphicsLayer::default)
                            .background(color(step.value(), 9)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

fn shell(scale: f32, scenes: fn(Step)) -> (AppShell<PixelsRenderer>, Step) {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || scenes(Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32 / scale, HEIGHT as f32 / scale),
        scale,
    );
    (shell, captured)
}

fn apply(kept: &mut [u8], area: PixelRect, pixels: &[u8]) {
    let row_bytes = area.width as usize * 4;
    for (row, source) in pixels.chunks_exact(row_bytes).enumerate() {
        let start = ((area.y as usize + row) * WIDTH as usize + area.x as usize) * 4;
        kept[start..start + row_bytes].copy_from_slice(source);
    }
}

fn first_difference(kept: &[u8], full: &[u8]) -> Option<(usize, usize)> {
    let pixel = kept
        .as_chunks::<4>()
        .0
        .iter()
        .zip(full.as_chunks::<4>().0)
        .position(|(a, b)| a != b)?;
    Some((pixel % WIDTH as usize, pixel / WIDTH as usize))
}

fn kept_frames_match_full_redraws(scale: f32, scenes: fn(Step)) {
    kept_frames_match_full_redraws_taken(scale, scenes, 1);
}

// The kept frame takes the picture at each `every`-th update alone, as a
// host does that draws slower than its scene changes.
fn kept_frames_match_full_redraws_taken(scale: f32, scenes: fn(Step), every: u32) {
    let (mut shell, captured) = shell(scale, scenes);
    KeptFrames::new((WIDTH, HEIGHT), scale).check(&mut shell, (FRAMES, every), |frame| {
        let step = (*captured.borrow()).expect("the composition remembered the step");
        step.set(frame);
    });
}

// Nodes that change by turns among nodes that hold still: two bars under
// still rows, each on its own frames; a box under a still parent whose
// layer comes and goes; and a line that leaves and comes back.
#[composable]
fn Turns(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            for row in 0..5u32 {
                cranpose_core::with_key(&row, || {
                    Text(
                        format!("Still row {row}"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                });
            }
            for bar in 0..2u32 {
                cranpose_core::with_key(&(100 + bar), || {
                    // Each frame gives one of the two bars its next width.
                    let turn = (n + 1 - bar) / 2;
                    Box(
                        Modifier::empty()
                            .width(20.0 + (turn * 13 % 150) as f32)
                            .height(8.0)
                            .background(color(turn, bar)),
                        BoxSpec::default(),
                        || {},
                    );
                });
            }
            Box(
                Modifier::empty()
                    .width(120.0)
                    .height(20.0)
                    .background(Color::from_rgb_u8(20, 20, 20)),
                BoxSpec::default(),
                move || {
                    let inner = Modifier::empty()
                        .width(40.0 + (n % 5) as f32 * 4.0)
                        .height(12.0)
                        .background(color(n / 3, 7));
                    let inner = if (n / 3) % 2 == 1 {
                        inner.graphics_layer(|| GraphicsLayer {
                            alpha: 0.5,
                            ..GraphicsLayer::default()
                        })
                    } else {
                        inner
                    };
                    Box(inner, BoxSpec::default(), || {});
                },
            );
            if n % 4 != 3 {
                Text("Comes and goes", Modifier::empty(), TextStyle::default());
            }
        },
    );
}

#[test]
fn kept_frames_match_full_redraws_while_nodes_change_by_turns() {
    kept_frames_match_full_redraws(1.0, Turns);
    kept_frames_match_full_redraws(1.5, Turns);
}

// The lags lie on both sides of the updates a renderer follows a store
// through before it reads the store whole.
#[test]
fn a_kept_frame_that_skips_updates_matches_a_full_redraw() {
    for every in [2, 3, 7, 8, 9] {
        kept_frames_match_full_redraws_taken(1.0, Turns, every);
        kept_frames_match_full_redraws_taken(1.5, Scenes, every);
    }
}

// How many of the boxes of `ManyBoxes` change at `frame`: counts on both
// sides of the records a store lists as changed in one update, and of half
// that many, as a record may come twice.
fn changing_boxes(frame: u32) -> u32 {
    [126, 127, 128, 129, 130, 254, 255, 256, 257, 258, 300, 3][frame as usize % 12]
}

// 300 boxes, of which the first `changing_boxes` take their next color each
// frame.
#[composable]
fn ManyBoxes(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            for index in 0..300u32 {
                cranpose_core::with_key(&index, || {
                    let changes: u32 = (1..=n)
                        .map(|frame| u32::from(index < changing_boxes(frame)))
                        .sum();
                    Box(
                        Modifier::empty()
                            .offset((index % 30) as f32 * 8.0, (index / 30) as f32 * 6.0)
                            .width(6.0)
                            .height(4.0)
                            .background(color(changes, index)),
                        BoxSpec::default(),
                        || {},
                    );
                });
            }
        },
    );
}

#[test]
fn kept_frames_match_full_redraws_while_hundreds_of_nodes_change_at_once() {
    kept_frames_match_full_redraws(1.0, ManyBoxes);
    kept_frames_match_full_redraws_taken(1.0, ManyBoxes, 2);
}

// Content that moves between a parent that shifts and stretches it and a
// parent that fades it, in updates that also put a sibling before it, take
// one away, and give a keyed box's place to a box of another key.
#[composable]
fn Movers(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    let mover = rememberMovableContentOf(move || {
        Box(
            Modifier::empty()
                .width(30.0)
                .height(10.0)
                .background(color(step.value() / 5, 3)),
            BoxSpec::default(),
            || {},
        );
    });
    let (stretched, faded) = (mover.clone(), mover);
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            let stretched = stretched.clone();
            Column(
                Modifier::empty()
                    .width(120.0)
                    .height(40.0)
                    .graphics_layer(|| GraphicsLayer {
                        translation_x: 12.0,
                        scale_x: 1.5,
                        ..GraphicsLayer::default()
                    }),
                ColumnSpec::default(),
                move || {
                    if n % 3 == 1 {
                        Box(
                            Modifier::empty()
                                .width(20.0)
                                .height(6.0)
                                .background(color(1, 5)),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                    if n % 2 == 0 {
                        stretched.show();
                    }
                },
            );
            let faded = faded.clone();
            Column(
                Modifier::empty()
                    .width(120.0)
                    .height(40.0)
                    .graphics_layer(|| GraphicsLayer {
                        alpha: 0.5,
                        ..GraphicsLayer::default()
                    }),
                ColumnSpec::default(),
                move || {
                    if n % 2 == 1 {
                        faded.show();
                    }
                    let key = n % 4;
                    cranpose_core::with_key(&key, || {
                        Box(
                            Modifier::empty()
                                .width(10.0 + key as f32 * 5.0)
                                .height(6.0)
                                .background(color(key, 9)),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                },
            );
        },
    );
}

#[test]
fn kept_frames_match_full_redraws_while_content_moves_between_parents() {
    for every in [1, 2, 3] {
        kept_frames_match_full_redraws_taken(1.0, Movers, every);
        kept_frames_match_full_redraws_taken(1.5, Movers, every);
    }
}

// A 3D scene with two panels turned in depth and a flat box; one panel
// changes each frame, the other and the flat box by turns.
#[composable]
fn Surfaces(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    let plane = Arc::new(Mesh::plane());
    Box(
        Modifier::empty().fill_max_size().scene3d(|| Scene3d {
            camera: Camera::look_at(
                Vec3::new(0.0, 0.0, -400.0),
                Vec3::ZERO,
                Vec3::new(0.0, 1.0, 0.0),
                400.0,
            ),
            light: None,
            eye: None,
        }),
        BoxSpec::default(),
        move || {
            for panel in 0..2u32 {
                let plane = Arc::clone(&plane);
                cranpose_core::with_key(&panel, || {
                    let turn = if panel == 0 { n } else { n / 3 };
                    Box(
                        Modifier::empty()
                            .required_size(Size {
                                width: 60.0,
                                height: 40.0,
                            })
                            .mesh_surface(move || MeshSurface {
                                mesh: Arc::clone(&plane),
                                transform: Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
                                    .then(&Mat4::scaling(Vec3::new(60.0, 40.0, 1.0)))
                                    .then(&Mat4::rotation_y(0.5))
                                    .then(&Mat4::translation(Vec3::new(
                                        panel as f32 * 110.0 - 55.0,
                                        20.0,
                                        0.0,
                                    ))),
                                material: Material::Unlit,
                                casts_shadow: false,
                                solid: true,
                            })
                            .background(color(turn, panel)),
                        BoxSpec::default(),
                        move || {
                            Box(
                                Modifier::empty()
                                    .width(20.0 + (turn % 4) as f32 * 8.0)
                                    .height(10.0)
                                    .background(color(turn, panel + 4)),
                                BoxSpec::default(),
                                || {},
                            );
                        },
                    );
                });
            }
            Box(
                Modifier::empty()
                    .width(60.0 + (n / 4 % 3) as f32 * 10.0)
                    .height(20.0)
                    .background(color(n / 4, 11)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[test]
fn kept_frames_match_full_redraws_while_a_surface_turned_in_depth_changes() {
    for every in [1, 2] {
        kept_frames_match_full_redraws_taken(1.0, Surfaces, every);
        kept_frames_match_full_redraws_taken(1.5, Surfaces, every);
    }
}

// Two bars whose fills a draw callback draws: the first inside a clip of
// its width every frame, the second inside a clip on two frames of three and
// bare on the third, with a second clip inside the first on every fourth.
#[composable]
fn ClippedBars(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            for bar in 0..2u32 {
                cranpose_core::with_key(&bar, || {
                    Box(
                        Modifier::empty()
                            .width(200.0)
                            .height(16.0)
                            .background(Color::from_rgb_u8(40, 44, 64))
                            .draw_behind(move |scope| {
                                let size = scope.size();
                                let whole = Rect {
                                    x: 0.0,
                                    y: 0.0,
                                    width: size.width,
                                    height: size.height,
                                };
                                let shown = Rect {
                                    width: 20.0 + (n * 13 % 170) as f32,
                                    ..whole
                                };
                                let fill = Brush::solid(color(n, bar));
                                if bar == 1 && n % 3 == 2 {
                                    scope.draw_rect_at(shown, fill);
                                    return;
                                }
                                scope.clip_rect(shown, &mut |scope| {
                                    scope.draw_rect_at(whole, fill.clone());
                                    if bar == 1 && n % 4 == 0 {
                                        let inner = Rect {
                                            height: size.height / 2.0,
                                            ..whole
                                        };
                                        scope.clip_rect(inner, &mut |scope| {
                                            scope.draw_rect_at(whole, Brush::solid(color(n, 7)));
                                        });
                                    }
                                });
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                });
            }
        },
    );
}

#[test]
fn kept_frames_match_full_redraws_while_clipped_draws_change() {
    for every in [1, 2] {
        kept_frames_match_full_redraws_taken(1.0, ClippedBars, every);
        kept_frames_match_full_redraws_taken(1.5, ClippedBars, every);
    }
}

// A fill that stays while the clip around it shrinks a few points each
// frame: the draw group's bounds change and the draws do not.
#[composable]
fn ShrinkingClip(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let shown = 180.0 - step.value() as f32 * 7.0;
    Box(
        Modifier::empty()
            .offset(20.0, 50.0)
            .width(200.0)
            .height(16.0)
            .draw_behind(move |scope| {
                let whole = Rect::from_size(scope.size());
                let fill = Brush::solid(Color::from_rgb_u8(98, 160, 255));
                scope.clip_rect(
                    Rect::from_size(Size::new(shown, whole.height)),
                    &mut |scope| {
                        scope.draw_rect_at(whole, fill.clone());
                    },
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn kept_frames_match_full_redraws_while_a_clip_shrinks_around_a_still_fill() {
    kept_frames_match_full_redraws(1.0, ShrinkingClip);
    kept_frames_match_full_redraws(1.5, ShrinkingClip);
}

#[test]
fn a_clip_shrinking_around_a_still_fill_changes_the_strip_it_uncovers() {
    let (mut shell, captured) = shell(1.0, ShrinkingClip);
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    let step = (*captured.borrow()).expect("the composition remembered the step");
    step.set(1);
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    // The clip's end moves from x = 200 to 193 in the box at x = 20.
    assert!(
        areas.iter().any(|area| {
            area.x <= 193
                && area.x + area.width >= 200
                && area.y <= 50
                && area.y + area.height >= 66
        }),
        "the strip the clip uncovered lies in one area of {areas:?}"
    );
}

// A box that clips a child wider than it and shrinks a few points each
// frame: its bounds change and nothing it draws does.
#[composable]
fn ShrinkingParent(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let shown = 180.0 - step.value() as f32 * 7.0;
    Box(
        Modifier::empty()
            .offset(20.0, 50.0)
            .size(Size::new(shown, 16.0))
            .clip_to_bounds(),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .required_size(Size::new(200.0, 16.0))
                    .background(Color::from_rgb_u8(98, 160, 255)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[test]
fn kept_frames_match_full_redraws_while_a_clipping_box_shrinks_around_a_still_child() {
    kept_frames_match_full_redraws(1.0, ShrinkingParent);
    kept_frames_match_full_redraws(1.5, ShrinkingParent);
}

#[test]
fn kept_frames_match_full_redraws_at_one_pixel_per_point() {
    kept_frames_match_full_redraws(1.0, Scenes);
}

#[test]
fn kept_frames_match_full_redraws_at_fractional_density() {
    kept_frames_match_full_redraws(1.5, Scenes);
}

#[test]
fn kept_frames_match_full_redraws_at_two_pixels_per_point() {
    kept_frames_match_full_redraws(2.0, Scenes);
}

#[test]
fn a_parent_alpha_its_layer_alone_reads_reaches_a_child_that_did_not_change() {
    kept_frames_match_full_redraws(1.0, FadingParent);
}

#[test]
fn a_parent_clip_its_layer_alone_reads_reaches_a_child_that_did_not_change() {
    kept_frames_match_full_redraws(1.0, ClippingParent);
}

#[test]
fn layers_that_change_under_a_still_parent_keep_its_alpha_tint_and_clip() {
    kept_frames_match_full_redraws(1.5, StillParent);
}

// A parent whose padding changes every frame and whose rows hold still:
// a leaf child and a child layer move with the parent's content. A box
// beside the parent comes and goes every other frame, so the picture is
// also built anew from the rows it held.
#[composable]
fn PaddedParent(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let inset = (step.value() % 4) as f32 * 3.0;
    let beside = step.value() % 2 == 1;
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Box(
            Modifier::empty()
                .offset(16.0, 12.0)
                .width(150.0)
                .height(90.0)
                .background(Color::from_rgb_u8(40, 44, 64))
                .padding(inset),
            BoxSpec::default(),
            || {
                Column(Modifier::empty(), ColumnSpec::default(), || {
                    Box(
                        Modifier::empty()
                            .width(60.0)
                            .height(14.0)
                            .background(Color::from_rgb_u8(200, 90, 60)),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .width(40.0)
                            .height(14.0)
                            .graphics_layer(|| GraphicsLayer {
                                alpha: 0.7,
                                ..GraphicsLayer::default()
                            })
                            .background(Color::from_rgb_u8(60, 160, 220)),
                        BoxSpec::default(),
                        || {},
                    );
                });
            },
        );
        if beside {
            Box(
                Modifier::empty()
                    .width(30.0)
                    .height(10.0)
                    .background(Color::from_rgb_u8(90, 200, 90)),
                BoxSpec::default(),
                || {},
            );
        }
    });
}

#[test]
fn children_move_with_a_parent_whose_padding_changes() {
    kept_frames_match_full_redraws(1.0, PaddedParent);
}

#[composable]
fn Counter(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Text(
                "A title that stays",
                Modifier::empty(),
                TextStyle::default(),
            );
            Box(
                Modifier::empty()
                    .width(200.0)
                    .height(60.0)
                    .background(Color::WHITE),
                BoxSpec::default(),
                || {},
            );
            Text(
                format!("{} s", step.value()),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

#[test]
fn a_still_frame_changes_nothing_and_a_new_count_changes_only_its_text() {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || Counter(Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
    );
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    assert_eq!(
        areas,
        [PixelRect {
            x: 0,
            y: 0,
            width: WIDTH,
            height: HEIGHT
        }]
    );

    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    assert!(areas.is_empty(), "a still frame changed {areas:?}");

    let step = (*captured.borrow()).expect("the composition remembered the step");
    step.set(7);
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    let changed: u32 = areas.iter().map(|area| area.width * area.height).sum();
    assert!(!areas.is_empty());
    assert!(
        areas.iter().all(|area| area.y >= 70),
        "only the count below the box changes: {areas:?}"
    );
    assert!(
        changed < WIDTH * HEIGHT / 10,
        "the count's area is small: {changed} pixels in {areas:?}"
    );
}

#[composable]
fn ProgressBar(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let width = 40.0 + step.value() as f32 * 3.0;
    Box(
        Modifier::empty()
            .offset(20.0, 50.0)
            .width(200.0)
            .height(16.0)
            .background(Color::from_rgb_u8(40, 44, 64))
            .rounded_corners(8.0),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .width(width)
                    .height(16.0)
                    .background(Color::from_rgb_u8(98, 160, 255))
                    .rounded_corners(8.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[test]
fn a_growing_bar_changes_only_the_strip_at_its_end() {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || ProgressBar(Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
    );
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });

    let step = (*captured.borrow()).expect("the composition remembered the step");
    step.set(1);
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    // The bar's end moves from x = 60 to 63, and its corners reach 8 in.
    assert_eq!(
        areas,
        [PixelRect {
            x: 52,
            y: 50,
            width: 11,
            height: 16
        }]
    );
}

#[composable]
fn SpreadChanges(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .offset(10.0, 10.0)
                    .width(20.0)
                    .height(10.0)
                    .background(color(step.value(), 1)),
                BoxSpec::default(),
                || {},
            );
            for index in 0..10u32 {
                cranpose_core::with_key(&index, || {
                    Box(
                        Modifier::empty()
                            .offset(10.0 + index as f32 * 22.0, 160.0)
                            .width(12.0)
                            .height(10.0)
                            .background(color(step.value(), index + 2)),
                        BoxSpec::default(),
                        || {},
                    );
                });
            }
        },
    );
}

#[test]
fn many_changes_far_apart_leave_the_still_pixels_between_them() {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || SpreadChanges(Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
    );
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });

    let step = (*captured.borrow()).expect("the composition remembered the step");
    step.set(1);
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    assert!(
        areas
            .iter()
            .all(|area| area.y + area.height <= 20 || area.y >= 160),
        "eleven boxes change at the top and the bottom; the rows between them stay: {areas:?}"
    );
}

#[composable]
fn GrowingList(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .width(40.0)
                    .height(10.0)
                    .background(color(step.value(), 1)),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .width(200.0)
                    .height(100.0)
                    .background(Color::WHITE),
                BoxSpec::default(),
                || {},
            );
            for row in 0..2 + step.value() {
                cranpose_core::with_key(&row, || {
                    Box(
                        Modifier::empty()
                            .width(30.0)
                            .height(10.0)
                            .background(color(row, 3)),
                        BoxSpec::default(),
                        || {},
                    );
                });
            }
        },
    );
}

#[test]
fn a_row_added_below_still_content_leaves_that_content() {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || GrowingList(Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
    );
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });

    let step = (*captured.borrow()).expect("the composition remembered the step");
    step.set(1);
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    assert!(
        areas
            .iter()
            .all(|area| area.y + area.height <= 10 || area.y >= 110),
        "the top box changes and a row is added; the white box between them stays: {areas:?}"
    );
}

// A ring over its track, from `start` by `sweep`, as a gauge draws it.
fn ring(scope: &mut dyn DrawScope, center: Point, cap: StrokeCap, start: f32, sweep: f32) {
    let stroke = Stroke::new(9.0);
    scope.draw_circle_stroked(
        Brush::solid(Color::from_rgb_u8(40, 44, 64)),
        center,
        30.0,
        stroke,
    );
    scope.draw_arc(
        Brush::solid(Color::from_rgba_u8(98, 160, 255, 200)),
        center,
        30.0,
        start,
        sweep,
        Stroke { cap, ..stroke },
    );
}

// The sweep at `step`: a fifth of a turn growing to over half a turn and
// back.
fn sweep(step: u32) -> f32 {
    0.4 + 0.37 * step.min(FRAMES - step) as f32
}

// Rings whose sweeps grow past half a turn and shrink, with each cap; the
// third sweeps counter-clockwise, so its start moves.
#[composable]
fn GrowingRings(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    Box(
        Modifier::empty().fill_max_size().draw_behind(move |scope| {
            for (index, (cap, direction)) in [
                (StrokeCap::Round, 1.0),
                (StrokeCap::Butt, 1.0),
                (StrokeCap::Square, -1.0),
            ]
            .into_iter()
            .enumerate()
            {
                let center = Point::new(40.0 + index as f32 * 78.0, 60.3);
                let start = -TAU / 4.0 + index as f32 * 0.7;
                ring(scope, center, cap, start, direction * sweep(n));
            }
        }),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn kept_frames_match_full_redraws_while_arcs_sweep() {
    for scale in [1.0, 1.5, 2.0] {
        kept_frames_match_full_redraws(scale, GrowingRings);
    }
}

#[composable]
fn GrowingRing(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    let n = step.value();
    Box(
        Modifier::empty().fill_max_size().draw_behind(move |scope| {
            ring(
                scope,
                Point::new(60.0, 60.0),
                StrokeCap::Round,
                -TAU / 4.0,
                sweep(n),
            );
        }),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn a_growing_ring_changes_only_the_area_around_its_end() {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || GrowingRing(Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
    );
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });

    let step = (*captured.borrow()).expect("the composition remembered the step");
    step.set(1);
    shell.update();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
    });
    // The end moves from 23 to 44 degrees past the top, clockwise: the ring
    // and its track left of the top stay.
    let changed: u32 = areas.iter().map(|area| area.width * area.height).sum();
    assert!(!areas.is_empty());
    assert!(
        areas
            .iter()
            .all(|area| area.x >= 60 && area.y + area.height <= 60),
        "only the top right quarter changes: {areas:?}"
    );
    assert!(
        changed < 30 * 30,
        "the area around the end is small: {changed} pixels in {areas:?}"
    );
}

// 132 boxes that touch no other, in a grid of 12 columns and 11 rows, with
// sizes that repeat; two in three change each frame.
#[composable]
fn ScatteredBoxes(captured: Step) {
    let step = rememberMutableStateOf(|| 0u32);
    *captured.borrow_mut() = Some(step);
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            for index in 0..132u32 {
                cranpose_core::with_key(&index, || {
                    let (column, row) = (index % 12, index / 12);
                    Box(
                        Modifier::empty()
                            .offset(4.0 + column as f32 * 19.0, 4.0 + row as f32 * 15.0)
                            .width(6.0 + (index * 7 % 9) as f32)
                            .height(4.0 + (index * 5 % 7) as f32)
                            .background(color((2 * step.value() + index) / 3, index)),
                        BoxSpec::default(),
                        || {},
                    );
                });
            }
        },
    );
}

#[test]
fn many_changed_boxes_join_into_the_same_areas_frame_after_frame() {
    let (mut shell, captured) = shell(1.0, ScatteredBoxes);
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    let mut seen = Vec::new();
    for frame in 0..5 {
        if frame > 0 {
            let step = (*captured.borrow()).expect("the composition remembered the step");
            step.set(frame);
        }
        shell.update();
        shell.primary().with_renderer(|renderer| {
            renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
        });
        if frame > 0 {
            seen.push(
                areas
                    .iter()
                    .map(|area| (area.x, area.y, area.width, area.height))
                    .collect::<Vec<_>>(),
            );
        }
    }
    // The areas as the greedy join of the two areas that add the fewest
    // still pixels gives them, in the order it leaves them: the pattern
    // repeats every three frames.
    let first = vec![
        (194, 4, 33, 160),
        (80, 94, 84, 7),
        (80, 34, 33, 54),
        (137, 109, 33, 54),
        (80, 109, 33, 53),
        (137, 4, 33, 85),
        (23, 34, 33, 127),
        (23, 4, 90, 22),
    ];
    let expected = vec![
        first.clone(),
        vec![
            (4, 4, 12, 159),
            (42, 4, 31, 160),
            (156, 4, 28, 21),
            (99, 4, 31, 51),
            (213, 4, 14, 158),
            (156, 139, 28, 22),
            (156, 34, 31, 96),
            (99, 64, 31, 96),
        ],
        vec![
            (118, 139, 32, 24),
            (118, 4, 29, 6),
            (137, 19, 13, 113),
            (118, 19, 12, 115),
            (61, 4, 32, 160),
            (175, 4, 32, 81),
            (4, 4, 32, 159),
            (175, 94, 32, 70),
        ],
        first,
    ];
    assert_eq!(seen, expected);
}

// A box that clips a wider child, a box of its own and a faded box with a
// child that handles clicks: records that a write of the scene store
// changes alone.
#[composable]
fn ClippedPair(_captured: Step) {
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .offset(20.0, 20.0)
                    .width(80.0)
                    .height(60.0)
                    .clip_to_bounds()
                    .background(color(1, 2)),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .offset(10.0, 10.0)
                            .width(120.0)
                            .height(30.0)
                            .background(color(3, 4)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .offset(140.0, 100.0)
                    .width(40.0)
                    .height(40.0)
                    .background(color(5, 6)),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(20.0, 100.0)
                    .width(60.0)
                    .height(50.0)
                    .graphics_layer(|| GraphicsLayer {
                        alpha: FADED_ALPHA,
                        ..GraphicsLayer::default()
                    })
                    .clickable(|_| {})
                    .background(color(7, 8)),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .offset(5.0, 5.0)
                            .width(30.0)
                            .height(20.0)
                            .background(color(9, 10)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

// The record of `ClippedPair` that draws `size`.
fn record_of_size(store: &SceneStore, size: Size) -> RecordId {
    fn find(layer: LayerRef<'_>, size: Size) -> Option<RecordId> {
        if layer.size() == size {
            return Some(layer.id());
        }
        layer.items().find_map(|item| match item {
            ItemRef::Layer(child) | ItemRef::Leaf(child) => find(child, size),
            _ => None,
        })
    }
    let root = store.root_layer().expect("the scene has a root layer");
    find(root, size).expect("a record draws that size")
}

const CLIP: Size = Size {
    width: 80.0,
    height: 60.0,
};
const SIDE: Size = Size {
    width: 40.0,
    height: 40.0,
};
const FADED: Size = Size {
    width: 60.0,
    height: 50.0,
};
const FADED_ALPHA: f32 = 0.8;

// What a store write alone does to the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Seen {
    // Some pixel changes.
    Changed,
    // Every pixel stays.
    Same,
}

// Draws `ClippedPair` into a kept frame, runs `write` alone in an update of
// the scene's records, and asserts that the write does to the picture what
// `seen` says and that the frame kept from the areas the renderer reports
// equals a full redraw.
fn a_store_write_alone_keeps_the_frame_whole(
    label: &str,
    seen: Seen,
    write: impl FnOnce(&mut SceneStore),
) {
    let (mut shell, _) = shell(1.0, ClippedPair);
    shell.update();
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    let mut kept = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut full = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut before = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut area_pixels = Vec::new();
    shell.primary().with_renderer(|renderer| {
        let mut keep = |renderer: &mut PixelsRenderer, kept: &mut [u8]| {
            renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut areas);
            for area in &areas {
                area_pixels.resize(area.width as usize * area.height as usize * 4, 0);
                renderer.draw_picture_area(&picture, *area, &mut area_pixels);
                apply(kept, *area, &area_pixels);
            }
        };
        keep(renderer, &mut kept);
        before.copy_from_slice(&kept);
        renderer.scene_mut().edit_store(write);
        keep(renderer, &mut kept);
        renderer.draw_scaled(&mut full, WIDTH, HEIGHT, 1.0);
    });
    assert_eq!(
        first_difference(&before, &full).map_or(Seen::Same, |_| Seen::Changed),
        seen,
        "{label}: what the write does to the picture"
    );
    assert_eq!(
        first_difference(&kept, &full),
        None,
        "{label}: the kept frame differs from a full redraw at (x, y); areas {areas:?}"
    );
}

#[test]
fn a_record_moved_alone_redraws() {
    a_store_write_alone_keeps_the_frame_whole("set_placement", Seen::Changed, |store| {
        let record = record_of_size(store, SIDE);
        let origin = store.layer(record).origin_in_parent();
        store.set_placement(record, Point::new(origin.x - 30.0, origin.y + 10.0), None);
    });
}

#[test]
fn a_record_turned_alone_redraws() {
    a_store_write_alone_keeps_the_frame_whole("set_placement", Seen::Changed, |store| {
        let record = record_of_size(store, CLIP);
        let origin = store.layer(record).origin_in_parent();
        let transform = ProjectiveTransform::translation(origin.x + 25.0, origin.y + 5.0);
        store.set_placement(record, origin, Some(transform));
    });
}

#[test]
fn a_record_that_stops_handling_clicks_alone_keeps_its_pixels() {
    a_store_write_alone_keeps_the_frame_whole("set_hit_test", Seen::Same, |store| {
        let record = record_of_size(store, FADED);
        let root = store.root().expect("the scene has a root");
        assert!(
            store.layer(root).has_hit_targets(),
            "the faded box handles clicks"
        );
        store.set_hit_test(record, None);
        assert!(
            !store.layer(root).has_hit_targets(),
            "set_hit_test: no record handles clicks"
        );
    });
}

#[test]
fn a_record_that_opens_a_space_alone_keeps_its_pixels() {
    a_store_write_alone_keeps_the_frame_whole("set_graphics_layer", Seen::Same, |store| {
        let record = record_of_size(store, FADED);
        let root = store.root().expect("the scene has a root");
        assert!(!store.layer(root).has_space(), "no record holds a space");
        store.set_graphics_layer(
            record,
            Some(GraphicsLayer {
                alpha: FADED_ALPHA,
                space: Some(Arc::new(LayerSpace::Transform(Mat4::IDENTITY))),
                ..GraphicsLayer::default()
            }),
        );
        assert!(
            store.layer(root).has_space(),
            "set_graphics_layer: the faded box holds a space"
        );
    });
}

#[test]
fn a_record_whose_content_shifts_alone_redraws() {
    a_store_write_alone_keeps_the_frame_whole("set_content_offset", Seen::Changed, |store| {
        let record = record_of_size(store, CLIP);
        store.set_content_offset(record, Point::new(-6.0, 8.0));
    });
}

#[test]
fn a_record_faded_alone_redraws() {
    a_store_write_alone_keeps_the_frame_whole("set_graphics_layer", Seen::Changed, |store| {
        let record = record_of_size(store, CLIP);
        store.set_graphics_layer(
            record,
            Some(GraphicsLayer {
                alpha: 0.5,
                ..GraphicsLayer::default()
            }),
        );
    });
}

#[test]
fn a_record_of_another_kind_alone_redraws() {
    a_store_write_alone_keeps_the_frame_whole("set_kind", Seen::Same, |store| {
        let record = record_of_size(store, SIDE);
        let layer = store.layer(record);
        assert_eq!(layer.kind(), RecordKind::Leaf, "the plain box is a leaf");
        store.set_kind(record, RecordKind::Layer, layer.node_id());
    });
}
