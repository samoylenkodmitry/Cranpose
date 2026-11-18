// A popover's glass tail: it covers the point it reaches, leaves the backdrop
// beside and past it, and draws the same bytes with its own folded pipeline
// as with the general one.

use cranpose_app_shell::AppShell;
use cranpose_liquid::{
    Glass, GlassDynamics, GlassMorph, GlassTail, GlassTailEdge, LiquidModifierExt, LiquidShape,
    LiquidTheme, LiquidThemeSpec,
};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui_graphics::Brush;
use support::page::*;

use crate::support;

const FRAME_WIDTH: u32 = 360;
const FRAME_HEIGHT: u32 = 240;
// The popover node: a 240 by 172 card over a 13 dp tail.
const NODE: [f32; 4] = [60.0, 20.0, 240.0, 185.0];
const CARD_HEIGHT: f32 = 172.0;
const TAIL_HEIGHT: f32 = 13.0;
const GENERAL: &str = "CRANPOSE_NO_SHADER_SPECIALIZATION";

fn card_glass() -> Glass {
    Glass::regular()
        .shape(LiquidShape::RoundedRect(34.0))
        .shadow(false)
        .no_clip()
}

fn popover_dynamics(tail: Option<GlassTail>) -> GlassDynamics {
    GlassDynamics {
        activity: Some(1.0),
        morph: Some(GlassMorph {
            node_size: (NODE[2], NODE[3]),
            primary: (NODE[2] * 0.5, CARD_HEIGHT * 0.5, NODE[2], CARD_HEIGHT, 34.0),
            tail,
            ..GlassMorph::default()
        }),
        ..Default::default()
    }
}

fn bottom_tail() -> Option<GlassTail> {
    Some(GlassTail {
        tip: (NODE[2] * 0.5, CARD_HEIGHT + TAIL_HEIGHT),
        edge: GlassTailEdge::Bottom,
        height: TAIL_HEIGHT,
        tip_radius: 2.0,
        glue: 2.0,
    })
}

#[composable]
fn PopoverPage(tailed: bool) {
    LiquidTheme(LiquidThemeSpec::default(), move || {
        FramePage(FRAME_WIDTH, FRAME_HEIGHT, Color::WHITE, move || {
            Box(
                Modifier::empty().fill_max_size().draw_behind(|scope| {
                    let size = scope.size();
                    let mut x = 0.0;
                    while x < size.width {
                        scope.draw_rect_at(
                            cranpose_ui_graphics::Rect {
                                x,
                                y: 0.0,
                                width: 4.0,
                                height: size.height,
                            },
                            Brush::solid(Color::from_rgb_u8(30, 90, 200)),
                        );
                        x += 8.0;
                    }
                }),
                BoxSpec::default(),
                move || {
                    let dynamics = move || popover_dynamics(tailed.then(bottom_tail).flatten());
                    Box(
                        rect_modifier(NODE).glass_effect_with(card_glass(), dynamics),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    });
}

#[composable]
fn TailedPage() {
    PopoverPage(true);
}

#[composable]
fn PlainCardPage() {
    PopoverPage(false);
}

const SETTLE: std::time::Duration = std::time::Duration::from_secs(30);

// The frame once every glass draw uses the pipeline it asked for.
fn settled(shell: &mut AppShell<WgpuRenderer>) -> (CapturedFrame, RenderStatsSnapshot) {
    let deadline = std::time::Instant::now() + SETTLE;
    loop {
        let (frame, stats) = support::warm_shell_frame(shell, FRAME_WIDTH, FRAME_HEIGHT);
        if stats.shader_pipeline_fallback_draws == 0 {
            return (frame, stats);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the glass pipelines never finished compiling"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn capture(
    page: fn(),
    toggle: Option<&'static str>,
) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    let (_lock, mut shell) = support::app_shell_for(
        page,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        |_| {},
    )?;
    if let Some(toggle) = toggle {
        cranpose_core::debug_toggles::set_debug_toggle(toggle, Some("1"));
    }
    let frame = settled(&mut shell);
    if let Some(toggle) = toggle {
        cranpose_core::debug_toggles::set_debug_toggle(toggle, None);
    }
    Some(frame)
}

fn pixel(frame: &CapturedFrame, x: f32, y: f32) -> [u8; 4] {
    let index = (y as usize * FRAME_WIDTH as usize + x as usize) * 4;
    frame.pixels[index..index + 4]
        .try_into()
        .expect("four channels")
}

#[test]
fn a_glass_tail_covers_its_point_and_leaves_the_backdrop_beside_and_past_it() {
    let Some((tailed, _)) = capture(TailedPage, None) else {
        return;
    };
    let (plain, _) = capture(PlainCardPage, None).expect("headless WGPU init failed mid-suite");
    let center = NODE[0] + NODE[2] * 0.5;
    let edge = NODE[1] + CARD_HEIGHT;
    for depth in [3.0, 6.0, 9.0] {
        assert_ne!(
            pixel(&tailed, center, edge + depth),
            pixel(&plain, center, edge + depth),
            "the tail covers its centre line {depth} dp past the card"
        );
        let half_width = TAIL_HEIGHT + 0.83 - depth;
        for side in [-1.0, 1.0] {
            let beside = center + side * (half_width + 3.0);
            assert_eq!(
                pixel(&tailed, beside, edge + depth),
                pixel(&plain, beside, edge + depth),
                "the tail leaves the backdrop 3 dp beside its slope, {depth} dp past the card"
            );
        }
    }
    for past in [2.0, 6.0] {
        assert_eq!(
            pixel(&tailed, center, edge + TAIL_HEIGHT + past),
            pixel(&plain, center, edge + TAIL_HEIGHT + past),
            "the tail ends at its tip"
        );
    }
}

#[test]
fn a_tailed_glass_draws_the_same_with_its_own_pipeline_and_the_general_one() {
    let Some((own, own_stats)) = capture(TailedPage, None) else {
        return;
    };
    let (general, _) =
        capture(TailedPage, Some(GENERAL)).expect("headless WGPU init failed mid-suite");
    assert!(own_stats.shader_specialized_draws > 0);
    support::assert_same_bytes(
        "a tailed glass with its own pipeline and with the general one",
        FRAME_WIDTH,
        &own.pixels,
        &general.pixels,
    );
}
