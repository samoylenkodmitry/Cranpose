use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Alignment, Box, BoxSpec, Color, CompositingStrategy, GraphicsLayer, Modifier, Text, TextStyle,
    composable,
    text::{SpanStyle, TextUnit},
};

use crate::support;

const WIDTH: u32 = 240;
const HEIGHT: u32 = 120;
const HALF: usize = (WIDTH / 2) as usize;
const PAGE: Color = Color(1.0, 1.0, 1.0, 1.0);

/// One of the two cards: where it sits, how it is scaled and turned, and its
/// colour. The cards' scales differ, so their surfaces rasterize at scales of
/// their own.
struct Card {
    x: f32,
    scale: f32,
    degrees: f32,
    color: Color,
}

const CARDS: [Card; 2] = [
    Card {
        x: 20.0,
        scale: 0.9,
        degrees: 20.0,
        color: Color(0.85, 0.25, 0.30, 1.0),
    },
    Card {
        x: 140.0,
        scale: 0.7,
        degrees: -35.0,
        color: Color(0.20, 0.55, 0.85, 1.0),
    },
];

#[composable]
fn Cards(shown: [bool; 2], label: MutableState<u32>) {
    Box(
        Modifier::empty()
            .size_points(WIDTH as f32, HEIGHT as f32)
            .background(PAGE),
        BoxSpec::default(),
        move || {
            for card in CARDS
                .iter()
                .zip(shown)
                .filter_map(|(card, shown)| shown.then_some(card))
            {
                let (scale, degrees, color) = (card.scale, card.degrees, card.color);
                Box(
                    Modifier::empty()
                        .offset(card.x, 20.0)
                        .size_points(80.0, 80.0)
                        .graphics_layer_value(GraphicsLayer {
                            scale_x: scale,
                            scale_y: scale,
                            rotation_z: degrees,
                            compositing_strategy: CompositingStrategy::Offscreen,
                            ..Default::default()
                        })
                        .background(color)
                        .rounded_corners(10.0),
                    BoxSpec::new().content_alignment(Alignment::CENTER),
                    move || {
                        Text(
                            format!("card {}", label.get()),
                            Modifier::empty(),
                            TextStyle::from_span_style(SpanStyle {
                                color: Some(Color::WHITE),
                                font_size: TextUnit::Sp(16.0),
                                ..Default::default()
                            }),
                        );
                    },
                );
            }
        },
    );
}

struct CardsHarness {
    shell: AppShell<WgpuRenderer>,
    label: Rc<RefCell<Option<MutableState<u32>>>>,
}

impl CardsHarness {
    fn new(renderer: WgpuRenderer, shown: [bool; 2]) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let label: Rc<RefCell<Option<MutableState<u32>>>> = Rc::new(RefCell::new(None));
        let label_for_app = Rc::clone(&label);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = cranpose_core::rememberMutableStateOf(|| 0u32);
            *label_for_app.borrow_mut() = Some(state);
            Cards(shown, state);
        });
        shell.set_viewport(WIDTH as f32, HEIGHT as f32);
        shell.set_buffer_size(WIDTH, HEIGHT);
        shell.update();
        Self { shell, label }
    }

    /// Settles the cards' pipelines, then relabels them so the measured
    /// frame renders their surfaces afresh.
    fn relabeled_frame(&mut self) -> (RenderStatsSnapshot, CapturedFrame) {
        support::settle(|| support::update_and_capture(&mut self.shell, WIDTH, HEIGHT));
        support::wait_for_background_compiler_idle();
        let label = self
            .label
            .borrow()
            .as_ref()
            .copied()
            .expect("label captured");
        self.shell.debug_enter_app_context(|| label.set(1));
        support::update_and_capture(&mut self.shell, WIDTH, HEIGHT)
    }
}

fn half(frame: &CapturedFrame, right: bool) -> Vec<u8> {
    let row_bytes = WIDTH as usize * 4;
    let start = if right { HALF * 4 } else { 0 };
    frame
        .pixels
        .chunks_exact(row_bytes)
        .flat_map(|row| row[start..start + HALF * 4].iter().copied())
        .collect()
}

