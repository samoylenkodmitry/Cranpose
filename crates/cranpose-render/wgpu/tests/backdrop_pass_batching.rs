mod support;

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    Color, LinearArrangement, Modifier, RenderEffect, composable,
    widgets::{Box, BoxSpec, LazyColumn, LazyColumnSpec},
};
use cranpose_ui_graphics::{LiquidGlassRect, LiquidGlassSpec, TileMode, liquid_glass_effect};

const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 640;

fn chain_glass_effect(rect_width: f32, rect_height: f32, tint: Color) -> RenderEffect {
    let optical = liquid_glass_effect(
        &LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: rect_width,
            height: rect_height,
            tint_color: tint,
        },
        &LiquidGlassSpec {
            corner_radius: 14.0,
            blur_radius: 0.0,
            ..LiquidGlassSpec::default()
        },
        rect_width,
        rect_height,
    );
    RenderEffect::blur_with_edge_treatment(18.0, TileMode::Mirror).then(optical)
}

#[composable]
#[allow(non_snake_case)]
fn ScrollRow(index: usize) {
    let fill = match index % 4 {
        0 => Color(0.85, 0.25, 0.25, 1.0),
        1 => Color(0.20, 0.55, 0.90, 1.0),
        2 => Color(0.25, 0.75, 0.40, 1.0),
        _ => Color(0.90, 0.75, 0.20, 1.0),
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(72.0)
            .background(fill)
            .rounded_corners(12.0),
        BoxSpec::new(),
        || {},
    );
}

#[composable]
#[allow(non_snake_case)]
fn ShadowedScrollRow(index: usize) {
    let fill = match index % 4 {
        0 => Color(0.85, 0.25, 0.25, 1.0),
        1 => Color(0.20, 0.55, 0.90, 1.0),
        2 => Color(0.25, 0.75, 0.40, 1.0),
        _ => Color(0.90, 0.75, 0.20, 1.0),
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(72.0)
            .background(fill)
            .rounded_corners(12.0)
            .shadow(4.0),
        BoxSpec::new(),
        || {},
    );
}

#[composable]
#[allow(non_snake_case)]
fn FixedGlassScene(
    list_state: LazyListState,
    glass_count: usize,
    overlap: bool,
    shadowed_rows: bool,
) {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.05, 0.06, 0.08, 1.0)),
        BoxSpec::new(),
        move || {
            LazyColumn(
                Modifier::empty().fill_max_size(),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
                move |scope| {
                    if shadowed_rows {
                        scope.items(
                            LazyItems::new(60).key(|i: usize| i as u64),
                            ShadowedScrollRow,
                        );
                    } else {
                        scope.items(LazyItems::new(60).key(|i: usize| i as u64), ScrollRow);
                    }
                },
            );
            for index in 0..glass_count {
                let (x, y) = if overlap {
                    (40.0 + index as f32 * 60.0, 40.0 + index as f32 * 30.0)
                } else {
                    (24.0, 24.0 + index as f32 * 120.0)
                };
                let tint = if index == 0 {
                    Color(0.9, 0.05, 0.05, 0.65)
                } else {
                    Color(0.9, 0.9, 0.95, 0.10)
                };
                Box(
                    Modifier::empty()
                        .offset(x, y)
                        .width(220.0)
                        .height(88.0)
                        .rounded_corners(14.0)
                        .shadow(8.0)
                        .backdrop_effect(chain_glass_effect(220.0, 88.0, tint))
                        .padding(12.0),
                    BoxSpec::new(),
                    move || {
                        Box(
                            Modifier::empty()
                                .width(90.0)
                                .height(16.0)
                                .background(Color(1.0, 1.0, 1.0, 0.8))
                                .rounded_corners(8.0),
                            BoxSpec::new(),
                            || {},
                        );
                    },
                );
            }
        },
    );
}

struct Harness {
    shell: AppShell<cranpose_render_wgpu::WgpuRenderer>,
    list_state: Rc<RefCell<Option<LazyListState>>>,
}

impl Harness {
    fn new(
        renderer: cranpose_render_wgpu::WgpuRenderer,
        glass_count: usize,
        overlap: bool,
        shadowed_rows: bool,
    ) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let list_state: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let list_state_for_app = Rc::clone(&list_state);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = rememberLazyListState();
            *list_state_for_app.borrow_mut() = Some(state);
            FixedGlassScene(state, glass_count, overlap, shadowed_rows);
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

    fn captured_frame(&mut self, scroll_delta: f32) -> cranpose_render_wgpu::CapturedFrame {
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
            .expect("frame capture should succeed")
    }
}

const WARMUP_FRAMES: usize = 4;
const MEASURED_FRAMES: usize = 6;

