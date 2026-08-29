mod support;

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    Color, LinearArrangement, Modifier, composable,
    widgets::{Box, BoxSpec, LazyColumn, LazyColumnSpec},
};

const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 640;
const CARD_HEIGHT: f32 = 120.0;
const CARD_ELEVATION: f32 = 6.0;

#[composable]
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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
                .cloned()
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
fn an_opaque_card_shadow_composites_only_its_visible_ring() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer);
    for _ in 0..4 {
        harness.frame(-12.0);
    }
    let stats = harness.frame(-12.0);
    assert!(
        stats.shadow_shape_cache_hits > 0,
        "warm scrolled frames must composite cached card shadows \
         (hits={})",
        stats.shadow_shape_cache_hits,
    );
    let hit_px = stats.shadow_shape_cache_hit_pixels;
    assert!(
        hit_px <= 550_000,
        "an opaque caster's shadow must composite only its visible ring: \
         {hit_px} shadow pixels in one frame, expected the banded perimeter \
         strips (≤ 0.55 MP on this fixture)"
    );
}

#[test]
fn the_shadow_ring_survives_and_the_card_interior_stays_clean() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer);
    for _ in 0..4 {
        harness.frame(-12.0);
    }
    if harness.frame(-12.0).shadow_shape_cache_hits == 0 {
        panic!("fixture stopped exercising cached card shadows");
    }
    let frame = {
        self::Harness::frame(&mut harness, -12.0);
        harness
            .shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed")
    };
    let pixel = |x: u32, y: u32| {
        let offset = ((y * frame.width + x) * 4) as usize;
        [
            frame.pixels[offset],
            frame.pixels[offset + 1],
            frame.pixels[offset + 2],
        ]
    };
    let x_inside = FRAME_WIDTH / 2;
    let mut card_top: Option<u32> = None;
    let mut y = 40u32;
    while y < FRAME_HEIGHT - 80 {
        let [r, g, b] = pixel(x_inside, y);
        if r > 230 && g > 230 && b > 230 {
            card_top = Some(y);
            break;
        }
        y += 1;
    }
    let card_top = card_top.expect("a bright opaque card is on screen");
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
fn a_fully_occluded_shadow_drops_its_composite_instead_of_desyncing_the_plan() {
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

    let mut occluded_seen = 0u32;
    for _ in 0..4 {
        shell.update();
        shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("a frame with a fully occluded shadow must still render");
        let stats = shell.renderer().last_frame_stats().expect("frame stats");
        occluded_seen = occluded_seen.max(stats.shadow_fully_occluded_composites);
    }
    assert!(
        occluded_seen > 0,
        "fixture must exercise the fully occluded shadow path \
         (shadow_fully_occluded_composites stayed 0)"
    );
}
