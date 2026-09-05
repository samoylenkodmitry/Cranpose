mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{ProjectiveTransform, RenderGraph, RenderNode},
    image_compare::image_difference_stats,
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    Color, GraphicsLayer, LayerShape, LiquidGlassRect, LiquidGlassSpec,
    RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RoundedCornerShape, RuntimeShader,
    SubstrateSpec, liquid_glass_effect,
};
use support::{SubstrateProbeRead, region_pixels, solid_rect};

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 120;
const GLASS_WIDTH: f32 = 56.0;
const GLASS_HEIGHT: f32 = 40.0;
const GLASS_TOP: f32 = 30.0;
const GLASS_PITCH: f32 = 72.0;
const GLASS_LEFT: f32 = 12.0;
const GLASS_RADIUS: f32 = 12.0;
const BLUR_RADIUS: f32 = 6.0;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn striped_page() -> Vec<RenderNode> {
    support::striped_page(FRAME_WIDTH, FRAME_HEIGHT)
}

fn glass_shader() -> RenderEffect {
    liquid_glass_effect(
        &LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: GLASS_WIDTH,
            height: GLASS_HEIGHT,
            tint_color: Color(1.0, 1.0, 1.0, 0.12),
        },
        &LiquidGlassSpec::default(),
        GLASS_WIDTH,
        GLASS_HEIGHT,
    )
}

fn unbatched(effect: RenderEffect) -> RenderEffect {
    match effect {
        RenderEffect::Shader { mut shader } => {
            shader.set_batched_source(false);
            RenderEffect::Shader { shader }
        }
        RenderEffect::Chain { first, second } => RenderEffect::Chain {
            first: Box::new(unbatched(*first)),
            second: Box::new(unbatched(*second)),
        },
        other => other,
    }
}

/// Content that stays clear of the rounded corners, so the layer draws
/// directly and its backdrop joins the parent's stages.
fn inset_content(width: f32, height: f32, radius: f32) -> RenderNode {
    let inset = radius + 1.0;
    solid_rect(
        rect(inset, inset, width - 2.0 * inset, height - 2.0 * inset),
        Color::from_rgba_u8(255, 255, 255, 40),
    )
}

fn glass_layer(index: usize, effect: RenderEffect) -> RenderNode {
    let bounds = rect(0.0, 0.0, GLASS_WIDTH, GLASS_HEIGHT);
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::translation(GLASS_LEFT + index as f32 * GLASS_PITCH, GLASS_TOP),
        GraphicsLayer {
            backdrop_effect: Some(effect),
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(GLASS_RADIUS)),
            ..GraphicsLayer::default()
        },
        vec![inset_content(GLASS_WIDTH, GLASS_HEIGHT, GLASS_RADIUS)],
    )))
}

