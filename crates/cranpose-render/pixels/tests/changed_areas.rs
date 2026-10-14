//! A frame kept by a host and redrawn only in the areas
//! `PixelsRenderer::update_picture` reports stays equal to a full redraw, frame
//! after frame, while content moves, changes color, text, alpha, shadow and
//! stacking order, scrolls under a clip, and appears and disappears.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_pixels::{Picture, PixelRect, PixelsRenderer};
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, Text, TextStyle};
use cranpose_ui_graphics::{Color, GraphicsLayer};

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
                || {},
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
        },
    );
}

fn shell(scale: f32) -> (AppShell<PixelsRenderer>, Step) {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || Scenes(Rc::clone(&content)),
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

fn kept_frames_match_full_redraws(scale: f32) {
    let (mut shell, captured) = shell(scale);
    let mut picture = Picture::default();
    let mut areas = Vec::new();
    let mut kept = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut full = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut area_pixels = Vec::new();
    for frame in 0..FRAMES {
        if frame > 0 {
            let step = (*captured.borrow()).expect("the composition remembered the step");
            step.set(frame);
        }
        shell.update();
        shell.primary().with_renderer(|renderer| {
            renderer.update_picture(&mut picture, WIDTH, HEIGHT, scale, &mut areas);
            for area in &areas {
                area_pixels.resize(area.width as usize * area.height as usize * 4, 0);
                renderer.draw_picture_area(&picture, *area, &mut area_pixels);
                apply(&mut kept, *area, &area_pixels);
            }
            renderer.draw_scaled(&mut full, WIDTH, HEIGHT, scale);
        });
        assert_eq!(
            first_difference(&kept, &full),
            None,
            "frame {frame} at scale {scale}: the kept frame differs from a full redraw at (x, y); areas {areas:?}"
        );
    }
}

#[test]
fn kept_frames_match_full_redraws_at_one_pixel_per_point() {
    kept_frames_match_full_redraws(1.0);
}

#[test]
fn kept_frames_match_full_redraws_at_fractional_density() {
    kept_frames_match_full_redraws(1.5);
}

#[test]
fn kept_frames_match_full_redraws_at_two_pixels_per_point() {
    kept_frames_match_full_redraws(2.0);
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