fn scrolled_pass_counts(glass_count: usize) -> Vec<u32> {
    scrolled_pass_counts_with_rows(glass_count, false)
}

fn scrolled_pass_counts_with_rows(glass_count: usize, shadowed_rows: bool) -> Vec<u32> {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer, glass_count, false, shadowed_rows);
    for _ in 0..WARMUP_FRAMES {
        harness.frame(-12.0);
    }
    (0..MEASURED_FRAMES)
        .map(|_| harness.frame(-12.0).pass_count)
        .collect()
}

#[test]
fn each_extra_fixed_glass_adds_only_its_blur_chain() {
    let single: Vec<u32> = scrolled_pass_counts(1);
    let triple: Vec<u32> = scrolled_pass_counts(3);
    let single_steady = *single.last().expect("single-glass passes");
    let triple_steady = *triple.last().expect("triple-glass passes");
    assert!(
        single.iter().all(|passes| *passes == single_steady),
        "single-glass scrolled frames must encode a steady pass count: {single:?}"
    );
    assert!(
        triple.iter().all(|passes| *passes == triple_steady),
        "triple-glass scrolled frames must encode a steady pass count: {triple:?}"
    );
    assert_eq!(
        triple_steady,
        single_steady + 4,
        "two extra fixed glasses must add exactly their blur pairs (2 \
         small-target passes each), with the shader tails and composites \
         riding the shared batches: single={single:?} triple={triple:?}"
    );
}

#[test]
fn a_shadowed_list_under_glass_holds_its_scrolled_pass_budget() {
    let single = scrolled_pass_counts_with_rows(1, true);
    let single_steady = *single.last().expect("single-glass passes");
    assert_eq!(
        single_steady, 6,
        "shadowed-rows single-glass scrolled frame pass budget moved: {single:?}"
    );
}

#[test]
fn a_glass_elements_own_content_stays_legible_on_a_scrolled_frame() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer, 1, false, false);
    for _ in 0..WARMUP_FRAMES {
        harness.frame(-12.0);
    }
    let frame = harness.captured_frame(-12.0);

    let pixel = |x: u32, y: u32| {
        let offset = ((y * frame.width + x) * 4) as usize;
        [
            frame.pixels[offset],
            frame.pixels[offset + 1],
            frame.pixels[offset + 2],
        ]
    };
    let mut legible = 0usize;
    let mut total = 0usize;
    for y in 40..48u32 {
        for x in 44..118u32 {
            total += 1;
            let [r, g, b] = pixel(x, y);
            if r.min(g).min(b) > 170 {
                legible += 1;
            }
        }
    }
    assert!(
        legible * 10 >= total * 8,
        "the glass pill must render on top of the optical tail: only \
         {legible}/{total} sampled pixels are pill-bright"
    );
}

#[test]
fn an_overlapping_capture_still_sees_the_glass_below_it() {
    let overlap_pixels = {
        let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
        let mut harness = Harness::new(renderer, 2, true, false);
        for _ in 0..WARMUP_FRAMES {
            harness.frame(-12.0);
        }
        harness.captured_frame(-12.0)
    };
    let solo_pixels = {
        let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
        let mut harness = Harness::new(renderer, 1, true, false);
        for _ in 0..WARMUP_FRAMES {
            harness.frame(-12.0);
        }
        harness.captured_frame(-12.0)
    };

    let sample_rect = (110u32, 80u32, 60u32, 30u32);
    let pixel = |frame: &cranpose_render_wgpu::CapturedFrame, x: u32, y: u32| {
        let offset = ((y * frame.width + x) * 4) as usize;
        [
            frame.pixels[offset],
            frame.pixels[offset + 1],
            frame.pixels[offset + 2],
            frame.pixels[offset + 3],
        ]
    };
    let mut differing = 0usize;
    let mut total = 0usize;
    for y in sample_rect.1..sample_rect.1 + sample_rect.3 {
        for x in sample_rect.0..sample_rect.0 + sample_rect.2 {
            let overlap_px = pixel(&overlap_pixels, x, y);
            let solo_px = pixel(&solo_pixels, x, y);
            total += 1;
            let delta = overlap_px
                .iter()
                .zip(solo_px.iter())
                .map(|(a, b)| a.abs_diff(*b) as usize)
                .sum::<usize>();
            if delta > 12 {
                differing += 1;
            }
        }
    }
    assert!(
        differing * 5 >= total,
        "the upper glass must refract the red glass below it: only \
         {differing}/{total} sampled pixels differ from the red-less scene"
    );
}