fn glasses_page(count: usize, effect: impl Fn() -> RenderEffect) -> RenderGraph {
    let mut children = striped_page();
    for index in 0..count {
        children.push(glass_layer(index, effect()));
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn capture(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> CapturedFrame {
    support::capture_graph(renderer, graph, FRAME_WIDTH, FRAME_HEIGHT)
}

/// The first glass with a margin around it: its own pixels plus the page
/// beside it, where a neighbour's texels or an over-wide scissor would show.
fn first_glass_region() -> Rect {
    rect(
        GLASS_LEFT - 8.0,
        GLASS_TOP - 8.0,
        GLASS_WIDTH + 16.0,
        GLASS_HEIGHT + 16.0,
    )
}

fn assert_region_matches(
    label: &str,
    alone: &CapturedFrame,
    packed: &CapturedFrame,
    tolerance: u32,
) {
    let region = first_glass_region();
    let alone_pixels = region_pixels(alone, region);
    let packed_pixels = region_pixels(packed, region);
    let stats = image_difference_stats(
        &alone_pixels,
        &packed_pixels,
        region.width as u32,
        region.height as u32,
        tolerance,
    );
    let max_channel_delta = support::max_channel_delta(&alone_pixels, &packed_pixels);
    assert!(
        u32::from(max_channel_delta) <= tolerance,
        "{label}: max_channel_delta={max_channel_delta} {stats:?}",
    );
}

fn glass_has_content(frame: &CapturedFrame) {
    let region = first_glass_region();
    let pixels = region_pixels(frame, region);
    let distinct = support::distinct_colors(&pixels);
    assert!(
        distinct > 8,
        "the glass region must show structured content, saw {distinct} colours"
    );
}

/// Renders one glass alone and packed beside two others and asserts the
/// first glass matches within `tolerance`.
fn assert_packed_parity(label: &str, effect: impl Fn() -> RenderEffect, tolerance: u32) {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping atlas parity: {err}");
            return;
        }
    };
    let alone = capture(&mut renderer, glasses_page(1, &effect));
    let packed = capture(&mut renderer, glasses_page(3, &effect));
    glass_has_content(&alone);
    assert_region_matches(label, &alone, &packed, tolerance);
}

#[test]
fn a_blurred_glass_renders_the_same_pixels_alone_and_packed_beside_others() {
    assert_packed_parity(
        "blurred glass packed beside two others",
        || RenderEffect::blur(BLUR_RADIUS),
        0,
    );
}

/// Packed beside others the shader reads its region through a float
/// mapping, which moves a tap by a few ulps and a pixel by at most one
/// 8-bit step.
const REGION_MAPPING_TOLERANCE: u32 = 1;

#[test]
fn a_shader_glass_renders_the_same_pixels_alone_and_packed_beside_others() {
    assert_packed_parity(
        "shader glass packed beside two others",
        glass_shader,
        REGION_MAPPING_TOLERANCE,
    );
}

#[test]
fn a_blur_then_shader_glass_renders_the_same_pixels_alone_and_packed_beside_others() {
    assert_packed_parity(
        "blur-then-shader glass packed beside two others",
        || RenderEffect::blur(BLUR_RADIUS).then(glass_shader()),
        REGION_MAPPING_TOLERANCE,
    );
}

/// A shader that paints the reserved slots it is handed: the logical size
/// its source region stands for in red and green, the region's height in
/// blue. Read just inside the glass's top edge, clear of its inset content.
fn probe_shader() -> RenderEffect {
    let mut shader = RuntimeShader::new(&format!(
        "{}\n{}",
        RUNTIME_SHADER_PRELUDE_WGSL,
        r#"@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let logical = u[63u].xy;
    let region = u[59u];
    return vec4<f32>(logical.x / 255.0, logical.y / 255.0, region.w / 255.0, 1.0);
}
"#
    ));
    shader.set_batched_source(true);
    RenderEffect::Shader { shader }
}

fn pixel_at(frame: &CapturedFrame, x: f32, y: f32) -> [u8; 4] {
    let pixels = region_pixels(frame, rect(x, y, 1.0, 1.0));
    [pixels[0], pixels[1], pixels[2], pixels[3]]
}

fn split_name_probe(name: &'static str) -> RenderEffect {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}",
        r#"
override FIRST: i32 = 0;
override SECOND: i32 = 0;
@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let part = max(FIRST, SECOND);
    if (part == 1 && input.uv.x >= 0.5) || (part == 2 && input.uv.x < 0.5) {
        discard;
    }
    return select(vec4<f32>(0.0, 0.0, 1.0, 1.0), vec4<f32>(1.0, 0.0, 0.0, 1.0), FIRST != 0);
}
"#
    ));
    shader.set_batched_source(true);
    shader.set_draw_split(Some(name));
    RenderEffect::Shader { shader }
}

#[test]
fn split_override_names_select_distinct_compiled_pipelines() {
    let mut renderer = support::headless_renderer().expect("headless renderer");
    for (name, expected) in [("FIRST", [255, 0, 0, 255]), ("SECOND", [0, 0, 255, 255])] {
        let frame = capture(&mut renderer, glasses_page(1, || split_name_probe(name)));
        assert_eq!(
            pixel_at(&frame, GLASS_LEFT + GLASS_WIDTH / 2.0, GLASS_TOP + 4.0),
            expected,
            "the {name} override must select its own split pipelines"
        );
    }
}

