//! Elevation shadows draw straight into the page: Skia's round rect shadow
//! at each pixel, with no surface, blur or composite.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_render_common::layer_shadow::{ShadowLight, layer_shadow_geometry};
use cranpose_ui::{
    Color, LinearArrangement, Modifier, composable,
    widgets::{Box, BoxSpec, LazyColumn, LazyColumnSpec},
};
use cranpose_ui_graphics::{GraphicsLayer, LayerShape, Rect, RoundedCornerShape};

use crate::support;

const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 640;
const CARD_HEIGHT: f32 = 120.0;
const CARD_ELEVATION: f32 = 6.0;

#[composable]
fn OpaqueCard(index: usize) {
    let fill = if index.is_multiple_of(2) {
        Color(0.98, 0.98, 0.99, 1.0)
    } else {
        Color(0.94, 0.95, 0.97, 1.0)
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(CARD_HEIGHT)
            .rounded_corners(12.0)
            .shadow(CARD_ELEVATION)
            .background(fill),
        BoxSpec::new(),
        || {},
    );
}

#[composable]
fn CardListScene(list_state: LazyListState) {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.85, 0.86, 0.88, 1.0)),
        BoxSpec::new(),
        move || {
            LazyColumn(
                Modifier::empty().fill_max_size().padding(16.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(14.0)),
                move |scope| {
                    scope.items(LazyItems::new(60).key(|i: usize| i as u64), OpaqueCard);
                },
            );
        },
    );
}

struct Harness {
    shell: AppShell<cranpose_render_wgpu::WgpuRenderer>,
    list_state: Rc<RefCell<Option<LazyListState>>>,
}

impl Harness {
    fn new(renderer: cranpose_render_wgpu::WgpuRenderer) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let list_state: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let list_state_for_app = Rc::clone(&list_state);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = rememberLazyListState();
            *list_state_for_app.borrow_mut() = Some(state);
            CardListScene(state);
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self { shell, list_state }
    }

    fn frame(&mut self, scroll_delta: f32) -> cranpose_render_wgpu::RenderStatsSnapshot {
        if scroll_delta != 0.0 {
            let state = self
                .list_state
                .borrow()
                .as_ref()
                .copied()
                .expect("list state captured");
            self.shell
                .debug_enter_app_context(|| state.dispatch_scroll_delta(scroll_delta));
        }
        self.shell.update();
        self.shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed");
        self.shell
            .renderer()
            .last_frame_stats()
            .expect("frame stats")
    }
}

#[test]
fn scrolling_card_shadows_draw_in_the_page_pass() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer);
    for _ in 0..4 {
        harness.frame(-12.0);
    }
    let stats = harness.frame(-12.5);
    assert_eq!(stats.blur_passes, 0, "no shadow blurs: {stats:?}");
    assert_eq!(stats.offscreen_acquires, 0, "no shadow surface: {stats:?}");
    assert_eq!(
        stats.shadow_shape_cache_misses + stats.shadow_shape_cache_hits,
        0,
        "no shadow cache: {stats:?}"
    );
}

#[test]
fn the_shadow_ring_survives_and_the_card_interior_stays_clean() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer);
    for _ in 0..5 {
        harness.frame(-12.0);
    }
    let frame = harness
        .shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture should succeed");
    let pixel = |x: u32, y: u32| {
        let offset = ((y * frame.width + x) * 4) as usize;
        [
            frame.pixels[offset],
            frame.pixels[offset + 1],
            frame.pixels[offset + 2],
        ]
    };
    let x_inside = FRAME_WIDTH / 2;
    let bright = |y: u32| pixel(x_inside, y).iter().all(|c| *c > 230);
    let card_top = (41..FRAME_HEIGHT - 80)
        .find(|&y| bright(y) && !bright(y - 1))
        .expect("the top edge of a bright opaque card is on screen");
    let card_bottom = card_top + CARD_HEIGHT as u32 - 1;

    let interior = pixel(x_inside, card_top + CARD_HEIGHT as u32 / 2);
    assert!(
        interior.iter().all(|c| *c > 225),
        "the card interior must stay the card's own bright fill: {interior:?}"
    );

    let below = pixel(x_inside, card_bottom + 4);
    let background = pixel(x_inside, card_bottom + 60);
    let below_sum: u32 = below.iter().map(|c| *c as u32).sum();
    let background_sum: u32 = background.iter().map(|c| *c as u32).sum();
    assert!(
        below_sum + 12 < background_sum,
        "the shadow ring below the card must stay darker than the far \
         background: below={below:?} background={background:?}"
    );
}

