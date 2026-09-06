mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_liquid::{Glass, GlassDynamics, LiquidColors, LiquidShape};
use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    Brush, Color, GraphicsLayer, LIQUID_GLASS_WGSL, Point, Rect, RenderEffect, RuntimeShader,
    SubstrateSpec, TileMode,
};
use support::{brush_rect, solid_rect};

const REFERENCE_WGSL: &str = include_str!("fixtures/liquid_glass_reference.wgsl");
const FRAME_WIDTH: u32 = 360;
const FRAME_HEIGHT: u32 = 240;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn resourced_shader(shader: &RuntimeShader, source: &str) -> RuntimeShader {
    let mut copy = RuntimeShader::new(source);
    for (index, value) in shader.uniforms().iter().enumerate() {
        copy.set_float(index, *value);
    }
    for (name, value) in shader.overrides() {
        copy.set_override(name, *value);
    }
    copy.set_input_padding(shader.input_padding());
    copy.set_output_padding(shader.output_padding());
    copy.set_output_support(shader.output_support());
    copy.set_sample_domain(shader.sample_domain());
    copy.set_substrates(shader.substrates().to_vec());
    copy.set_draw_split(shader.draw_split());
    copy.set_batched_source(shader.batched_source());
    copy
}

fn resourced(effect: &RenderEffect, source: &str) -> RenderEffect {
    match effect {
        RenderEffect::Shader { shader } => RenderEffect::Shader {
            shader: resourced_shader(shader, source),
        },
        RenderEffect::Chain { first, second } => RenderEffect::Chain {
            first: Box::new(resourced(first, source)),
            second: Box::new(resourced(second, source)),
        },
        other => other.clone(),
    }
}

fn card_glass(shape: LiquidShape) -> Glass {
    Glass::regular()
        .shape(shape)
        .blur_radius(0.0)
        .refraction_depth(0.58)
        .refraction_curve(0.62)
        .dispersion(1.0)
        .transmission_refraction(0.72)
        .highlight(0.72)
        .adaptive_frost(Color::from_rgb_u8(230, 230, 250), 0.42)
}

fn lens_glass(blur: f32) -> Glass {
    Glass::regular()
        .shape(LiquidShape::RoundedRect(12.0))
        .blur_radius(blur)
        .shadow(false)
        .no_clip()
}

fn glass_layer(
    node: Rect,
    effect: RenderEffect,
    alpha: f32,
    children: Vec<RenderNode>,
    source: &str,
) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, node.width, node.height),
        ProjectiveTransform::translation(node.x, node.y),
        GraphicsLayer {
            backdrop_effect: Some(resourced(&effect, source)),
            alpha,
            ..GraphicsLayer::default()
        },
        children,
    )))
}

fn backdrop() -> Vec<RenderNode> {
    let mut children = vec![brush_rect(
        rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
        Brush::radial_gradient_stops(
            vec![
                (0.0, Color::from_rgb_u8(90, 70, 140)),
                (0.6, Color::from_rgb_u8(30, 26, 60)),
                (1.0, Color::from_rgb_u8(8, 8, 20)),
            ],
            Point::new(FRAME_WIDTH as f32 * 0.4, FRAME_HEIGHT as f32 * 0.3),
            FRAME_WIDTH as f32 * 0.9,
            TileMode::Clamp,
        ),
    )];
    for i in 0..80u32 {
        let x = (i as f32 * 41.3) % FRAME_WIDTH as f32;
        let y = (i as f32 * 23.7) % FRAME_HEIGHT as f32;
        let size = 1.0 + (i % 4) as f32;
        children.push(solid_rect(
            rect(x, y, size, size),
            Color::from_rgba_u8(255, 240, 200, 150 + (i % 4) as u8 * 25),
        ));
    }
    children
}