fn active_glass(activity: f32, rim_style: f32, specialized: bool) -> RenderEffect {
    let RenderEffect::Shader { shader: base } = glass_shader() else {
        panic!("glass shader");
    };
    let mut shader = RuntimeShader::new(base.source());
    for (index, value) in base.uniforms().iter().copied().enumerate() {
        shader.set_float(index, value);
    }
    shader.set_float(cranpose_ui_graphics::GLASS_ACTIVITY_UNIFORM, activity);
    shader.set_float(6, 4.0);
    shader.set_float(28, rim_style);
    shader.set_float(9, 0.65);
    shader.set_float(cranpose_ui_graphics::GLASS_DISPERSION_UNIFORM, 0.8);
    shader.set_batched_source(true);
    if specialized {
        cranpose_ui_graphics::specialize_liquid_glass(&mut shader);
    }
    RenderEffect::blur(BLUR_RADIUS).then(RenderEffect::Shader { shader })
}

#[test]
fn split_glass_preserves_resting_partial_and_full_activity() {
    let mut renderer = support::headless_renderer().expect("headless renderer");
    let resting = capture(
        &mut renderer,
        glasses_page(1, || active_glass(0.0, 0.0, false)),
    );
    let full = capture(
        &mut renderer,
        glasses_page(1, || active_glass(1.0, 0.0, false)),
    );
    assert_ne!(
        resting.pixels, full.pixels,
        "activity must change the rendered glass"
    );
    for activity in [0.0, 0.4, 1.0] {
        for rim_style in [0.0, 0.5, 1.0] {
            let reference = capture(
                &mut renderer,
                glasses_page(1, || active_glass(activity, rim_style, false)),
            );
            let specialized = capture(
                &mut renderer,
                glasses_page(1, || active_glass(activity, rim_style, true)),
            );
            support::assert_same_bytes(
                &format!("glass activity={activity} rim_style={rim_style}"),
                FRAME_WIDTH,
                &reference.pixels,
                &specialized.pixels,
            );
        }
    }
}

/// A shader after a blur reads the blur's downscaled result and is told the
/// capture size that result stands for, so its pixel-calibrated offsets
/// keep their length; without that size it would calibrate to the
/// downscaled texels and displace every ray by the downscale.
#[test]
fn a_shader_after_a_blur_is_told_the_size_its_downscaled_source_stands_for() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping logical size probe: {err}");
            return;
        }
    };
    let effect = || RenderEffect::blur(BLUR_RADIUS).then(probe_shader());
    let padding = (effect().input_padding() + effect().output_padding()).ceil();
    let capture_size = (GLASS_WIDTH + 2.0 * padding, GLASS_HEIGHT + 2.0 * padding);
    let downscaled_height = (capture_size.1 / 2.0).ceil();
    let frame = capture(&mut renderer, glasses_page(1, effect));
    let probe = pixel_at(&frame, GLASS_LEFT + GLASS_WIDTH / 2.0, GLASS_TOP + 4.0);
    assert_eq!(
        [probe[0], probe[1], probe[2]],
        [
            capture_size.0 as u8,
            capture_size.1 as u8,
            downscaled_height as u8
        ],
        "the probe paints the logical size and its region's height: {probe:?}"
    );
}

/// The mean of the page's 4 x 4 block of pixels at `block` of the first
/// glass's capture, per channel.
fn block_mean(page: &CapturedFrame, block: (usize, usize)) -> [f32; 3] {
    let pixels = region_pixels(
        page,
        rect(
            GLASS_LEFT + 4.0 * block.0 as f32,
            GLASS_TOP + 4.0 * block.1 as f32,
            4.0,
            4.0,
        ),
    );
    let mut mean = [0.0f32; 3];
    for pixel in pixels.chunks(4) {
        for (channel, value) in pixel.iter().take(3).enumerate() {
            mean[channel] += f32::from(*value) / 16.0;
        }
    }
    mean
}

