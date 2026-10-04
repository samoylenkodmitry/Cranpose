use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_liquid::prelude::*;
use cranpose_macros::composable;
use cranpose_render_common::{
    Renderer,
    graph::{LayerNode, RenderNode},
};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    Brush, Color, DrawScope, LIQUID_GLASS_SPECIALIZATIONS, LIQUID_GLASS_WGSL, Point, RenderEffect,
    RuntimeShader, ShaderTarget, ShaderWarmUp, TileMode,
};

use crate::support;

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

/// A night-sky gradient with forty stars, for the glass to refract.
fn star_field(scope: &mut dyn DrawScope) {
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
            Brush::solid(Color::from_rgba_u8(255, 255, 255, 120 + (i % 5) as u8 * 20)),
            Point::new(x, y),
            1.0 + (i % 3) as f32,
        );
    }
}

/// `content` over the star field, in the dark Liquid theme.
fn night_sky(content: impl FnMut() + 'static) {
    LiquidTheme(
        LiquidThemeSpec {
            scheme: SchemeMode::Dark,
            ..LiquidThemeSpec::default()
        },
        move || {
            Box(
                Modifier::empty().fill_max_size().draw_behind(star_field),
                BoxSpec::default(),
                content,
            );
        },
    );
}

#[composable]
fn GlassCardScene(button: bool) {
    night_sky(move || {
        Box(
            Modifier::empty()
                .offset(30.0, 40.0)
                .width(300.0)
                .height(120.0),
            BoxSpec::default(),
            move || {
                GlassSurface(Modifier::empty().fill_max_size(), card_glass(), move || {
                    if button {
                        GlassIconButton(
                            Modifier::empty(),
                            GlassButtonSpec::glass(),
                            40.0,
                            || {},
                            icons::STAR,
                        );
                    }
                });
            },
        );
    });
}

fn capture_card(unspecialized: bool) -> Result<CapturedFrame, String> {
    capture_glass_under(unspecialized.then_some(TOGGLE), false).map(|(frame, _)| frame)
}

fn capture_card_and_stats() -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    capture_glass_under(None, false)
}

/// The toggles are process-global, so one raised for a capture is raised
/// only while this capture holds the GPU lock: set after the lock, cleared
/// before it is released, or a concurrent capture in this binary renders
/// under it.
fn capture_glass_under(
    toggle: Option<&'static str>,
    button: bool,
) -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    let (_lock, renderer) = support::headless_renderer_parts()?;
    if let Some(toggle) = toggle {
        cranpose_render_wgpu::set_debug_toggle(toggle, Some("1"));
    }
    let captured = settle(
        &mut card_shell(renderer, button),
        (FRAME_WIDTH, FRAME_HEIGHT),
    );
    if let Some(toggle) = toggle {
        cranpose_render_wgpu::set_debug_toggle(toggle, None);
    }
    captured
}

fn card_shell(renderer: WgpuRenderer, button: bool) -> AppShell<WgpuRenderer> {
    scene_shell(renderer, (VIEW_WIDTH, VIEW_HEIGHT), SCALE, move || {
        GlassCardScene(button);
    })
}

/// A shell composing `content` in a `view` of logical pixels drawn at
/// `scale`.
fn scene_shell(
    mut renderer: WgpuRenderer,
    view: (f32, f32),
    scale: f32,
    content: impl FnMut() + 'static,
) -> AppShell<WgpuRenderer> {
    let app_context = cranpose_ui::AppContext::new();
    renderer.attach_app_context_services(&app_context);
    let mut shell = AppShell::new(renderer, location_key(file!(), line!(), column!()), content);
    shell.renderer().set_root_scale(scale);
    shell.set_density(scale);
    shell.set_buffer_size((view.0 * scale) as u32, (view.1 * scale) as u32);
    shell.set_viewport(view.0, view.1);
    shell.update();
    shell.update();
    shell
}

fn capture_card_frame(
    shell: &mut AppShell<WgpuRenderer>,
) -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    capture_scene_frame(shell, (FRAME_WIDTH, FRAME_HEIGHT))
}

