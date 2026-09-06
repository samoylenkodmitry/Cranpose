mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_liquid::prelude::*;
use cranpose_macros::composable;
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    Brush, Color, LIQUID_GLASS_SPECIALIZATIONS, Point, RenderEffect, TileMode,
};

const VIEW_WIDTH: f32 = 360.0;
const VIEW_HEIGHT: f32 = 240.0;
const SCALE: f32 = 2.0;
const FRAME_WIDTH: u32 = (VIEW_WIDTH * SCALE) as u32;
const FRAME_HEIGHT: u32 = (VIEW_HEIGHT * SCALE) as u32;
const TOGGLE: &str = "CRANPOSE_NO_SHADER_SPECIALIZATION";
const NO_SPLIT: &str = "CRANPOSE_NO_GLASS_SPLIT_SCISSORS";

/// The showcase's card material: refracting, dispersing, adaptive frost,
/// no blur.
fn card_glass() -> Glass {
    Glass::regular()
        .shape(LiquidShape::RoundedRect(20.0))
        .blur_radius(0.0)
        .refraction_depth(0.58)
        .refraction_curve(0.62)
        .dispersion(1.0)
        .transmission_refraction(0.72)
        .highlight(0.72)
        .adaptive_frost(Color::WHITE, 0.42)
}

#[composable]
#[allow(non_snake_case)]
fn GlassCardScene() {
    LiquidTheme(
        LiquidThemeSpec {
            scheme: SchemeMode::Dark,
            ..LiquidThemeSpec::default()
        },
        move || {
            Box(
                Modifier::empty().fill_max_size().draw_behind(|scope| {
                    let size = scope.size();
                    scope.draw_rect(Brush::radial_gradient_stops(
                        vec![
                            (0.0, Color::from_rgb_u8(24, 20, 46)),
                            (0.55, Color::from_rgb_u8(11, 10, 26)),
                            (1.0, Color::from_rgb_u8(4, 4, 10)),
                        ],
                        Point::new(size.width * 0.5, size.height * 0.1),
                        size.width.max(size.height) * 0.95,
                        TileMode::Clamp,
                    ));
                    for i in 0..40u32 {
                        let x = (i as f32 * 53.7) % size.width;
                        let y = (i as f32 * 29.3) % size.height;
                        scope.draw_circle(
                            Brush::solid(Color::from_rgba_u8(
                                255,
                                255,
                                255,
                                120 + (i % 5) as u8 * 20,
                            )),
                            Point::new(x, y),
                            1.0 + (i % 3) as f32,
                        );
                    }
                }),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .offset(30.0, 40.0)
                            .width(300.0)
                            .height(120.0),
                        BoxSpec::default(),
                        || {
                            GlassSurface(Modifier::empty().fill_max_size(), card_glass(), || {});
                        },
                    );
                },
            );
        },
    );
}

fn capture_card(unspecialized: bool) -> Result<CapturedFrame, String> {
    capture_card_and_stats_under(unspecialized.then_some(TOGGLE)).map(|(frame, _)| frame)
}

fn capture_card_and_stats() -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    capture_card_and_stats_under(None)
}

/// The toggles are process-global, so one raised for a capture is raised
/// only while this capture holds the GPU lock: set after the lock, cleared
/// before it is released, or a concurrent capture in this binary renders
/// under it.
fn capture_card_and_stats_under(
    toggle: Option<&'static str>,
) -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    let (_lock, renderer) = support::headless_renderer_parts()?;
    if let Some(toggle) = toggle {
        cranpose_render_wgpu::set_debug_toggle(toggle, Some("1"));
    }
    let captured = render_card_and_stats(renderer);
    if let Some(toggle) = toggle {
        cranpose_render_wgpu::set_debug_toggle(toggle, None);
    }
    captured
}

fn render_card_and_stats(
    mut renderer: WgpuRenderer,
) -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    let app_context = cranpose_ui::AppContext::new();
    renderer.attach_app_context_services(&app_context);
    let mut shell = AppShell::new(
        renderer,
        location_key(file!(), line!(), column!()),
        GlassCardScene,
    );
    shell.renderer().set_root_scale(SCALE);
    shell.set_density(SCALE);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.set_viewport(VIEW_WIDTH, VIEW_HEIGHT);
    shell.update();
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .map_err(|err| format!("glass card capture failed: {err:?}"))?;
    assert_eq!(
        shell.renderer().device_error_count_for_tests(),
        0,
        "the device recorded a validation error, so the frame is whatever the failed \
         pipeline left behind"
    );
    let stats = shell
        .renderer()
        .last_frame_stats()
        .ok_or_else(|| "the capture recorded no frame stats".to_string())?;
    Ok((frame, stats))
}