/// The substrate a batched shader is handed is its capture at a quarter of
/// its resolution, every texel the mean of a 4 x 4 block, packed in the
/// same texture: the probe paints each pixel's block mean, read from the
/// page the glass captures, within the rounding of the average and its
/// texel.
#[test]
fn a_shader_reads_its_capture_averaged_into_blocks_of_four_through_the_substrate_slot() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping substrate probe: {err}");
            return;
        }
    };
    let page = capture(&mut renderer, glasses_page(0, glass_shader));
    let frame = capture(
        &mut renderer,
        glasses_page(1, || {
            support::substrate_probe(
                SubstrateSpec::Average { block: 4 },
                SubstrateProbeRead::BlockTexel,
            )
        }),
    );
    for y in 2..12 {
        for x in 14..(GLASS_WIDTH as usize - 14) {
            let actual = pixel_at(&frame, GLASS_LEFT + x as f32, GLASS_TOP + y as f32);
            let expected = block_mean(&page, (x / 4, y / 4));
            for channel in 0..3 {
                let delta = (f32::from(actual[channel]) - expected[channel]).abs();
                assert!(
                    delta <= 2.0,
                    "the probe at ({x}, {y}) paints {actual:?}, its block averages {expected:?}"
                );
            }
        }
    }
}

/// A blurred glass reads its blur downscaled, and its rounded mask is
/// measured in the pixels that read stands for: the corner outside the
/// rounding shows the page, as it does for a plain blurred layer.
#[test]
fn a_blurred_glass_keeps_the_page_outside_its_rounded_corners() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping rounded corner check: {err}");
            return;
        }
    };
    let page = capture(&mut renderer, glasses_page(0, glass_shader));
    let glass = capture(
        &mut renderer,
        glasses_page(1, || RenderEffect::blur(BLUR_RADIUS).then(glass_shader())),
    );
    glass_has_content(&glass);
    for (x, y) in [
        (GLASS_LEFT, GLASS_TOP),
        (
            GLASS_LEFT + GLASS_WIDTH - 1.0,
            GLASS_TOP + GLASS_HEIGHT - 1.0,
        ),
    ] {
        assert_eq!(
            pixel_at(&glass, x, y),
            pixel_at(&page, x, y),
            "the page shows through the glass's rounded corner at ({x}, {y})"
        );
    }
    let centre = pixel_at(
        &glass,
        GLASS_LEFT + GLASS_WIDTH / 2.0,
        GLASS_TOP + GLASS_HEIGHT / 2.0,
    );
    assert_ne!(
        centre,
        pixel_at(
            &page,
            GLASS_LEFT + GLASS_WIDTH / 2.0,
            GLASS_TOP + GLASS_HEIGHT / 2.0
        ),
        "the glass shades its centre"
    );
}

/// The shader applies its rounded clip and alpha itself when drawn into the
/// final pass; the reference is the same shader resolved into its own
/// texture and blitted through the renderer's rounded mask.
#[test]
fn a_shader_glass_drawn_in_the_final_pass_matches_its_masked_blit_reference() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping mask parity: {err}");
            return;
        }
    };
    let batched = capture(&mut renderer, glasses_page(2, glass_shader));
    let reference = capture(&mut renderer, glasses_page(2, || unbatched(glass_shader())));
    let batched_stats = renderer.last_frame_stats().expect("stats");
    glass_has_content(&batched);
    assert_region_matches(
        "in-shader mask against the masked blit",
        &reference,
        &batched,
        12,
    );
    let corner = first_glass_region();
    let outside_corner = region_pixels(&batched, rect(GLASS_LEFT, GLASS_TOP, 2.0, 2.0));
    let page_corner = region_pixels(&reference, rect(GLASS_LEFT, GLASS_TOP, 2.0, 2.0));
    assert_eq!(
        outside_corner, page_corner,
        "the clipped corner outside the rounded rect must show the page in both: region={corner:?} stats={batched_stats:?}"
    );
}