#[test]
fn surfaces_at_different_scales_share_one_atlas_pass_and_draw_as_they_do_alone() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (both_stats, both) = CardsHarness::new(renderer, [true, true]).relabeled_frame();
    let alone = |shown| {
        CardsHarness::new(
            support::headless_renderer_beside_locked().expect("reference renderer"),
            shown,
        )
        .relabeled_frame()
    };
    let (left_stats, left) = alone([true, false]);
    let (_, right) = alone([false, true]);
    assert!(
        support::pipelines_settled(&both_stats),
        "the measured frame drew with stand-in pipelines: {both_stats:?}"
    );
    assert_eq!(both_stats.isolated_layer_renders, 2);
    assert_eq!(left_stats.isolated_layer_renders, 1);
    assert_eq!(
        both_stats.pass_count, left_stats.pass_count,
        "two surfaces at scales of their own render in one atlas pass, no more passes \
         than one surface alone"
    );
    support::assert_same_bytes(
        "left card",
        WIDTH / 2,
        &half(&left, false),
        &half(&both, false),
    );
    support::assert_same_bytes(
        "right card",
        WIDTH / 2,
        &half(&right, true),
        &half(&both, true),
    );
}

/// Tiles each composited through a surface of its own, `side` points square.
/// The odd tiles take a shade of blue from `phase`.
#[composable]
fn Tiles(side: MutableState<f32>, phase: MutableState<u32>) {
    Box(
        Modifier::empty()
            .size_points(WIDTH as f32, HEIGHT as f32)
            .background(PAGE),
        BoxSpec::default(),
        move || {
            for index in 0..6u8 {
                let side = side.get();
                let shade = if index % 2 == 1 { phase.get() % 8 } else { 0 };
                Box(
                    Modifier::empty()
                        .offset(8.0 + f32::from(index) * 38.0, 20.0)
                        .size_points(side, side)
                        .graphics_layer_value(GraphicsLayer {
                            compositing_strategy: CompositingStrategy::Offscreen,
                            ..Default::default()
                        })
                        .background(Color(
                            0.15 * f32::from(index),
                            0.4,
                            0.8 - 0.05 * shade as f32,
                            1.0,
                        ))
                        .rounded_corners(6.0),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

/// The tiles' side and the odd tiles' phase.
type TileStates = (MutableState<f32>, MutableState<u32>);

fn tiles_shell(
    renderer: WgpuRenderer,
    side: f32,
    density: u32,
) -> (AppShell<WgpuRenderer>, TileStates) {
    let state: Rc<RefCell<Option<TileStates>>> = Rc::new(RefCell::new(None));
    let state_for_app = Rc::clone(&state);
    let mut shell = AppShell::new_with_size_and_density(
        renderer,
        location_key(file!(), line!(), column!()),
        move || {
            let side = cranpose_core::rememberMutableStateOf(|| side);
            let phase = cranpose_core::rememberMutableStateOf(|| 0u32);
            *state_for_app.borrow_mut() = Some((side, phase));
            Tiles(side, phase);
        },
        (WIDTH * density, HEIGHT * density),
        (WIDTH as f32, HEIGHT as f32),
        density as f32,
    );
    shell.update();
    let states = state
        .borrow()
        .as_ref()
        .copied()
        .expect("tile states captured");
    (shell, states)
}

/// Resizes the still tiles twice, checks the second resize draws what a
/// fresh renderer draws and returns the new textures each resize took;
/// none when there is no GPU. The tiles' kept atlas stays until the frame
/// that replaces them ends, so the first resize may take one texture beside
/// it; the second finds that atlas back in the pool.
fn resize_tiles_twice(first: f32, second: f32) -> Option<(u32, u32)> {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping (headless WGPU init failed)");
        return None;
    };
    let (mut shell, (side, _)) = tiles_shell(renderer, 36.0, 1);
    support::settle(|| support::update_and_capture(&mut shell, WIDTH, HEIGHT));
    support::wait_for_background_compiler_idle();
    shell.debug_enter_app_context(|| side.set(first));
    let (stats, _) = support::update_and_capture(&mut shell, WIDTH, HEIGHT);
    assert_eq!(stats.isolated_layer_renders, 6);
    let first_news = stats.offscreen_news;
    shell.debug_enter_app_context(|| side.set(second));
    let (stats, resized) = support::update_and_capture(&mut shell, WIDTH, HEIGHT);
    assert_eq!(stats.isolated_layer_renders, 6);
    let reference = {
        let (mut fresh, _) = tiles_shell(
            support::headless_renderer_beside_locked().expect("reference renderer"),
            second,
            1,
        );
        support::settle(|| support::update_and_capture(&mut fresh, WIDTH, HEIGHT))
    };
    support::assert_same_bytes("resized tiles", WIDTH, &reference.pixels, &resized.pixels);
    Some((first_news, stats.offscreen_news))
}

#[test]
fn an_atlas_whose_surfaces_shrink_to_a_third_draws_into_the_texture_it_had() {
    let Some((first, second)) = resize_tiles_twice(20.0, 19.0) else {
        return;
    };
    assert!(
        first <= 1,
        "the smaller atlas takes one texture beside the kept one, not {first}"
    );
    assert_eq!(
        second, 0,
        "the next atlas takes the texture the larger one drew into"
    );
}

#[test]
fn an_atlas_whose_surfaces_grow_a_little_takes_one_texture_at_most() {
    let Some((first, second)) = resize_tiles_twice(40.0, 41.0) else {
        return;
    };
    assert!(
        first <= 1,
        "the grown atlas takes one texture beside the kept one, not {first}"
    );
    assert!(
        second <= 1,
        "a step of growth takes one texture of its own size on Metal and none \
         where new atlases take a step of headroom, not {second}"
    );
}

/// Tiles that hold still are kept and drawn from the cache as a fresh
/// renderer draws them. A copy-free renderer keeps them where their atlas
/// drew them, at no copy; another copies each out of the shared atlas.
#[test]
fn kept_atlas_surfaces_are_read_in_place_without_copies() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (mut shell, _) = tiles_shell(renderer, 36.0, 1);
    let (mut copies, mut hits) = (0, 0);
    let mut frame = || {
        let (stats, captured) = support::update_and_capture(&mut shell, WIDTH, HEIGHT);
        copies += stats.copy_count;
        hits += stats.layer_cache_hits;
        (stats, captured)
    };
    support::settle(&mut frame);
    support::wait_for_background_compiler_idle();
    frame();
    let (_, kept) = frame();
    assert!(
        hits > 0,
        "the still tiles are kept and drawn from the cache"
    );
    if support::renders_copy_free() {
        assert_eq!(
            copies, 0,
            "kept surfaces stay in the atlas they were drawn in"
        );
    } else {
        assert!(copies > 0, "kept surfaces are copied out of the atlas");
    }

    let (mut fresh, _) = tiles_shell(
        support::headless_renderer_beside_locked().expect("reference renderer"),
        36.0,
        1,
    );
    let reference = support::settle(|| support::update_and_capture(&mut fresh, WIDTH, HEIGHT));
    support::assert_same_bytes("kept tiles", WIDTH, &reference.pixels, &kept.pixels);
}

/// The odd tiles change every frame from the third on, as a starting
/// screen's animations do: a renderer that copies kept surfaces out of the
/// shared atlas copies none of them out, since the second frame repeated
/// the first, and keeps the still tiles once they held still for two frames
/// after their first.
#[test]
fn a_starting_screens_animated_atlas_members_take_no_copies() {
    const FRAMES: u32 = 8;
    let Ok((_lock, renderer)) = support::headless_renderer_parts_copying_uploads() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (mut shell, (_, phase)) = tiles_shell(renderer, 30.0, 1);
    let mut copies = Vec::new();
    let mut last = None;
    for frame in 0..FRAMES {
        if frame >= 2 {
            shell.debug_enter_app_context(|| phase.set(frame));
        }
        let (stats, _) = support::update_and_capture(&mut shell, WIDTH, HEIGHT);
        assert!(
            support::pipelines_settled(&stats),
            "frame {frame} drew with stand-in pipelines: {stats:?}"
        );
        copies.push(stats.copy_count);
        last = Some(stats);
    }
    assert_eq!(
        copies,
        [0, 0, 3, 0, 0, 0, 0, 0],
        "the three still tiles are copied out once, on their third frame"
    );
    let last = last.expect("frames rendered");
    assert_eq!(
        last.layer_cache_hits, 3,
        "the still tiles draw from the cache"
    );
    assert_eq!(last.isolated_layer_renders, 3, "the changing tiles render");
}

/// A host that builds its shell at a density and sets nothing more, as a
/// headless bench or an embedder does, gets surfaces rasterized at that
/// density, as on a device: the first tile, 30 points wide from x = 8, ends
/// at device pixel 114 with the gap to the next tile after it, and each
/// surface holds at least the tile's 90 by 90 device pixels.
#[test]
fn a_shell_built_at_a_density_rasterizes_its_surfaces_at_that_density() {
    const DENSITY: u32 = 3;
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (width, height) = (WIDTH * DENSITY, HEIGHT * DENSITY);
    let (mut shell, (side, _)) = tiles_shell(renderer, 36.0, DENSITY);
    support::settle(|| support::update_and_capture(&mut shell, width, height));
    support::wait_for_background_compiler_idle();
    shell.debug_enter_app_context(|| side.set(30.0));
    let (stats, frame) = support::update_and_capture(&mut shell, width, height);
    assert_eq!(stats.isolated_layer_renders, 6);
    let side_px = u64::from(30 * DENSITY);
    assert!(
        stats.isolated_layer_pixels >= 6 * side_px * side_px,
        "six surfaces of {side_px} by {side_px} device pixels: {stats:?}"
    );
    let pixel = |x: u32, y: u32| {
        let at = ((y * width + x) * 4) as usize;
        &frame.pixels[at..at + 4]
    };
    let middle = (20 + 15) * DENSITY;
    let page = pixel(2, 2);
    assert_ne!(
        pixel(112, middle),
        page,
        "the first tile reaches device pixel 114"
    );
    assert_eq!(
        pixel(116, middle),
        page,
        "the gap after the first tile starts at device pixel 114"
    );
}

/// How many cards draw on each frame of [`changing_cards_shell`]'s cycle:
/// rows of five scrolling through a list change one row or two a frame.
const CHANGING_COUNTS: [u32; 7] = [5, 10, 10, 10, 5, 5, 10];
/// Every this many frames, four small avatars draw instead of the cards:
/// further apart than a texture stays pooled unused.
const AVATAR_FRAMES: u32 = 150;
/// The frames a card's width holds before it steps, as a list whose width
/// follows the frame measures its cards a point wider or narrower.
const WIDTH_STEP_FRAMES: u32 = 20;
const WIDTH_STEPS: u32 = 6;
const CARDS_WIDTH: u32 = 330;
const CARDS_HEIGHT: u32 = 176;
const CARDS_DENSITY: u32 = 3;

/// `count` cards composited through surfaces of their own, each a colour
/// that follows `frame`, so every card draws again on every frame, and a
/// point wider each [`WIDTH_STEP_FRAMES`] frames up to [`WIDTH_STEPS`]
/// points, then narrow again; four avatars of their own instead every
/// [`AVATAR_FRAMES`] frames.
#[composable]
fn ChangingCards(count: MutableState<u32>, frame: MutableState<u32>) {
    Box(
        Modifier::empty()
            .size_points(CARDS_WIDTH as f32, CARDS_HEIGHT as f32)
            .background(PAGE),
        BoxSpec::default(),
        move || {
            let frame = frame.get();
            let width = 58.0 + ((frame / WIDTH_STEP_FRAMES) % WIDTH_STEPS) as f32;
            let count = count.get();
            let (count, width, height) = if frame.is_multiple_of(AVATAR_FRAMES) {
                (4, 16.0, 16.0)
            } else {
                (count, width, 83.0)
            };
            for index in 0..count {
                let shade = ((frame + index) % 8) as f32 / 8.0;
                Box(
                    Modifier::empty()
                        .offset(
                            2.0 + (index % 5) as f32 * 65.0,
                            4.0 + (index / 5) as f32 * 86.0,
                        )
                        .size_points(width, height)
                        .graphics_layer_value(GraphicsLayer {
                            compositing_strategy: CompositingStrategy::Offscreen,
                            ..Default::default()
                        })
                        .background(Color(shade, 0.4, 0.8, 1.0))
                        .rounded_corners(6.0),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

/// The number of cards drawn and the frame they follow.
type CardStates = (MutableState<u32>, MutableState<u32>);

fn changing_cards_shell(
    renderer: WgpuRenderer,
) -> (AppShell<WgpuRenderer>, MutableState<u32>, MutableState<u32>) {
    let states: Rc<RefCell<Option<CardStates>>> = Rc::new(RefCell::new(None));
    let states_for_app = Rc::clone(&states);
    let mut shell = AppShell::new_with_size_and_density(
        renderer,
        location_key(file!(), line!(), column!()),
        move || {
            let count = cranpose_core::rememberMutableStateOf(|| CHANGING_COUNTS[0]);
            let frame = cranpose_core::rememberMutableStateOf(|| 0u32);
            *states_for_app.borrow_mut() = Some((count, frame));
            ChangingCards(count, frame);
        },
        (CARDS_WIDTH * CARDS_DENSITY, CARDS_HEIGHT * CARDS_DENSITY),
        (CARDS_WIDTH as f32, CARDS_HEIGHT as f32),
        CARDS_DENSITY as f32,
    );
    shell.update();
    let (count, frame) = states.borrow().as_ref().copied().expect("states captured");
    (shell, count, frame)
}

/// Frames that draw a varying number of surfaces again keep drawing them into
/// the atlas texture the busiest of them needed, the avatars' frames too:
/// none takes a texture of its own once those numbers have been seen,
/// however long the cycle runs, and the last frame draws what a fresh
/// renderer draws. The frame after the avatars' is left out: its cards are
/// new layers again, and the layer cache may keep one of them at once.
#[test]
fn surfaces_drawn_again_in_varying_numbers_keep_one_atlas_texture() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (width, height) = (CARDS_WIDTH * CARDS_DENSITY, CARDS_HEIGHT * CARDS_DENSITY);
    let (mut shell, count, frame) = changing_cards_shell(renderer);
    let step = |shell: &mut AppShell<WgpuRenderer>, index: u32| {
        shell.debug_enter_app_context(|| {
            count.set(CHANGING_COUNTS[index as usize % CHANGING_COUNTS.len()]);
            frame.set(index);
        });
        support::update_and_capture(shell, width, height)
    };
    let mut index = 0;
    support::settle(|| {
        index += 1;
        step(&mut shell, index)
    });
    support::wait_for_background_compiler_idle();
    for _ in 0..AVATAR_FRAMES {
        index += 1;
        step(&mut shell, index);
    }
    let mut news = Vec::new();
    for _ in 0..3 * AVATAR_FRAMES {
        index += 1;
        let (stats, _) = step(&mut shell, index);
        if stats.offscreen_news > 0 && index % AVATAR_FRAMES != 1 {
            news.push((index, stats.offscreen_news));
        }
    }
    assert!(news.is_empty(), "frames that took new textures: {news:?}");

    let (_, drawn) = support::update_and_capture(&mut shell, width, height);
    let (mut fresh, fresh_count, fresh_frame) = changing_cards_shell(
        support::headless_renderer_beside_locked().expect("reference renderer"),
    );
    fresh.debug_enter_app_context(|| {
        fresh_count.set(CHANGING_COUNTS[index as usize % CHANGING_COUNTS.len()]);
        fresh_frame.set(index);
    });
    let reference = support::settle(|| support::update_and_capture(&mut fresh, width, height));
    support::assert_same_bytes("changing cards", width, &reference.pixels, &drawn.pixels);
}
