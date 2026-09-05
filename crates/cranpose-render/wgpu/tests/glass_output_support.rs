mod support;

use cranpose_liquid::{
    Glass, GlassDeformation, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape,
    LiquidTheme, LiquidThemeSpec,
};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui_graphics::{Brush, Point, TileMode};
use support::page::*;

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
    GlassDynamics {
        activity: Some(1.0),
        morph: Some(GlassMorph {
            node_size: (NODE[2], NODE[3]),
            primary: (140.0, 80.0, 120.0, 40.0, -1.0),
            shapes: Vec::new(),
            glue: 0.0,
            wobble_amplitude: 1.0,
            wobble_phase: 0.4,
            bulge_amplitude: 2.0,
            bulge_direction: 0.7,
            ellipse_blend: 0.5,
            deformation: Some(GlassDeformation::incompressible((1.0, 0.0), 1.05)),
            zoom_anchor: (0.0, 0.0),
        }),
        ..Default::default()
    }
}

#[composable]
#[allow(non_snake_case)]
fn LensPage() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        FramePage(
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Color(0.1, 0.1, 0.16, 1.0),
            || {
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
                    || {
                        Box(
                            rect_modifier(NODE).glass_effect_with(lens_glass(), lens_dynamics),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    });
}

fn capture(whole_node: bool) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, whole_node.then_some("1"));
    let captured = capture_with_current_toggles();
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);
    captured
}

fn capture_with_current_toggles() -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    support::warm_app_frame(LensPage, FRAME_WIDTH, FRAME_HEIGHT)
}

#[test]
fn a_lens_composited_within_its_declared_support_is_the_lens_composited_over_its_whole_node() {
    let Some((within_support, with_support)) = capture(false) else {
        return;
    };
    let (whole_node, without_support) = capture(true).expect("headless WGPU init failed mid-suite");
    let differing: Vec<(usize, usize, [u8; 4], [u8; 4])> = within_support
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(whole_node.pixels.as_chunks::<4>().0)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(index, (a, b))| {
            (
                index % FRAME_WIDTH as usize,
                index / FRAME_WIDTH as usize,
                *a,
                *b,
            )
        })
        .collect();
    let bbox = differing
        .iter()
        .fold((usize::MAX, usize::MAX, 0, 0), |b, (x, y, _, _)| {
            (b.0.min(*x), b.1.min(*y), b.2.max(*x), b.3.max(*y))
        });
    assert!(
        differing.is_empty(),
        "{} pixels differ between the lens composited within its declared output support \
         and over its whole node; bbox x {}..={} y {}..={}; first {:?}",
        differing.len(),
        bbox.0,
        bbox.2,
        bbox.1,
        bbox.3,
        &differing[..differing.len().min(6)]
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