/// A child that draws nothing itself and whose effect is one runtime shader
/// draws that shader straight into the final pass, applying the child's
/// alpha itself; the reference is the same child with a shader that cannot
/// take the alpha, which forces the surface path and the alpha blit.
#[test]
fn an_empty_shader_child_drawn_in_the_final_pass_matches_its_surface_resolve() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping shader child parity: {err}");
            return;
        }
    };
    const CHILD_ALPHA: f32 = 0.6;
    let child = |effect: RenderEffect| {
        let bounds = rect(0.0, 0.0, GLASS_WIDTH, GLASS_HEIGHT);
        let mut children = striped_page();
        children.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                bounds,
                ProjectiveTransform::translation(GLASS_LEFT, GLASS_TOP),
                GraphicsLayer {
                    render_effect: Some(effect),
                    alpha: CHILD_ALPHA,
                    ..GraphicsLayer::default()
                },
                Vec::new(),
            ),
        )));
        RenderGraph::new(shared_test_support::layer_node(
            rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
            ProjectiveTransform::identity(),
            GraphicsLayer::default(),
            children,
        ))
    };
    let tail = capture(&mut renderer, child(glass_shader()));
    let tail_stats = renderer.last_frame_stats().expect("tail stats");
    let surface = capture(&mut renderer, child(unbatched(glass_shader())));
    let surface_stats = renderer.last_frame_stats().expect("surface stats");
    assert_eq!(
        tail_stats.isolated_layer_renders, 0,
        "an empty shader child needs no surface pass: {tail_stats:?}"
    );
    assert_eq!(
        surface_stats.isolated_layer_renders, 1,
        "the reference child must resolve through its surface: {surface_stats:?}"
    );
    glass_has_content(&tail);
    assert_region_matches("shader child tail against its surface", &surface, &tail, 3);
}

const BAND_TOP: f32 = GLASS_TOP;
const BAND_HEIGHT: f32 = GLASS_HEIGHT;

fn band_shader() -> RenderEffect {
    liquid_glass_effect(
        &LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: FRAME_WIDTH as f32,
            height: BAND_HEIGHT,
            tint_color: Color(1.0, 1.0, 1.0, 0.12),
        },
        &LiquidGlassSpec::default(),
        FRAME_WIDTH as f32,
        BAND_HEIGHT,
    )
}

/// `card_count` blurred glass cards over the page, optionally over a
/// page-wide empty shader child spanning the glass row that their captures
/// read.
fn cards_over_band(card_count: usize, band: bool) -> RenderGraph {
    let bounds = rect(0.0, 0.0, FRAME_WIDTH as f32, BAND_HEIGHT);
    let mut children = striped_page();
    if band {
        children.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                bounds,
                ProjectiveTransform::translation(0.0, BAND_TOP),
                GraphicsLayer {
                    render_effect: Some(band_shader()),
                    ..GraphicsLayer::default()
                },
                Vec::new(),
            ),
        )));
    }
    for index in 0..card_count {
        children.push(glass_layer(index, RenderEffect::blur(BLUR_RADIUS)));
    }
    RenderGraph::new(shared_test_support::layer_node(
        rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    ))
}

/// Surface resolves and shader pixels the band adds on top of the cards
/// alone.
fn band_cost(renderer: &mut support::LockedRenderer, card_count: usize) -> (u32, u64) {
    capture(renderer, cards_over_band(card_count, false));
    let cards_only = renderer.last_frame_stats().expect("stats");
    let frame = capture(renderer, cards_over_band(card_count, true));
    glass_has_content(&frame);
    let with_band = renderer.last_frame_stats().expect("stats");
    (
        with_band.isolated_layer_renders - cards_only.isolated_layer_renders,
        with_band.shader_pixels - cards_only.shader_pixels,
    )
}

