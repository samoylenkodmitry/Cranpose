//! Pass structure for fixed glass over scrolling content — the uncacheable
//! backdrop topology (issue #500). Every scrolled frame re-captures and
//! re-blurs each fixed glass element; what must NOT scale with the element
//! count is the number of passes that touch the root target. Each extra
//! disjoint glass may add its own blur chain, but the optical composites
//! belong in one batched pass, and a capture must only force pending
//! composites out when one actually overlaps it.

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

/// A Regular-glass-shaped chain: separable Gaussian pre-blur feeding the
/// liquid-glass optical shader, the exact effect shape the material builds
/// for every fixed chrome element.
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

/// The device list shape: every card carries an elevation shadow, so the
/// frame interleaves shadow draws between the queued composites.
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
            // Fixed glass chrome: disjoint elements stacked down the left
            // edge, or (for the ordering test) two elements that overlap.
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
                // Shaped like the device chrome: shape clip + shadow + own
                // content, so each element renders as a child layer surface
                // with a backdrop (reasons: backdrop+shape_clip+
                // immediate_shadow), not as a bare scene backdrop.
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

/// Each extra disjoint fixed glass pays exactly its own small-target work —
/// the separable blur pair, nothing more — and NOTHING on the root target:
/// its capture is a copy, not a pass, the blur's second tap writes the
/// cacheable surface directly, and its optical shader tail defers into the
/// shared batched shader composite. Before the pre-capture flushes were
/// dependency-gated, every capture flushed the whole pending batch, so each
/// glass also fragmented the composites into its own root-target pass
/// (issue #500, cause 2); before the tail deferred, each glass also paid a
/// materialize pass baking per-frame shader dynamics into its cache key.
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

/// The most device-like headless scene — elevation-shadowed list cards
/// under fixed glass — pins its scrolled pass budget exactly. Shadow
/// draws interleave between queued composites here, so this is the scene
/// where an ordering or flush change fragments passes first (the
/// shadow-encode ablation once took a 13-pass frame to 25 by splitting
/// exactly these batches).
#[test]
fn a_shadowed_list_under_glass_holds_its_scrolled_pass_budget() {
    let single = scrolled_pass_counts_with_rows(1, true);
    let single_steady = *single.last().expect("single-glass passes");
    assert_eq!(
        single_steady, 6,
        "shadowed-rows single-glass scrolled frame pass budget moved: {single:?}"
    );
}

/// The glass element's OWN foreground must stay on top of its optical
/// tail on every drain path. The tail (a shader-queue item) and the body
/// (a blit-queue item) share the element's z index, and the fused segment
/// path used to break that tie by insertion order — blits first — so the
/// tail composited OVER the body and the white pill smeared into the
/// blur. On the Mate 20 X this read as an unreadable nav bar and empty
/// glass chips, flickering with whichever drain path the frame took.
/// Pass counts and frame times were identical on the broken build; only
/// pixels see this.
#[test]
fn a_glass_elements_own_content_stays_legible_on_a_scrolled_frame() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer, 1, false, false);
    for _ in 0..WARMUP_FRAMES {
        harness.frame(-12.0);
    }
    let frame = harness.captured_frame(-12.0);

    // The glass sits at (24,24), pads 12, and its first child is a white
    // pill 90x16 — interior sample well inside the pill's edges.
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

/// Deferring the optical composites must never let a capture read stale
/// content: when a second glass genuinely overlaps the first, its capture
/// depends on the first's composited output, so the pending batch has to
/// flush before the copy. A red-tinted lower glass must remain visible in
/// what the upper glass refracts.
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

    // The two scenes differ only by the red glass below; sample the region
    // where the upper glass overlaps it. If the upper glass captured before
    // the red glass composited, the two captures would be identical there.
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
