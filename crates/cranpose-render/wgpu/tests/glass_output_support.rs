use cranpose_liquid::{
    Glass, GlassDynamics, LiquidModifierExt, LiquidShape, LiquidTheme, LiquidThemeSpec,
};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui_graphics::{Brush, Point, Rect, TileMode};
use support::page::*;

use crate::support;

const FRAME_WIDTH: u32 = 360;
const FRAME_HEIGHT: u32 = 240;
const NODE: [f32; 4] = [40.0, 40.0, 280.0, 160.0];
const TOGGLE: &str = "CRANPOSE_NO_EFFECT_DOMAINS";

fn lens_glass() -> Glass {
    Glass::regular()
        .shape(LiquidShape::RoundedRect(12.0))
        .blur_radius(12.0)
        .shadow(false)
        .no_clip()
}

fn lens_dynamics() -> GlassDynamics {
    support::morphing_lens_dynamics(
        Rect {
            x: NODE[0],
            y: NODE[1],
            width: NODE[2],
            height: NODE[3],
        },
        (140.0, 80.0, 120.0, 40.0, -1.0),
    )
}

fn left_lens_dynamics() -> GlassDynamics {
    support::morphing_lens_dynamics(
        Rect {
            x: NODE[0],
            y: NODE[1],
            width: NODE[2],
            height: NODE[3],
        },
        (55.0, 80.0, 56.0, 36.0, -1.0),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum PageContent {
    Lens,
    DisjointGlass,
    Empty,
}

#[composable]
fn PatternedFrame(content: PageContent) {
    FramePage(
        FRAME_WIDTH,
        FRAME_HEIGHT,
        Color(0.1, 0.1, 0.16, 1.0),
        move || {
            Box(
                Modifier::empty().fill_max_size().draw_behind(|scope| {
                    let size = scope.size();
                    scope.draw_rect(Brush::radial_gradient_stops(
                        vec![
                            (0.0, Color::from_rgb_u8(90, 70, 140)),
                            (0.6, Color::from_rgb_u8(30, 26, 60)),
                            (1.0, Color::from_rgb_u8(8, 8, 20)),
                        ],
                        Point::new(size.width * 0.4, size.height * 0.3),
                        size.width.max(size.height) * 0.9,
                        TileMode::Clamp,
                    ));
                    for i in 0..60u32 {
                        let x = (i as f32 * 41.3) % size.width;
                        let y = (i as f32 * 23.7) % size.height;
                        scope.draw_circle(
                            Brush::solid(Color::from_rgba_u8(
                                255,
                                240,
                                200,
                                150 + (i % 4) as u8 * 25,
                            )),
                            Point::new(x, y),
                            1.0 + (i % 4) as f32,
                        );
                    }
                }),
                BoxSpec::default(),
                move || {
                    if content != PageContent::Empty {
                        let dynamics = if content == PageContent::DisjointGlass {
                            left_lens_dynamics
                        } else {
                            lens_dynamics
                        };
                        Box(
                            rect_modifier(NODE).glass_effect_with(lens_glass(), dynamics),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                    if content == PageContent::DisjointGlass {
                        Box(
                            rect_modifier([245.0, 90.0, 65.0, 50.0]).glass_effect(lens_glass()),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn LensPage() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        PatternedFrame(PageContent::Lens);
    });
}

#[composable]
fn DisjointGlassPage() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        PatternedFrame(PageContent::DisjointGlass);
    });
}

#[composable]
fn PlainPage() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        PatternedFrame(PageContent::Empty);
    });
}

fn capture(whole_node: bool) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    capture_page(LensPage, whole_node)
}

fn capture_page(page: fn(), whole_node: bool) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    capture_page_with_cache(page, whole_node, false)
}

fn capture_uncached_page(
    page: fn(),
    whole_node: bool,
) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    capture_page_with_cache(page, whole_node, true)
}

struct CaptureToggles;

impl Drop for CaptureToggles {
    fn drop(&mut self) {
        cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", None);
    }
}

fn capture_page_with_cache(
    page: fn(),
    whole_node: bool,
    uncached: bool,
) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    let (_lock, mut shell) = support::app_shell_for(
        page,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {},
    )?;

    let _toggles = CaptureToggles;
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, whole_node.then_some("1"));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", uncached.then_some("1"));
    Some(support::warm_shell_frame(
        &mut shell,
        FRAME_WIDTH,
        FRAME_HEIGHT,
    ))
}

#[test]
fn a_lens_composited_within_its_declared_support_is_the_lens_composited_over_its_whole_node() {
    let Some((within_support, with_support)) = capture(false) else {
        return;
    };
    let (whole_node, without_support) = capture(true).expect("headless WGPU init failed mid-suite");
    let differing =
        support::differing_pixels(FRAME_WIDTH, &within_support.pixels, &whole_node.pixels);
    assert!(
        differing.is_empty(),
        "the lens composited within its declared output support differs from the lens \
         composited over its whole node: {}",
        support::describe_differing(&differing)
    );
    assert!(
        with_support.shader_pixels < without_support.shader_pixels,
        "the declared support must shrink the lens's composite: {} shader pixels with it \
         against {} without; blur {} against {} (stages {})",
        with_support.shader_pixels,
        without_support.shader_pixels,
        with_support.blur_pixels,
        without_support.blur_pixels,
        with_support.stages
    );
    assert_eq!(
        with_support.blur_pixels, without_support.blur_pixels,
        "a material that declares no sample domain keeps its whole blur (stages {})",
        with_support.stages
    );
}

#[test]
fn disjoint_glass_outputs_share_a_stage_without_changing_the_picture() {
    let Some((with_support, supported_stats)) = capture_uncached_page(DisjointGlassPage, false)
    else {
        return;
    };
    let (whole_node, conservative_stats) = capture_uncached_page(DisjointGlassPage, true)
        .expect("headless WGPU init failed mid-suite");
    let (plain, _) = capture_page(PlainPage, false).expect("headless WGPU init failed mid-suite");

    let differing =
        support::differing_pixels(FRAME_WIDTH, &with_support.pixels, &whole_node.pixels);
    assert!(
        differing.is_empty(),
        "support-based backdrop staging changed pixels: {}",
        support::describe_differing(&differing)
    );
    assert!(
        !support::differing_pixels(FRAME_WIDTH, &with_support.pixels, &plain.pixels).is_empty(),
        "the glass surfaces must visibly render over the patterned page"
    );
    let jobs =
        |stats: &cranpose_render_wgpu::RenderStatsSnapshot| stats.pass_count + stats.copy_count;
    eprintln!(
        "glass stages {} -> {}, passes and copies {} -> {}",
        conservative_stats.stages,
        supported_stats.stages,
        jobs(&conservative_stats),
        jobs(&supported_stats)
    );
    assert!(
        supported_stats.stages < conservative_stats.stages,
        "disjoint actual output support should allow one fewer stage ({} vs {})",
        supported_stats.stages,
        conservative_stats.stages
    );
    assert!(
        jobs(&supported_stats) < jobs(&conservative_stats),
        "sharing the capture stage should remove side and capture work ({} vs {} passes and \
         copies)",
        jobs(&supported_stats),
        jobs(&conservative_stats)
    );
}