#[test]
fn a_shadow_clipped_to_its_hole_draws_nothing_and_the_card_shows() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, || {
        Box(
            Modifier::empty()
                .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
                .background(Color(0.85, 0.86, 0.88, 1.0)),
            BoxSpec::new(),
            || {
                Box(
                    Modifier::empty()
                        .offset(160.0, 160.0)
                        .size_points(200.0, 200.0)
                        .clip_to_bounds(),
                    BoxSpec::new(),
                    || {
                        Box(
                            Modifier::empty()
                                .offset(-60.0, -60.0)
                                .required_size(cranpose_ui::Size {
                                    width: 320.0,
                                    height: 320.0,
                                })
                                .rounded_corners(2.0)
                                .shadow(CARD_ELEVATION)
                                .background(Color(0.98, 0.98, 0.99, 1.0)),
                            BoxSpec::new(),
                            || {},
                        );
                    },
                );
            },
        );
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("a frame whose shadow lies under its card must render");
    for (x, y) in [(161, 161), (260, 260), (358, 358)] {
        let offset = ((y * frame.width + x) * 4) as usize;
        assert!(
            frame.pixels[offset..offset + 3].iter().all(|c| *c > 245),
            "the card's fill at ({x}, {y}): {:?}",
            &frame.pixels[offset..offset + 3]
        );
    }
}

const PAGE: Color = Color(0.85, 0.86, 0.88, 1.0);
const CARD: Rect = Rect {
    x: 40.0,
    y: 30.0,
    width: 80.0,
    height: 50.0,
};

/// One white card with a round rect shadow on a grey page, `width` × 120.
fn shadowed_card(width: f32) {
    Box(
        Modifier::empty().size_points(width, 120.0).background(PAGE),
        BoxSpec::new(),
        || {
            Box(
                Modifier::empty()
                    .offset(CARD.x, CARD.y)
                    .size_points(CARD.width, CARD.height)
                    .shadow_with(
                        6.0,
                        LayerShape::Rounded(RoundedCornerShape::uniform(10.0)),
                        false,
                        Color::BLACK,
                        Color::BLACK,
                    )
                    .background(Color::WHITE),
                BoxSpec::new(),
                || {},
            );
        },
    );
}

#[test]
fn an_elevation_shadow_draws_skias_ramp_at_every_pixel() {
    const WIDTH: u32 = 160;
    const HEIGHT: u32 = 120;
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, || shadowed_card(WIDTH as f32));
    shell.set_viewport(WIDTH as f32, HEIGHT as f32);
    shell.set_buffer_size(WIDTH, HEIGHT);
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(WIDTH, HEIGHT)
        .expect("frame capture should succeed");

    let layer = GraphicsLayer {
        shadow_elevation: 6.0,
        ambient_shadow_color: Color::BLACK,
        spot_shadow_color: Color::BLACK,
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(10.0)),
        ..GraphicsLayer::default()
    };
    let light = ShadowLight::for_window(WIDTH as f32, HEIGHT as f32, 1.0, 1.0);
    let geometry =
        layer_shadow_geometry(&layer, CARD, Some(RoundedCornerShape::uniform(10.0)), light);
    let near_card = |x: f32, y: f32| {
        x > CARD.x - 1.0
            && x < CARD.x + CARD.width + 1.0
            && y > CARD.y - 1.0
            && y < CARD.y + CARD.height + 1.0
    };
    let mut worst = (0.0_f32, 0, 0);
    let mut shaded = 0;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            if near_card(cx, cy) {
                continue;
            }
            let kept: f32 = geometry
                .passes()
                .map(|(shadow, color)| 1.0 - color.a() * shadow.coverage(cx, cy))
                .product();
            if kept < 1.0 {
                shaded += 1;
            }
            let offset = ((y * WIDTH + x) * 4) as usize;
            for (channel, page) in [PAGE.r(), PAGE.g(), PAGE.b()].into_iter().enumerate() {
                let expected = page * kept * 255.0;
                let error = (frame.pixels[offset + channel] as f32 - expected).abs();
                if error > worst.0 {
                    worst = (error, x, y);
                }
            }
        }
    }
    assert!(
        shaded > 1_000,
        "the fixture shades the page: {shaded} pixels"
    );
    assert!(
        worst.0 <= 2.0,
        "the shadow strays {} levels from Skia's ramp at ({}, {})",
        worst.0,
        worst.1,
        worst.2
    );
}
