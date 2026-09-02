mod support;

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember};
use cranpose_liquid::{Glass, GlassButton, GlassButtonSpec, GlassSurface};
use cranpose_ui::{
    Color, LazyListScope, LazyListState, LinearArrangement, Modifier, RenderEffect, ScrollState,
    Size, TextStyle, composable, rememberLazyListState,
    widgets::{Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Text},
};

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 240;
const BAR: [f32; 4] = [16.0, 24.0, 288.0, 56.0];
const BUTTON: [f32; 4] = [260.0, 32.0, 40.0, 40.0];
const ROW_HEIGHT: f32 = 3.0;
const ROWS: usize = 240;
const SCROLL_STEP: f32 = 1.0;
const FEED_LANE: (u32, u32, u32, u32) = (40, 62, 200, 8);
const OPEN_LANE: (u32, u32, u32, u32) = (40, 150, 200, 8);

fn rect_modifier(rect: [f32; 4]) -> Modifier {
    Modifier::empty().offset(rect[0], rect[1]).size(Size {
        width: rect[2],
        height: rect[3],
    })
}

fn frame_size() -> Size {
    Size {
        width: FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    }
}

/// The page every scene sits on: a solid ground the size of the frame.
#[composable]
#[allow(non_snake_case)]
fn Page(content: impl FnMut() + 'static) {
    Box(
        Modifier::empty()
            .size(frame_size())
            .background(Color(0.08, 0.12, 0.30, 1.0)),
        BoxSpec::new(),
        content,
    );
}

#[composable]
#[allow(non_snake_case)]
fn FeedRow(index: usize) {
    Box(
        Modifier::empty()
            .size(Size {
                width: FRAME_WIDTH as f32,
                height: ROW_HEIGHT,
            })
            .background(row_color(index)),
        BoxSpec::new(),
        || {},
    );
}

fn row_color(index: usize) -> Color {
    let step = (index % 6) as f32 / 6.0;
    Color(0.15 + step * 0.7, 0.9 - step * 0.8, 0.35, 1.0)
}

#[composable]
#[allow(non_snake_case)]
fn GlassOverScrollingFeed(scroll_slot: Rc<RefCell<Option<ScrollState>>>) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    scroll_slot.borrow_mut().replace(scroll);
    Page(move || {
        Column(
            Modifier::empty()
                .size(frame_size())
                .vertical_scroll(scroll, false),
            ColumnSpec::default(),
            || {
                for index in 0..ROWS {
                    FeedRow(index);
                }
            },
        );
        Box(
            rect_modifier(BAR)
                .backdrop_effect(RenderEffect::blur(0.5))
                .background(Color(1.0, 1.0, 1.0, 0.10))
                .rounded_corners(14.0),
            BoxSpec::new(),
            || {
                Text(
                    "Library",
                    Modifier::empty().offset(16.0, 16.0),
                    TextStyle::default(),
                );
                Box(
                    rect_modifier([BUTTON[0] - BAR[0], BUTTON[1] - BAR[1], BUTTON[2], BUTTON[3]])
                        .backdrop_effect(RenderEffect::blur(4.0))
                        .background(Color(1.0, 1.0, 1.0, 0.24))
                        .rounded_corners(20.0),
                    BoxSpec::new(),
                    || {},
                );
            },
        );
    });
}

fn lane_pixels(frame: &[u8], lane: (u32, u32, u32, u32)) -> Vec<u8> {
    let (x, y, w, h) = lane;
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * FRAME_WIDTH + x) * 4) as usize;
        out.extend_from_slice(&frame[start..start + (w * 4) as usize]);
    }
    out
}

fn capture(shell: &mut AppShell<cranpose_render_wgpu::WgpuRenderer>) -> Vec<u8> {
    shell.update();
    shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture should succeed")
        .pixels
}

fn mean_abs_delta(a: &[u8], b: &[u8]) -> f32 {
    let total: u64 = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u64).sum();
    total as f32 / a.len().max(1) as f32
}