/// The card's adaptive frost reads a wide neighbourhood of its capture; the
/// renderer packs that capture averaged to a quarter of its size beside it
/// and the material declares it wants that substrate, so the frame carries
/// exactly one.
#[test]
fn a_card_glass_with_adaptive_frost_is_handed_one_substrate() {
    let (_, stats) = match capture_card_and_stats() {
        Ok(captured) => captured,
        Err(err) => {
            eprintln!("skipping substrate count: {err}");
            return;
        }
    };
    assert_eq!(
        stats.substrates, 1,
        "the card's capture must be averaged into one substrate: {stats:?}"
    );
}

#[test]
fn the_card_material_raises_most_specialization_flags() {
    let RenderEffect::Shader { shader } = cranpose_ui_graphics::liquid_glass_effect(
        &cranpose_ui_graphics::LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: 300.0,
            height: 120.0,
            tint_color: Color(1.0, 1.0, 1.0, 0.08),
        },
        &cranpose_ui_graphics::LiquidGlassSpec::default(),
        300.0,
        120.0,
    ) else {
        panic!("liquid glass must be one runtime shader");
    };
    let raised = shader.overrides().len();
    assert!(
        raised >= LIQUID_GLASS_SPECIALIZATIONS.len() - 3,
        "a plain glass pane leaves almost every optional feature inactive; only {raised} of \
         {} flags were raised: {:?}",
        LIQUID_GLASS_SPECIALIZATIONS.len(),
        shader.overrides()
    );
}

#[test]
fn a_specialized_glass_pipeline_matches_the_general_one_byte_for_byte() {
    let specialized = match capture_card(false) {
        Ok(frame) => frame,
        Err(err) => {
            eprintln!("skipping glass specialization parity: {err}");
            return;
        }
    };
    let general = capture_card(true).expect("headless WGPU init failed mid-suite");
    assert_eq!(specialized.pixels.len(), general.pixels.len());
    let distinct = support::distinct_colors(&specialized.pixels);
    if let Ok(dir) = std::env::var("CRANPOSE_PARITY_DUMP_DIR") {
        let rgb: Vec<u8> = specialized
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect();
        std::fs::write(
            format!("{dir}/glass_parity.ppm"),
            [
                format!("P6 {FRAME_WIDTH} {FRAME_HEIGHT} 255\n").as_bytes(),
                &rgb,
            ]
            .concat(),
        )
        .unwrap();
    }
    assert!(
        distinct > 600,
        "{distinct} distinct colors — the scene must carry a refracted star field, not a \
         flat fill"
    );
    support::assert_same_bytes(
        "material-specialized glass pipeline vs the general one; a raised flag must substitute \
         exactly the value its uniform held",
        FRAME_WIDTH,
        &specialized.pixels,
        &general.pixels,
    );
}

#[test]
fn a_scissor_split_glass_matches_whole_quads_byte_for_byte_and_shades_fewer_pixels() {
    let (split, split_stats) = match capture_card_and_stats() {
        Ok(captured) => captured,
        Err(err) => {
            eprintln!("skipping glass split parity: {err}");
            return;
        }
    };
    let (whole, whole_stats) =
        capture_card_and_stats_under(Some(NO_SPLIT)).expect("headless WGPU init failed mid-suite");
    assert_eq!(
        split_stats.shader_pixels, whole_stats.shader_pixels,
        "the split shades the same composite area once"
    );
    assert!(
        split_stats.glass_rasterized_pixels * 10 < whole_stats.glass_rasterized_pixels * 9,
        "the split must rasterise fewer glass fragments: split {} vs whole {}",
        split_stats.glass_rasterized_pixels,
        whole_stats.glass_rasterized_pixels
    );
    support::assert_same_bytes(
        "rim bands and inset interior scissors vs whole quads; a scissor only removes \
         fragments the shader discards, never one it shades",
        FRAME_WIDTH,
        &split.pixels,
        &whole.pixels,
    );
}