fn capture_scene_frame(
    shell: &mut AppShell<WgpuRenderer>,
    (width, height): (u32, u32),
) -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    let frame = shell
        .renderer()
        .capture_frame(width, height)
        .map_err(|err| format!("glass capture failed: {err:?}"))?;
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

const SETTLE: std::time::Duration = std::time::Duration::from_secs(30);

/// Captures `shell` until every glass draw uses the specialization it
/// asked for, so the statistics describe the settled frame.
fn settle(
    shell: &mut AppShell<WgpuRenderer>,
    size: (u32, u32),
) -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    let deadline = std::time::Instant::now() + SETTLE;
    loop {
        let (frame, stats) = capture_scene_frame(shell, size)?;
        if stats.shader_pipeline_fallback_draws == 0 {
            return Ok((frame, stats));
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the glass specializations never finished compiling"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The runtime shader a single-pass glass effect draws with.
fn glass_material(effect: RenderEffect) -> RuntimeShader {
    match effect {
        RenderEffect::Shader { shader } => std::sync::Arc::unwrap_or_clone(shader),
        other => panic!("a single-pass glass material, not {other:?}"),
    }
}

/// What a stand-in compiles for a first screen: the glass folded only to
/// the overrides all its materials share. Each fold is exact on its own, so
/// every material drawn with that pipeline lands on its own pipeline's
/// bytes, interior and rim alike.
#[test]
fn a_glass_folded_only_to_what_its_neighbours_share_keeps_its_bytes() {
    use support::glass_page::{FRAME_HEIGHT, FRAME_WIDTH, GLASS_HEIGHT, GLASS_WIDTH, panes_page};
    let _folds = support::glass_page::GlassFolds::set(true);
    let frosted = cranpose_ui_graphics::liquid_glass_effect(
        &cranpose_ui_graphics::LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: GLASS_WIDTH,
            height: GLASS_HEIGHT,
            tint_color: Color(1.0, 1.0, 1.0, 0.12),
        },
        &cranpose_ui_graphics::LiquidGlassSpec {
            blur_radius: 4.0,
            ..cranpose_ui_graphics::LiquidGlassSpec::default()
        },
        GLASS_WIDTH,
        GLASS_HEIGHT,
    );
    let loupe = cranpose_ui_graphics::liquid_loupe_effect(
        (GLASS_WIDTH, GLASS_HEIGHT),
        &cranpose_ui_graphics::LiquidLoupeSpec::default(),
    );
    let materials = [support::glass_page::glass_shader(), frosted, loupe].map(glass_material);
    let shared: Vec<(&'static str, f64)> = materials[0]
        .overrides()
        .iter()
        .copied()
        .filter(|&(name, value)| {
            materials.iter().all(|material| {
                material
                    .overrides()
                    .iter()
                    .any(|&(own, fixed)| own == name && fixed.to_bits() == value.to_bits())
            })
        })
        .collect();
    assert!(
        !shared.is_empty()
            && materials
                .iter()
                .all(|material| material.overrides().len() > shared.len()),
        "the materials share some folds, and each folds more on its own"
    );
    let mut renderer = support::headless_renderer().expect("GPU required for glass parity");
    let plain = support::capture_graph(&mut renderer, panes_page([]), FRAME_WIDTH, FRAME_HEIGHT);
    for (index, material) in materials.iter().enumerate() {
        let mut stand_in = material.clone();
        for &(name, value) in material.overrides() {
            if !shared.contains(&(name, value)) {
                stand_in.clear_override(name);
            }
        }
        assert_eq!(stand_in.draw_split(), material.draw_split());
        let own = support::capture_graph(
            &mut renderer,
            panes_page([RenderEffect::runtime_shader(material.clone())]),
            FRAME_WIDTH,
            FRAME_HEIGHT,
        );
        let standing_in = support::capture_graph(
            &mut renderer,
            panes_page([RenderEffect::runtime_shader(stand_in)]),
            FRAME_WIDTH,
            FRAME_HEIGHT,
        );
        assert!(own.pixels != plain.pixels, "material {index} must draw");
        support::assert_same_bytes(
            &format!("material {index} drawn with only the shared folds"),
            FRAME_WIDTH,
            &own.pixels,
            &standing_in.pixels,
        );
    }
}

/// The first frame of the card that draws its glass, not a placeholder
/// while the glass's blur pipelines compile. A material's own pipeline is
/// asked for by its first draw, so this frame stands in.
fn first_drawn_frame(shell: &mut AppShell<WgpuRenderer>) -> (CapturedFrame, RenderStatsSnapshot) {
    let deadline = std::time::Instant::now() + SETTLE;
    loop {
        let (frame, stats) = capture_card_frame(shell).expect("capture");
        if stats.placeholder_draws == 0 {
            return (frame, stats);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the glass pipelines never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Asks renderers to build the glass shader's general pipelines on their
/// background compilers.
fn request_glass_general() {
    cranpose_ui_graphics::request_shader_warm_ups([ShaderTarget::Page, ShaderTarget::Layer].map(
        |target| ShaderWarmUp {
            shader: RuntimeShader::new(LIQUID_GLASS_WGSL),
            target,
        },
    ));
}

/// The general draws a glass several times slower than its own pipelines
/// (~300 ms frames on a Mali), so even with it built a new card material
/// shows its placeholder until its own pipelines land, and never the
/// general.
#[test]
fn a_new_glass_waits_for_its_own_pipelines_rather_than_draw_with_the_general() {
    request_glass_general();
    let Ok((_lock, renderer)) = support::headless_renderer_parts_compiling_in_background() else {
        eprintln!("skipping glass pipeline readiness: no headless renderer");
        return;
    };
    let mut shell = card_shell(renderer, false);
    support::wait_for_background_compiler_idle();
    let (_, waiting) = capture_card_frame(&mut shell).expect("capture");
    assert!(
        waiting.placeholder_draws > 0,
        "the first frame shows the placeholder: {waiting:?}"
    );
    let (_, drawn) = first_drawn_frame(&mut shell);
    assert_eq!(drawn.shader_pipeline_fallback_draws, 0, "{drawn:?}");
    assert!(drawn.shader_specialized_draws > 0, "{drawn:?}");
}

/// The card's adaptive frost reads a wide neighbourhood of its capture; the
/// renderer packs that capture averaged to a quarter of its size beside it
/// and the material declares it wants that substrate, so the frame carries
/// exactly one. The backdrop cache is off because a replayed capture builds
/// no substrate, and the settled frame is a replay.
#[test]
fn a_card_glass_with_adaptive_frost_is_handed_one_substrate() {
    let (_, stats) = match capture_glass_under(Some("CRANPOSE_NO_BACKDROP_CACHE"), false) {
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
    // Ask for folds rather than inherit them. `liquid_glass_effect` specializes
    // with the process-wide `GLASS_MATERIAL_FOLDS`, which is false off Android
    // until something calls `set_glass_material_folds(true)` -- which
    // `tests/support.rs` does when a sibling here builds a renderer. This test
    // passed only because it ran after one of them in the same process: alone
    // it raised 0 of 20 flags. Every test of the folds inside
    // cranpose-ui-graphics already asks the same way.
    let mut shader = (*shader).clone();
    cranpose_ui_graphics::specialize_liquid_glass_with_folds(&mut shader, true);
    let raised = shader.overrides().len();
    assert!(
        raised >= LIQUID_GLASS_SPECIALIZATIONS.len() - 3,
        "a card material with folds on should raise almost every optional feature; only \
         {raised} of {} were raised: {:?}",
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
        capture_glass_under(Some(NO_SPLIT), false).expect("headless WGPU init failed mid-suite");
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

#[test]
fn a_floating_button_draws_the_same_with_its_general_and_its_own_pipelines() {
    let general = match capture_glass_under(Some(TOGGLE), true) {
        Ok((frame, _)) => frame,
        Err(err) => {
            eprintln!("skipping floating glass parity: {err}");
            return;
        }
    };
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping floating glass parity: no headless renderer");
        return;
    };
    let mut shell = card_shell(renderer, true);
    let (settled, settled_stats) =
        settle(&mut shell, (FRAME_WIDTH, FRAME_HEIGHT)).expect("settled capture");
    assert!(settled_stats.shader_specialized_draws > 0);
    support::assert_same_bytes(
        "the floating button's glass, glyph and ring shadow with its own pipelines",
        FRAME_WIDTH,
        &general.pixels,
        &settled.pixels,
    );
    let graph = shell
        .renderer()
        .scene_mut()
        .graph
        .as_mut()
        .expect("button graph");
    assert!(expand_content_masks(&mut graph.root) > 0);
    let (expanded, _) = capture_card_frame(&mut shell).expect("expanded mask capture");
    support::assert_same_bytes(
        "the content mask ends at its silhouette even when the material casts a wide shadow",
        FRAME_WIDTH,
        &settled.pixels,
        &expanded.pixels,
    );
}

fn expand_content_masks(layer: &mut LayerNode) -> usize {
    let mut expanded = 0;
    if let Some(RenderEffect::Shader { shader }) = &mut layer.graphics_layer.render_effect
        && shader.uniforms().get(112).is_some_and(|mask| *mask > 0.5)
    {
        let shader = std::sync::Arc::make_mut(shader);
        shader.set_output_support(None);
        shader.set_output_padding(64.0);
        expanded += 1;
    }
    for child in &mut layer.children {
        if let RenderNode::Layer(child) = child {
            expanded += expand_content_masks(child);
        }
    }
    expanded
}

/// The star field under one card-material glass filling the view.
#[composable]
fn GlassPanelScene() {
    night_sky(|| GlassSurface(Modifier::empty().fill_max_size(), card_glass(), || {}));
}

/// The settled panel scene in a `width` by `height` frame at scale one,
/// specialized with folding `folds`.
fn settled_panel(
    width: u32,
    height: u32,
    folds: bool,
) -> Result<(CapturedFrame, RenderStatsSnapshot), String> {
    let (_lock, renderer) = support::headless_renderer_parts()?;
    cranpose_ui_graphics::set_glass_material_folds(folds);
    let mut shell = scene_shell(
        renderer,
        (width as f32, height as f32),
        1.0,
        GlassPanelScene,
    );
    let settled = settle(&mut shell, (width, height));
    cranpose_ui_graphics::set_glass_material_folds(true);
    settled
}

/// Where folding is off, a glass covering more than a 1080p frame still
/// takes its folded pipeline, drawing its interior and rim apart as the
/// folded platforms do and landing on their bytes, while a smaller one keeps
/// drawing whole.
#[test]
fn a_glass_larger_than_a_1080p_frame_folds_where_folding_is_off() {
    let (large, large_stats) = match settled_panel(1920, 1200, false) {
        Ok(settled) => settled,
        Err(err) => {
            eprintln!("skipping large glass folds: {err}");
            return;
        }
    };
    let (folded, folded_stats) =
        settled_panel(1920, 1200, true).expect("headless WGPU init failed mid-suite");
    support::assert_same_bytes(
        "the folded pipeline changed the large glass",
        1920,
        &folded.pixels,
        &large.pixels,
    );
    assert_eq!(
        large_stats.glass_rasterized_pixels, folded_stats.glass_rasterized_pixels,
        "a glass over a whole 1920x1200 frame must draw its folded interior and rim where \
         folding is off: {large_stats:?}"
    );
    let (_, small_stats) =
        settled_panel(640, 400, false).expect("headless WGPU init failed mid-suite");
    assert_eq!(
        small_stats.glass_rasterized_pixels, small_stats.shader_pixels,
        "a glass under a 1080p frame keeps drawing whole where folding is off: {small_stats:?}"
    );
}