/// A shader drawn in the final pass is shaded once, in its stratum; every
/// capture above it reads the page it was drawn into, so however many cards
/// read it, it resolves into no surface and shades no pixel twice.
#[test]
fn a_shader_child_read_by_captures_is_shaded_once_on_the_page() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping tail recapture: {err}");
            return;
        }
    };
    let (one_resolves, one_pixels) = band_cost(&mut renderer, 1);
    let (three_resolves, three_pixels) = band_cost(&mut renderer, 3);
    assert_eq!(
        one_resolves, 0,
        "one card reads a corner of the band: it stays a tail"
    );
    assert_eq!(
        three_resolves, 0,
        "three cards read most of the band: it still stays a tail"
    );
    assert_eq!(
        three_pixels, one_pixels,
        "the band is shaded once whether one card or three read it"
    );
}

const BUTTON_SIZE: f32 = 16.0;
const BUTTON_RADIUS: f32 = 4.0;

fn button_shader() -> RenderEffect {
    liquid_glass_effect(
        &LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: BUTTON_SIZE,
            height: BUTTON_SIZE,
            tint_color: Color(1.0, 1.0, 1.0, 0.12),
        },
        &LiquidGlassSpec::default(),
        BUTTON_SIZE,
        BUTTON_SIZE,
    )
}

fn button_layer(x: f32, y: f32, effect: RenderEffect) -> RenderNode {
    let bounds = rect(0.0, 0.0, BUTTON_SIZE, BUTTON_SIZE);
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::translation(x, y),
        GraphicsLayer {
            backdrop_effect: Some(effect),
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(BUTTON_RADIUS)),
            ..GraphicsLayer::default()
        },
        vec![inset_content(BUTTON_SIZE, BUTTON_SIZE, BUTTON_RADIUS)],
    )))
}

/// One shader glass with two shader glass buttons over it, whose captures
/// read the glass.
fn glass_with_buttons(batched: bool) -> RenderGraph {
    let effect = |effect: RenderEffect| if batched { effect } else { unbatched(effect) };
    let mut children = striped_page();
    children.push(glass_layer(0, effect(glass_shader())));
    for offset in [8.0, 32.0] {
        children.push(button_layer(
            GLASS_LEFT + offset,
            GLASS_TOP + 12.0,
            effect(button_shader()),
        ));
    }
    RenderGraph::new(shared_test_support::layer_node(
        rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    ))
}

/// A glass that captures above it read is shaded once into a texture the
/// captures and the final pass blit, never again per capture; its pixels
/// match the same scene resolved through per-effect captures and masked
/// blits.
#[test]
fn a_glass_read_by_captures_above_it_is_shaded_once() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping read tail resolve: {err}");
            return;
        }
    };
    let batched = capture(&mut renderer, glass_with_buttons(true));
    let stats = renderer.last_frame_stats().expect("stats");
    let reference = capture(&mut renderer, glass_with_buttons(false));
    glass_has_content(&batched);
    let shaded_once = (GLASS_WIDTH * GLASS_HEIGHT + 2.0 * BUTTON_SIZE * BUTTON_SIZE) as u64;
    assert_eq!(
        stats.shader_pixels, shaded_once,
        "the glass and its two buttons shade their own pixels once: {stats:?}"
    );
    assert_region_matches(
        "read glass against per-effect captures",
        &reference,
        &batched,
        12,
    );
}