#[test]
fn a_glass_bar_with_a_nested_glass_button_follows_the_feed_scrolling_beneath_it() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return;
        }
    };
    let scroll_slot = Rc::new(RefCell::new(None::<ScrollState>));
    let root_key = location_key(file!(), line!(), column!());
    let slot_for_content = Rc::clone(&scroll_slot);
    let mut shell = AppShell::new(renderer, root_key, move || {
        GlassOverScrollingFeed(Rc::clone(&slot_for_content))
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    let first = capture(&mut shell);
    let mut previous = lane_pixels(&first, FEED_LANE);
    let mut previous_open = lane_pixels(&first, OPEN_LANE);
    let steady = lane_pixels(&capture(&mut shell), FEED_LANE);
    assert!(
        mean_abs_delta(&previous, &steady) < 0.05,
        "an unchanged frame must reproduce the lane under the bar"
    );
    let scroll = scroll_slot
        .borrow()
        .expect("the feed remembers its scroll state");
    for step in 1..=6 {
        shell
            .app_context()
            .clone()
            .enter(|| scroll.scroll_to(step as f32 * SCROLL_STEP));
        let frame = capture(&mut shell);
        let current = lane_pixels(&frame, FEED_LANE);
        let current_open = lane_pixels(&frame, OPEN_LANE);
        let open_delta = mean_abs_delta(&previous_open, &current_open);
        assert!(
            open_delta > 1.0,
            "step {step}: the feed itself did not scroll (mean delta {open_delta} in the open lane)"
        );
        previous_open = current_open;
        let delta = mean_abs_delta(&previous, &current);
        assert!(
            delta > 1.0,
            "step {step}: the feed scrolled one pixel but the pixels under the glass bar did not move (mean delta {delta}); \
             the bar's baked underlay was served for a prefix that changed"
        );
        previous = current;
    }
}

fn feed_glass() -> Glass {
    Glass::regular()
        .blur_radius(0.0)
        .refraction_depth(0.35)
        .refraction_depth_dp(6.0)
        .refraction_curve(0.6)
}

#[composable]
#[allow(non_snake_case)]
fn LiquidBarOverLazyFeed(list_slot: Rc<RefCell<Option<LazyListState>>>) {
    let list_state = rememberLazyListState();
    list_slot.borrow_mut().replace(list_state);
    Page(move || {
        LazyColumn(
            Modifier::empty().size(frame_size()),
            list_state,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(0.0)),
            move |scope| {
                scope.items(ROWS, move |index| {
                    FeedRow(index);
                });
            },
        );
        GlassSurface(rect_modifier(BAR), feed_glass(), || {
            Text(
                "Library",
                Modifier::empty().offset(16.0, 16.0),
                TextStyle::default(),
            );
            GlassButton(
                rect_modifier([BUTTON[0] - BAR[0], BUTTON[1] - BAR[1], BUTTON[2], BUTTON[3]]),
                GlassButtonSpec::glass().with_glass(feed_glass()),
                || {},
                || {},
            );
        });
    });
}

#[test]
fn a_liquid_glass_bar_with_a_glass_button_follows_a_lazy_feed_scrolling_beneath_it() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return;
        }
    };
    let list_slot = Rc::new(RefCell::new(None::<LazyListState>));
    let root_key = location_key(file!(), line!(), column!());
    let slot_for_content = Rc::clone(&list_slot);
    let mut shell = AppShell::new(renderer, root_key, move || {
        LiquidBarOverLazyFeed(Rc::clone(&slot_for_content))
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    let first = capture(&mut shell);
    let mut previous = lane_pixels(&first, FEED_LANE);
    let mut previous_open = lane_pixels(&first, OPEN_LANE);
    let list_state = (*list_slot.borrow()).expect("the feed remembers its list state");
    for step in 1..=6 {
        shell
            .app_context()
            .clone()
            .enter(|| list_state.scroll_to_item(0, step as f32 * SCROLL_STEP));
        let frame = capture(&mut shell);
        let current = lane_pixels(&frame, FEED_LANE);
        let current_open = lane_pixels(&frame, OPEN_LANE);
        let open_delta = mean_abs_delta(&previous_open, &current_open);
        assert!(
            open_delta > 1.0,
            "step {step}: the lazy feed itself did not scroll (mean delta {open_delta} in the open lane)"
        );
        previous_open = current_open;
        let delta = mean_abs_delta(&previous, &current);
        assert!(
            delta > 1.0,
            "step {step}: the lazy feed scrolled one pixel but the pixels under the liquid glass bar did not move (mean delta {delta})"
        );
        previous = current;
    }
}