#[composable]
#[allow(non_snake_case)]
fn DeferredContentUnderGlass(overlap: bool) {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.05, 0.06, 0.08, 1.0)),
        BoxSpec::new(),
        move || {
            Box(
                Modifier::empty()
                    .offset(24.0, 24.0)
                    .width(220.0)
                    .height(88.0)
                    .rounded_corners(14.0)
                    .backdrop_effect(chain_glass_effect(220.0, 88.0, Color(0.9, 0.9, 0.95, 0.1))),
                BoxSpec::new(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(40.0, 200.0)
                    .width(200.0)
                    .height(60.0)
                    .background(Color(1.0, 0.0, 0.0, 1.0)),
                BoxSpec::new(),
                || {},
            );
            let y = if overlap { 210.0 } else { 400.0 };
            Box(
                Modifier::empty()
                    .offset(60.0, y)
                    .width(160.0)
                    .height(40.0)
                    .rounded_corners(10.0)
                    .backdrop_effect(chain_glass_effect(160.0, 40.0, Color(0.9, 0.9, 0.95, 0.1))),
                BoxSpec::new(),
                || {},
            );
        },
    );
}

fn deferred_frame(overlap: bool) -> (cranpose_render_wgpu::CapturedFrame, u32) {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, move || {
        DeferredContentUnderGlass(overlap)
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture should succeed");
    let passes = shell
        .renderer()
        .last_frame_stats()
        .expect("frame stats")
        .pass_count;
    (frame, passes)
}

#[test]
fn a_direct_draw_between_two_glasses_is_captured_by_the_glass_that_covers_it() {
    let (frame, _) = deferred_frame(true);
    let pixel = |x: usize, y: usize| {
        let index = (y * FRAME_WIDTH as usize + x) * 4;
        [
            frame.pixels[index],
            frame.pixels[index + 1],
            frame.pixels[index + 2],
        ]
    };
    let under_glass = pixel(140, 230);
    let beside_glass = pixel(40 + 8, 205);
    assert!(
        beside_glass[0] > 200 && beside_glass[1] < 40,
        "the red box draws where nothing covers it: {beside_glass:?}"
    );
    assert!(
        under_glass[0] > 120 && under_glass[0] > under_glass[1] + 60,
        "the glass covering the red box must have captured it after it was drawn, not the \
         empty page beneath: {under_glass:?}"
    );
}

#[test]
fn direct_draws_between_non_overlapping_glasses_share_one_fused_pass() {
    let (_, overlapping_passes) = deferred_frame(true);
    let (_, disjoint_passes) = deferred_frame(false);
    assert!(
        disjoint_passes < overlapping_passes,
        "a draw no later capture reads must ride the frame's single fused pass; disjoint \
         {disjoint_passes} vs overlapping {overlapping_passes}"
    );
}

#[composable]
#[allow(non_snake_case)]
fn ShadowedGlassAfterAnotherGlass() {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.92, 0.92, 0.94, 1.0)),
        BoxSpec::new(),
        || {
            Box(
                Modifier::empty()
                    .offset(60.0, 20.0)
                    .width(200.0)
                    .height(40.0)
                    .rounded_corners(12.0)
                    .backdrop_effect(chain_glass_effect(200.0, 40.0, Color(1.0, 1.0, 1.0, 0.05))),
                BoxSpec::new(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(60.0, 120.0)
                    .width(200.0)
                    .height(90.0)
                    .rounded_corners(16.0)
                    .drop_shadow(
                        cranpose_ui::LayerShape::Rounded(cranpose_ui::RoundedCornerShape::uniform(
                            16.0,
                        )),
                        |scope| {
                            scope.radius = 24.0;
                            scope.offset.y = 12.0;
                            scope.color = Color(0.0, 0.0, 0.0, 0.9);
                        },
                    )
                    .backdrop_effect(chain_glass_effect(200.0, 90.0, Color(1.0, 1.0, 1.0, 0.05))),
                BoxSpec::new(),
                || {},
            );
        },
    );
}

#[test]
fn a_glass_captures_its_own_drop_shadow_when_that_shadow_is_all_that_is_pending() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, ShadowedGlassAfterAnotherGlass);
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture should succeed");
    let luma = |x: usize, y: usize| {
        let index = (y * FRAME_WIDTH as usize + x) * 4;
        (u32::from(frame.pixels[index])
            + u32::from(frame.pixels[index + 1])
            + u32::from(frame.pixels[index + 2]))
            / 3
    };
    let bottom_inside = luma(160, 204);
    let top_inside = luma(160, 126);
    assert!(
        top_inside > bottom_inside + 20,
        "the second glass must read its own shadow off the page below it while the first \
         glass already consumed everything else pending: top {top_inside} vs bottom \
         {bottom_inside}"
    );
}

const SHADOWED_CARD_COUNT: usize = 3;
const SHADOWED_CARD_PITCH: f32 = 160.0;
const SHADOWED_CARD_TOP: f32 = 40.0;
const SHADOWED_CARD_HEIGHT: f32 = 100.0;