/// A frame's uploads reach the GPU as one write per buffer, however many
/// uniform blocks, chunks and quads it stages: here every glass composite
/// and its blur passes over a page of shapes, in the uniform write, the
/// viewport ring and the arena's tables, whichever of the four the page
/// fills.
#[test]
fn a_frame_of_glasses_stages_its_uploads_in_a_handful_of_writes() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    renderer.scene_mut().graph = Some(glasses_page(3, || {
        RenderEffect::blur(BLUR_RADIUS).then(glass_shader())
    }));
    let first = renderer
        .render_current_scene_to_texture(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("render should succeed");
    assert!(
        first.upload_writes <= 6,
        "three blurred glasses staged {} buffer writes on their first frame, expected at most six",
        first.upload_writes
    );
    let second = renderer
        .render_current_scene_to_texture(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("render should succeed");
    assert!(
        second.upload_writes <= 5,
        "the same page staged {} buffer writes on its second frame, expected at most five",
        second.upload_writes
    );
}

fn stacked_cached_glasses(first_blur: f32, identified: bool) -> RenderGraph {
    let mut children = striped_page();
    for (index, radius) in [first_blur, 2.0].into_iter().enumerate() {
        let mut node = glass_layer(0, RenderEffect::blur(radius));
        let RenderNode::Layer(layer) = &mut node else {
            unreachable!();
        };
        layer.node_id = identified.then_some(100 + index);
        children.push(node);
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

#[test]
fn a_cached_backdrop_tracks_the_effect_of_the_glass_beneath_it() {
    let Ok(mut renderer) = support::headless_renderer() else {
        return;
    };
    for _ in 0..6 {
        capture(&mut renderer, stacked_cached_glasses(2.0, true));
    }
    let before = capture(&mut renderer, stacked_cached_glasses(2.0, true));
    assert!(renderer.last_frame_stats().expect("stats").layer_cache_hits > 0);
    let changed = capture(&mut renderer, stacked_cached_glasses(14.0, true));
    let reference = capture(&mut renderer, stacked_cached_glasses(14.0, false));
    let region = rect(
        GLASS_LEFT + 4.0,
        GLASS_TOP + 4.0,
        GLASS_WIDTH - 8.0,
        GLASS_HEIGHT - 8.0,
    );
    let expected = region_pixels(&reference, region);
    assert_ne!(region_pixels(&before, region), expected);
    let difference = image_difference_stats(
        &region_pixels(&changed, region),
        &expected,
        region.width as u32,
        region.height as u32,
        2,
    );
    assert_eq!(difference.differing_pixels, 0, "{difference:?}");
}

fn independent_cached_glasses(identified: bool) -> RenderGraph {
    let mut children = support::striped_page(1104, 720);
    for index in 0..9 {
        let mut layer = shared_test_support::layer_node(
            rect(0.0, 0.0, 300.0, 160.0),
            ProjectiveTransform::translation(
                32.0 + (index % 3) as f32 * 368.0,
                32.0 + (index / 3) as f32 * 240.0,
            ),
            GraphicsLayer {
                backdrop_effect: Some(RenderEffect::blur(12.0)),
                clip: true,
                shape: LayerShape::Rounded(RoundedCornerShape::uniform(12.0)),
                ..GraphicsLayer::default()
            },
            vec![inset_content(300.0, 160.0, 12.0)],
        );
        layer.node_id = identified.then_some(index + 1);
        children.push(RenderNode::Layer(Box::new(layer)));
    }
    support::page_graph(1104, 720, children)
}

#[test]
fn independent_glasses_are_admitted_over_several_frames_without_changing_pixels() {
    let Ok(mut renderer) = support::headless_renderer() else {
        return;
    };
    let mut render = |identified| {
        let frame = support::capture_graph(
            &mut renderer,
            independent_cached_glasses(identified),
            1104,
            720,
        );
        (renderer.last_frame_stats().expect("frame stats"), frame)
    };
    let (first, _) = render(true);
    assert_eq!(first.layer_cache_misses, 9);
    assert_eq!(first.backdrop_admissions, 0);
    let mut admitted = 0;
    let mut frames = 0;
    while admitted < first.layer_cache_misses {
        let (frame, _) = render(true);
        assert!(
            frame.backdrop_admissions > 0,
            "admissions stalled at frame {frames}"
        );
        assert!(
            frame.backdrop_admissions <= 3,
            "frame {frames} admitted {} glasses",
            frame.backdrop_admissions
        );
        admitted += frame.backdrop_admissions;
        frames += 1;
        assert!(frames <= first.layer_cache_misses);
    }
    assert!(frames >= 3);
    let (settled, settled_frame) = render(true);
    assert_eq!(settled.layer_cache_misses, 0);
    assert_eq!(settled.layer_cache_hits, first.layer_cache_misses);
    let (_, reference) = render(false);
    let max_channel_delta = support::max_channel_delta(&settled_frame.pixels, &reference.pixels);
    assert!(
        max_channel_delta <= 1,
        "cached/reference channel delta {max_channel_delta}"
    );
}