fn cards(source: &str, alpha: f32, with_content: bool) -> RenderGraph {
    let colors = LiquidColors::dark(Color::from_rgb_u8(120, 140, 255));
    let mut children = backdrop();
    for (node, shape, density) in [
        (
            rect(24.0, 20.0, 200.0, 90.0),
            LiquidShape::RoundedRect(18.0),
            1.0,
        ),
        (rect(240.0, 20.0, 100.0, 100.0), LiquidShape::Capsule, 2.0),
        (
            rect(24.0, 130.0, 300.0, 90.0),
            LiquidShape::RoundedRect(4.0),
            1.5,
        ),
    ] {
        let effect = card_glass(shape).backdrop_effect(&colors, density, GlassDynamics::default());
        let content = if with_content {
            vec![solid_rect(
                rect(12.0, 12.0, node.width * 0.5, 16.0),
                Color::from_rgba_u8(255, 255, 255, 200),
            )]
        } else {
            Vec::new()
        };
        children.push(glass_layer(node, effect, alpha, content, source));
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn lenses(source: &str) -> RenderGraph {
    let colors = LiquidColors::dark(Color::from_rgb_u8(120, 140, 255));
    let mut children = backdrop();
    let small = rect(20.0, 20.0, 160.0, 90.0);
    children.push(glass_layer(
        small,
        lens_glass(12.0).backdrop_effect(
            &colors,
            1.0,
            support::morphing_lens_dynamics(small, (80.0, 45.0, 120.0, 40.0, -1.0)),
        ),
        1.0,
        Vec::new(),
        source,
    ));
    let large = rect(30.0, 30.0, 300.0, 200.0);
    children.push(glass_layer(
        large,
        lens_glass(24.0).backdrop_effect(
            &colors,
            2.0,
            support::morphing_lens_dynamics(large, (150.0, 100.0, 220.0, 120.0, 30.0)),
        ),
        0.9,
        Vec::new(),
        source,
    ));
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn variants(source: &str) -> RenderGraph {
    let colors = LiquidColors::dark(Color::from_rgb_u8(120, 140, 255));
    let mut children = backdrop();
    let lens = rect(24.0, 20.0, 200.0, 90.0);
    children.push(glass_layer(
        lens,
        Glass::lens()
            .shape(LiquidShape::RoundedRect(18.0))
            .blur_radius(0.0)
            .dispersion(1.0)
            .highlight(0.72)
            .backdrop_effect(&colors, 1.5, GlassDynamics::default()),
        1.0,
        Vec::new(),
        source,
    ));
    let resting = rect(24.0, 130.0, 300.0, 90.0);
    children.push(glass_layer(
        resting,
        card_glass(LiquidShape::RoundedRect(4.0)).backdrop_effect(
            &colors,
            1.5,
            GlassDynamics {
                activity: Some(0.5),
                resting_tint: Some(Color::from_rgba_u8(40, 40, 80, 120)),
                ..GlassDynamics::default()
            },
        ),
        1.0,
        Vec::new(),
        source,
    ));
    let shallow = rect(240.0, 20.0, 100.0, 90.0);
    children.push(glass_layer(
        shallow,
        card_glass(LiquidShape::RoundedRect(12.0))
            .refraction_depth(0.05)
            .backdrop_effect(
                &colors,
                1.5,
                GlassDynamics {
                    activity: Some(0.5),
                    resting_tint: Some(Color::from_rgba_u8(80, 40, 40, 120)),
                    ..GlassDynamics::default()
                },
            ),
        1.0,
        Vec::new(),
        source,
    ));
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn without_adaptive_block(source: &str) -> String {
    let start = source
        .find("    if adaptive_frost > 0.0 {\n")
        .expect("the adaptive frost block");
    let end = source[start..]
        .find("\n    }\n")
        .expect("the adaptive frost block's end")
        + start
        + "\n    }\n".len();
    format!("{}{}", &source[..start], &source[end..])
}

fn with_substrates(effect: RenderEffect, substrates: Vec<SubstrateSpec>) -> RenderEffect {
    match effect {
        RenderEffect::Shader { mut shader } => {
            shader.set_substrates(substrates);
            RenderEffect::Shader { shader }
        }
        other => other,
    }
}

fn frosted_card(
    activity: f32,
    source: &str,
    substrates: Option<Vec<SubstrateSpec>>,
) -> RenderGraph {
    let colors = LiquidColors::dark(Color::from_rgb_u8(120, 140, 255));
    let mut children = backdrop();
    let node = rect(24.0, 20.0, 300.0, 200.0);
    let effect = card_glass(LiquidShape::RoundedRect(18.0))
        .adaptive_frost(Color::from_rgb_u8(40, 34, 70), 0.42)
        .backdrop_effect(
            &colors,
            1.5,
            GlassDynamics {
                activity: Some(activity),
                resting_tint: Some(Color::from_rgba_u8(40, 40, 80, 120)),
                ..GlassDynamics::default()
            },
        );
    let effect = match substrates {
        Some(substrates) => with_substrates(effect, substrates),
        None => effect,
    };
    children.push(glass_layer(node, effect, 1.0, Vec::new(), source));
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn capture(
    renderer: &mut support::LockedRenderer,
    graph: RenderGraph,
    root_scale: f32,
) -> CapturedFrame {
    support::capture_graph_with_scale(renderer, graph, FRAME_WIDTH, FRAME_HEIGHT, root_scale)
}

fn assert_matches_reference(
    renderer: &mut support::LockedRenderer,
    label: &str,
    graph: impl Fn(&str) -> RenderGraph,
    root_scale: f32,
) {
    let frozen = capture(renderer, graph(REFERENCE_WGSL), root_scale);
    let current = capture(renderer, graph(LIQUID_GLASS_WGSL), root_scale);
    let differing = support::differing_pixels(FRAME_WIDTH, &frozen.pixels, &current.pixels);
    assert!(
        differing.is_empty(),
        "{label}: the shipped glass shader renders differently from tests/fixtures/liquid_glass_reference.wgsl; a deliberate picture change re-freezes that file in the same commit: {}",
        support::describe_differing(&differing)
    );
    assert!(
        support::distinct_colors(&frozen.pixels) > 64,
        "{label}: the reference render is too flat to prove anything"
    );
}

#[test]
fn cards_on_the_page_path_match_the_reference_shader() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    assert_matches_reference(&mut renderer, "page cards", |s| cards(s, 1.0, false), 1.0);
}

#[test]
fn cards_on_the_child_path_with_content_match_the_reference_shader() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    assert_matches_reference(&mut renderer, "child cards", |s| cards(s, 0.9, true), 1.0);
    assert_matches_reference(
        &mut renderer,
        "page cards with content",
        |s| cards(s, 1.0, true),
        1.0,
    );
}

#[test]
fn cards_at_a_fractional_scale_match_the_reference_shader() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    assert_matches_reference(
        &mut renderer,
        "cards at 1.5x",
        |s| cards(s, 1.0, false),
        1.5,
    );
    assert_matches_reference(
        &mut renderer,
        "child cards at 0.75x",
        |s| cards(s, 0.9, true),
        0.75,
    );
}

#[test]
fn lenses_with_blur_and_substrates_match_the_reference_shader() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    assert_matches_reference(&mut renderer, "lenses", lenses, 1.0);
    assert_matches_reference(&mut renderer, "lenses at 1.5x", lenses, 1.5);
}

#[test]
fn a_lens_variant_and_a_resting_card_match_the_reference_shader() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    for scale in [1.0, 1.5] {
        assert_matches_reference(
            &mut renderer,
            "lens variant and resting card",
            variants,
            scale,
        );
    }
}