#[composable]
#[allow(non_snake_case)]
fn ShadowedGlassColumn(shadowed: bool, scroll: f32) {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.92, 0.92, 0.94, 1.0)),
        BoxSpec::new(),
        move || {
            for index in 0..SHADOWED_CARD_COUNT {
                let y = SHADOWED_CARD_TOP + index as f32 * SHADOWED_CARD_PITCH - scroll;
                let modifier = Modifier::empty()
                    .offset(60.0, y)
                    .width(220.0)
                    .height(SHADOWED_CARD_HEIGHT)
                    .rounded_corners(16.0);
                let modifier = if shadowed {
                    modifier.drop_shadow(
                        cranpose_ui::LayerShape::Rounded(cranpose_ui::RoundedCornerShape::uniform(
                            16.0,
                        )),
                        |scope| {
                            scope.radius = 24.0;
                            scope.offset.y = 12.0;
                            scope.color = Color(0.0, 0.0, 0.0, 0.9);
                        },
                    )
                } else {
                    modifier
                };
                Box(
                    modifier.backdrop_effect(chain_glass_effect(
                        220.0,
                        SHADOWED_CARD_HEIGHT,
                        Color(1.0, 1.0, 1.0, 0.05),
                    )),
                    BoxSpec::new(),
                    || {},
                );
            }
        },
    );
}

fn scrolled_column_frame(
    shadowed: bool,
    queue_shadows: bool,
) -> (cranpose_render_wgpu::CapturedFrame, u32) {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    cranpose_render_wgpu::set_debug_toggle(
        "CRANPOSE_NO_SHADOW_COMPOSITE_QUEUE",
        (!queue_shadows).then_some("1"),
    );
    let root_key = location_key(file!(), line!(), column!());
    let scroll = Rc::new(std::cell::Cell::new(0.0f32));
    let scroll_for_app = Rc::clone(&scroll);
    let mut shell = AppShell::new(renderer, root_key, move || {
        ShadowedGlassColumn(shadowed, scroll_for_app.get())
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    let mut passes = 0;
    let mut frame = None;
    for step in 0..WARMUP_FRAMES + 1 {
        scroll.set(step as f32 * 2.0);
        shell.update();
        frame = Some(
            shell
                .renderer()
                .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
                .expect("frame capture should succeed"),
        );
        passes = shell
            .renderer()
            .last_frame_stats()
            .expect("frame stats")
            .pass_count;
    }
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_SHADOW_COMPOSITE_QUEUE", None);
    (frame.expect("a frame was captured"), passes)
}

#[test]
fn drop_shadows_under_a_column_of_glass_cards_ride_the_frames_fused_pass() {
    let (plain, plain_passes) = scrolled_column_frame(false, true);
    let (shadowed, shadowed_passes) = scrolled_column_frame(true, true);
    let luma = |frame: &cranpose_render_wgpu::CapturedFrame, x: usize, y: usize| {
        let index = (y * FRAME_WIDTH as usize + x) * 4;
        (u32::from(frame.pixels[index])
            + u32::from(frame.pixels[index + 1])
            + u32::from(frame.pixels[index + 2]))
            / 3
    };
    let scroll = WARMUP_FRAMES as f32 * 2.0;
    for index in 0..SHADOWED_CARD_COUNT {
        let below = (SHADOWED_CARD_TOP + index as f32 * SHADOWED_CARD_PITCH + SHADOWED_CARD_HEIGHT
            - scroll) as usize
            + 6;
        let plain_luma = luma(&plain, 170, below);
        let shadowed_luma = luma(&shadowed, 170, below);
        assert!(
            shadowed_luma + 20 < plain_luma,
            "card {index} must cast its shadow onto the page: shadowed {shadowed_luma} vs plain \
             {plain_luma}"
        );
    }
    assert_eq!(
        shadowed_passes, plain_passes,
        "a cached drop shadow is a composite the glass above it replays, not a direct draw that \
         splits the frame's fused pass before every card"
    );
}

#[test]
fn a_shadow_composited_from_its_cache_matches_the_shadow_drawn_in_the_segment() {
    let (direct, direct_passes) = scrolled_column_frame(true, false);
    let (queued, queued_passes) = scrolled_column_frame(true, true);
    let max_delta = direct
        .pixels
        .iter()
        .zip(&queued.pixels)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(
        max_delta <= 1,
        "the queued shadow composite must land the same texels as the segment's own composite \
         of the same cached shadow: max delta {max_delta}"
    );
    assert!(
        queued_passes < direct_passes,
        "queuing the shadows must remove the per-card run flushes: queued {queued_passes} vs \
         direct {direct_passes}"
    );
}