fn assert_rest_agrees_and_activity_differs(
    renderer: &mut support::LockedRenderer,
    claim: &str,
    rest: (RenderGraph, RenderGraph),
    active: (RenderGraph, RenderGraph),
) {
    let resting = capture(renderer, rest.0, 1.0);
    let other = capture(renderer, rest.1, 1.0);
    let differing = support::differing_pixels(FRAME_WIDTH, &resting.pixels, &other.pixels);
    assert!(
        differing.is_empty(),
        "{claim}: {}",
        support::describe_differing(&differing)
    );
    assert!(
        support::distinct_colors(&resting.pixels) > 64,
        "the resting render is too flat to prove anything"
    );
    let active_a = capture(renderer, active.0, 1.0);
    let active_b = capture(renderer, active.1, 1.0);
    assert!(
        !support::differing_pixels(FRAME_WIDTH, &active_a.pixels, &active_b.pixels).is_empty(),
        "an active glass runs the block and reads its substrate, so the same comparison must \
         see it"
    );
}

#[test]
fn a_resting_frosted_glass_never_runs_its_adaptive_block() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let cut = without_adaptive_block(LIQUID_GLASS_WGSL);
    assert!(
        cut.len() < LIQUID_GLASS_WGSL.len()
            && LIQUID_GLASS_WGSL.contains("let adaptive_sample = sample_adaptive_neighborhood(")
            && !cut.contains("let adaptive_sample = sample_adaptive_neighborhood("),
        "the cut source must have lost the adaptive read and nothing else"
    );
    assert_rest_agrees_and_activity_differs(
        &mut renderer,
        "a resting glass returns before its adaptive block, so the shipped shader and the same \
         source without that block must agree at the same capture geometry",
        (
            frosted_card(0.0, LIQUID_GLASS_WGSL, None),
            frosted_card(0.0, &cut, None),
        ),
        (
            frosted_card(0.5, LIQUID_GLASS_WGSL, None),
            frosted_card(0.5, &cut, None),
        ),
    );
}

#[test]
fn a_resting_frosted_glass_reads_no_substrate() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let blur = vec![SubstrateSpec::Blur { radius_px: 24.0 }];
    assert_rest_agrees_and_activity_differs(
        &mut renderer,
        "a resting glass as specialized must render byte for byte like one with its blur \
         substrate declared: the declaration sets the capture geometry, and on Adreno a \
         geometry change moves pixels even where the substrate is never read",
        (
            frosted_card(0.0, LIQUID_GLASS_WGSL, None),
            frosted_card(0.0, LIQUID_GLASS_WGSL, Some(blur)),
        ),
        (
            frosted_card(0.5, LIQUID_GLASS_WGSL, None),
            frosted_card(0.5, LIQUID_GLASS_WGSL, Some(Vec::new())),
        ),
    );
}
