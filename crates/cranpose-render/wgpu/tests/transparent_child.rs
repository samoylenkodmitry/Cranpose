use cranpose_render_common::graph::{CachePolicy, ProjectiveTransform, RenderNode};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    BlendMode, CompositingStrategy, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect,
    RuntimeShader,
};

use crate::{
    shared_test_support, support,
    support::bar_scene::{BAR, HEIGHT, WIDTH},
};

/// A shader returning its source as it is, so zero over a transparent
/// layer; declared so or not.
fn identity_shader(declared: bool) -> RenderEffect {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n@fragment\nfn effect_fs(input: VertexOutput) -> \
         @location(0) vec4<f32> {{\n    return textureSample(input_texture, input_sampler, \
         input.uv);\n}}\n"
    ));
    shader.set_position_independent(true);
    shader.set_preserves_transparency(declared);
    RenderEffect::runtime_shader(shader)
}

/// A shader painting red whatever its source holds.
fn painting_shader() -> RenderEffect {
    painting_shader_at_alpha(1.0)
}

fn painting_shader_at_alpha(alpha: f32) -> RenderEffect {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n@fragment\nfn effect_fs(input: VertexOutput) -> \
         @location(0) vec4<f32> {{\n    return vec4<f32>({alpha}, 0.0, 0.0, {alpha});\n}}\n"
    ));
    shader.set_position_independent(true);
    RenderEffect::runtime_shader(shader)
}

/// The striped page with an empty layer over `BAR` carrying `effect`.
fn page_with_empty_layer(effect: RenderEffect) -> Vec<RenderNode> {
    page_with_layer(
        GraphicsLayer {
            render_effect: Some(effect),
            ..Default::default()
        },
        vec![],
    )
}

fn page_with_backdrop_layer(alpha: f32, effect: RenderEffect) -> Vec<RenderNode> {
    page_with_layer(
        GraphicsLayer {
            alpha,
            backdrop_effect: Some(effect),
            ..Default::default()
        },
        vec![],
    )
}

fn layer(rect: Rect, graphics_layer: GraphicsLayer, children: Vec<RenderNode>) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect,
        ProjectiveTransform::identity(),
        graphics_layer,
        children,
    )))
}

fn page_with_nodes(nodes: impl IntoIterator<Item = RenderNode>) -> Vec<RenderNode> {
    let mut page = support::striped_page(WIDTH, HEIGHT);
    page.extend(nodes);
    page
}

fn page_with_layer(graphics_layer: GraphicsLayer, children: Vec<RenderNode>) -> Vec<RenderNode> {
    page_with_nodes([layer(BAR, graphics_layer, children)])
}

fn translucent_foreground() -> RenderNode {
    support::solid_rect(
        BAR,
        cranpose_ui_graphics::Color::from_rgba_u8(0, 0, 255, 128),
    )
}

fn page_with_backdrop_and_foreground(
    alpha: f32,
    effect: RenderEffect,
    compositing_strategy: CompositingStrategy,
) -> Vec<RenderNode> {
    page_with_layer(
        GraphicsLayer {
            alpha,
            compositing_strategy,
            backdrop_effect: Some(effect),
            ..Default::default()
        },
        vec![translucent_foreground()],
    )
}

fn page_with_grouped_backdrop(
    alpha: f32,
    effect: RenderEffect,
    foreground: Option<RenderNode>,
) -> Vec<RenderNode> {
    let mut children = vec![backdrop_node(BAR, effect)];
    children.extend(foreground);
    page_with_layer(
        GraphicsLayer {
            alpha,
            render_effect: Some(identity_shader(true)),
            ..Default::default()
        },
        children,
    )
}

fn page_with_modulated_backdrop_and_foreground(alpha: f32) -> Vec<RenderNode> {
    page_with_nodes([
        backdrop_node(BAR, painting_shader_at_alpha(alpha)),
        layer(
            BAR,
            GraphicsLayer {
                alpha,
                compositing_strategy: CompositingStrategy::ModulateAlpha,
                ..Default::default()
            },
            vec![translucent_foreground()],
        ),
    ])
}

fn backdrop_node(rect: Rect, effect: RenderEffect) -> RenderNode {
    layer(
        rect,
        GraphicsLayer {
            backdrop_effect: Some(effect),
            ..Default::default()
        },
        vec![],
    )
}

fn capture(renderer: &mut support::LockedRenderer, page: Vec<RenderNode>) -> CapturedFrame {
    support::capture_graph_settled(
        renderer,
        support::page_graph(WIDTH, HEIGHT, page),
        WIDTH,
        HEIGHT,
    )
}

#[test]
fn changing_a_cloned_layers_opacity_preserves_the_original_picture() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    for explicit in [false, true] {
        let mut layer = support::layer_node(
            None,
            WIDTH as f32,
            HEIGHT as f32,
            vec![support::solid_rect(BAR, cranpose_ui_graphics::Color::RED)],
        );
        if explicit {
            layer.graphics_layer = GraphicsLayer::default().into();
        }
        let original = vec![RenderNode::Layer(Box::new(layer))];
        let expected = capture(&mut renderer, original.clone());
        let mut hidden = original.clone();
        let Some(RenderNode::Layer(layer)) = hidden.first_mut() else {
            panic!("visible layer");
        };
        layer.graphics_layer.alpha = 0.0;
        let hidden = capture(&mut renderer, hidden);
        assert!(
            hidden.pixels != expected.pixels,
            "opacity must change the picture"
        );
        let unchanged = capture(&mut renderer, original);
        assert!(
            unchanged.pixels == expected.pixels,
            "changing a clone changed the original picture"
        );
    }
}

/// An empty layer under a shader that keeps a transparent source
/// transparent composites nothing, so the renderer draws nothing for it;
/// the same shader undeclared shades the layer's pixels to the same page,
/// and a shader that paints from nothing is drawn as declared.
#[test]
fn an_empty_layer_under_a_transparency_preserving_shader_draws_nothing() {
    let mut renderer = support::headless_renderer().expect("GPU required for the layer probe");
    let plain = capture(&mut renderer, support::striped_page(WIDTH, HEIGHT));
    let plain_passes = renderer
        .last_frame_stats()
        .expect("frame statistics")
        .pass_count;

    let undeclared = capture(&mut renderer, page_with_empty_layer(identity_shader(false)));
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        stats.shader_pixels > 0,
        "an undeclared shader over an empty layer is drawn"
    );
    let differing = support::differing_pixels(WIDTH, &plain.pixels, &undeclared.pixels);
    assert!(
        differing.is_empty(),
        "the identity shader over nothing changed the page: {}",
        support::describe_differing(&differing)
    );

    let declared = capture(&mut renderer, page_with_empty_layer(identity_shader(true)));
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert_eq!(stats.shader_pixels, 0, "the declared shader is not drawn");
    assert_eq!(
        stats.pass_count, plain_passes,
        "the empty layer costs the page no pass"
    );
    let differing = support::differing_pixels(WIDTH, &plain.pixels, &declared.pixels);
    assert!(
        differing.is_empty(),
        "skipping the empty layer changed the page: {}",
        support::describe_differing(&differing)
    );

    let painted = capture(&mut renderer, page_with_empty_layer(painting_shader()));
    let differing = support::differing_pixels(WIDTH, &plain.pixels, &painted.pixels);
    assert_eq!(
        differing.len(),
        (BAR.width * BAR.height) as usize,
        "a shader painting from nothing paints the whole layer"
    );
}

#[test]
fn zero_opacity_skips_source_over_effects_but_keeps_destructive_blending() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let plain = capture(&mut renderer, support::striped_page(WIDTH, HEIGHT));
    let plain_passes = renderer
        .last_frame_stats()
        .expect("frame statistics")
        .pass_count;
    for blend_mode in [BlendMode::SrcOver, BlendMode::Src] {
        let mut page = page_with_empty_layer(painting_shader());
        let Some(RenderNode::Layer(layer)) = page.last_mut() else {
            panic!("effect layer");
        };
        layer.graphics_layer.alpha = 0.0;
        layer.graphics_layer.blend_mode = blend_mode;
        layer.children.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                BAR,
                ProjectiveTransform::identity(),
                GraphicsLayer {
                    backdrop_effect: Some(RenderEffect::blur(8.0)),
                    ..Default::default()
                },
                vec![],
            ),
        )));
        let frame = capture(&mut renderer, page);
        let stats = renderer.last_frame_stats().expect("frame statistics");
        if blend_mode == BlendMode::SrcOver {
            assert!(
                frame.pixels == plain.pixels,
                "hidden source-over changed pixels"
            );
            assert_eq!(
                stats.pass_count, plain_passes,
                "hidden effects added passes"
            );
            assert_eq!(stats.shader_pixels, 0, "hidden shader still ran");
            assert_eq!(stats.blur_passes, 0, "hidden descendant backdrop still ran");
        } else {
            assert!(
                frame.pixels != plain.pixels,
                "transparent Src must replace destination pixels"
            );
        }
    }
}

#[test]
fn zero_opacity_skips_a_backdrop_on_the_same_layer() {
    for effect in [painting_shader(), RenderEffect::blur(8.0)] {
        let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
        let plain = capture(&mut renderer, support::striped_page(WIDTH, HEIGHT));
        let plain_stats = renderer.last_frame_stats().expect("plain frame statistics");
        let hidden = capture(&mut renderer, page_with_backdrop_layer(0.0, effect));
        let stats = renderer
            .last_frame_stats()
            .expect("hidden frame statistics");
        let differing = support::differing_pixels(WIDTH, &plain.pixels, &hidden.pixels);

        assert!(
            differing.is_empty(),
            "an alpha-zero layer's own backdrop changed the page: {}",
            support::describe_differing(&differing)
        );
        assert_eq!(
            stats.pass_count, plain_stats.pass_count,
            "hidden backdrop added passes"
        );
        assert_eq!(stats.stages, 0, "hidden backdrop entered a stage");
        assert_eq!(stats.shader_pixels, 0, "hidden backdrop ran a shader");
        assert_eq!(stats.blur_passes, 0, "hidden backdrop ran a blur");
    }

    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let mut same_layer = page_with_backdrop_layer(0.0, painting_shader());
    let Some(RenderNode::Layer(layer)) = same_layer.last_mut() else {
        panic!("backdrop layer");
    };
    layer.graphics_layer.blend_mode = BlendMode::Src;
    let destructive = capture(&mut renderer, same_layer);

    let mut reference = page_with_empty_layer(painting_shader());
    let Some(RenderNode::Layer(layer)) = reference.last_mut() else {
        panic!("effect layer");
    };
    layer.graphics_layer.alpha = 0.0;
    layer.graphics_layer.blend_mode = BlendMode::Src;
    let expected = capture(&mut renderer, reference);
    let differing = support::differing_pixels(WIDTH, &destructive.pixels, &expected.pixels);
    assert!(
        differing.is_empty(),
        "zero-opacity Src backdrop must preserve destructive transparent-source blending: {}",
        support::describe_differing(&differing)
    );
}

#[test]
fn same_layer_backdrop_obeys_group_opacity() {
    for alpha in [0.0, 0.5, 1.0] {
        let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
        let same_layer = capture(
            &mut renderer,
            page_with_backdrop_layer(alpha, painting_shader()),
        );
        let grouped = capture(
            &mut renderer,
            page_with_grouped_backdrop(alpha, painting_shader(), None),
        );
        let differing = support::differing_pixels(WIDTH, &same_layer.pixels, &grouped.pixels);

        assert!(
            differing.is_empty(),
            "same-layer backdrop opacity {alpha} differed from grouped opacity: {}",
            support::describe_differing(&differing)
        );
    }
}

#[test]
fn same_layer_backdrop_and_translucent_foreground_share_group_opacity() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let same_layer = capture(
        &mut renderer,
        page_with_backdrop_and_foreground(0.5, painting_shader(), CompositingStrategy::Auto),
    );
    let grouped = capture(
        &mut renderer,
        page_with_grouped_backdrop(0.5, painting_shader(), Some(translucent_foreground())),
    );
    let differing = support::differing_pixels(WIDTH, &same_layer.pixels, &grouped.pixels);
    assert!(
        differing.is_empty(),
        "backdrop and overlapping translucent foreground must share group opacity: {}",
        support::describe_differing(&differing)
    );
}

#[test]
fn same_layer_modulate_alpha_applies_to_backdrop_and_foreground_independently() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let same_layer = capture(
        &mut renderer,
        page_with_backdrop_and_foreground(
            0.5,
            painting_shader(),
            CompositingStrategy::ModulateAlpha,
        ),
    );
    let siblings = capture(
        &mut renderer,
        page_with_modulated_backdrop_and_foreground(0.5),
    );
    let differing = support::differing_pixels(WIDTH, &same_layer.pixels, &siblings.pixels);
    assert!(
        differing.is_empty(),
        "ModulateAlpha must apply to each same-layer source as it does to siblings: {}",
        support::describe_differing(&differing)
    );
}

#[test]
fn cached_backdrop_uses_the_current_modulated_opacity() {
    let glass = cranpose_ui_graphics::liquid_glass_effect(
        &cranpose_ui_graphics::LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: BAR.width,
            height: BAR.height,
            tint_color: cranpose_ui_graphics::Color::WHITE.with_alpha(0.2),
        },
        &cranpose_ui_graphics::LiquidGlassSpec::default(),
        BAR.width,
        BAR.height,
    );
    for effect in [RenderEffect::blur(8.0), glass] {
        let page = |alpha| {
            let mut page = page_with_backdrop_layer(alpha, effect.clone());
            let Some(RenderNode::Layer(layer)) = page.last_mut() else {
                panic!("backdrop layer");
            };
            layer.node_id = Some(83);
            layer.cache_policy = CachePolicy::Auto;
            layer.graphics_layer.compositing_strategy = CompositingStrategy::ModulateAlpha;
            layer.recompute_raster_cache_hashes();
            page
        };
        let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
        capture(&mut renderer, page(0.25));
        let warm = capture(&mut renderer, page(0.25));
        assert!(
            renderer
                .last_frame_stats()
                .expect("warm stats")
                .layer_cache_hits
                > 0
        );
        let changed = capture(&mut renderer, page(0.75));
        assert!(
            renderer
                .last_frame_stats()
                .expect("changed stats")
                .layer_cache_hits
                > 0
        );
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", Some("1"));
        let uncached = capture(&mut renderer, page(0.75));
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", None);
        assert!(warm.pixels != changed.pixels, "opacity changes the picture");
        support::assert_same_bytes(
            "cached backdrop after opacity changes",
            WIDTH,
            &changed.pixels,
            &uncached.pixels,
        );
    }
}

#[test]
fn zero_opacity_nested_backdrop_does_not_split_independent_backdrop_stages() {
    let left = Rect {
        x: 8.0,
        y: 12.0,
        width: 30.0,
        height: 24.0,
    };
    let middle = Rect {
        x: 65.0,
        y: 34.0,
        width: 30.0,
        height: 24.0,
    };
    let right = Rect {
        x: 122.0,
        y: 58.0,
        width: 30.0,
        height: 24.0,
    };
    let page_with_siblings = |hidden_middle: bool| {
        let mut page = support::striped_page(WIDTH, HEIGHT);
        page.push(backdrop_node(left, painting_shader()));
        if hidden_middle {
            page.push(RenderNode::Layer(Box::new(
                shared_test_support::layer_node(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: WIDTH as f32,
                        height: HEIGHT as f32,
                    },
                    ProjectiveTransform::identity(),
                    GraphicsLayer {
                        alpha: 0.0,
                        ..Default::default()
                    },
                    vec![backdrop_node(middle, painting_shader())],
                ),
            )));
        }
        page.push(backdrop_node(right, painting_shader()));
        page
    };

    let mut baseline_renderer =
        support::headless_renderer().expect("GPU required for batching probe");
    let baseline = capture(&mut baseline_renderer, page_with_siblings(false));
    let baseline_stats = baseline_renderer
        .last_frame_stats()
        .expect("baseline statistics");
    drop(baseline_renderer);
    assert_eq!(
        baseline_stats.stages, 1,
        "independent backdrops share one stage"
    );

    let mut hidden_renderer =
        support::headless_renderer().expect("GPU required for batching probe");
    let hidden = capture(&mut hidden_renderer, page_with_siblings(true));
    let hidden_stats = hidden_renderer
        .last_frame_stats()
        .expect("hidden statistics");
    let differing = support::differing_pixels(WIDTH, &baseline.pixels, &hidden.pixels);
    assert!(
        differing.is_empty(),
        "the hidden group changed independent backdrop output: {}",
        support::describe_differing(&differing)
    );
    assert_eq!(
        hidden_stats.stages, baseline_stats.stages,
        "the hidden backdrop split independent siblings into extra stages"
    );
}

#[test]
fn a_descendant_backdrop_shares_its_parents_group_opacity() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let plain = capture(&mut renderer, support::striped_page(WIDTH, HEIGHT));
    let mut frames = Vec::new();
    for alpha in [0.0, 0.5, 1.0] {
        let mut page = support::striped_page(WIDTH, HEIGHT);
        page.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                BAR,
                ProjectiveTransform::identity(),
                GraphicsLayer {
                    alpha,
                    render_effect: Some(identity_shader(true)),
                    ..Default::default()
                },
                vec![RenderNode::Layer(Box::new(
                    shared_test_support::layer_node(
                        BAR,
                        ProjectiveTransform::identity(),
                        GraphicsLayer {
                            backdrop_effect: Some(painting_shader()),
                            ..Default::default()
                        },
                        vec![],
                    ),
                ))],
            ),
        )));
        frames.push(capture(&mut renderer, page));
    }
    assert_eq!(
        frames[0].pixels, plain.pixels,
        "hidden backdrop changed page"
    );
    let alpha = GraphicsLayer::composite_alpha_8bit(0.5);
    let mut changed = 0;
    for ((base, middle), opaque) in plain
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(frames[1].pixels.as_chunks::<4>().0)
        .zip(frames[2].pixels.as_chunks::<4>().0)
    {
        if base != opaque {
            changed += 1;
        }
        for channel in 0..4 {
            let expected =
                f32::from(base[channel]) * (1.0 - alpha) + f32::from(opaque[channel]) * alpha;
            assert!(
                (f32::from(middle[channel]) - expected).abs() <= 2.0,
                "group opacity differs: {base:?} {middle:?} {opaque:?}"
            );
        }
    }
    assert!(changed > 0, "opaque backdrop must visibly paint the page");
}

#[test]
fn zero_modulated_primitive_alpha_keeps_a_shader_that_paints_from_nothing() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let expected = capture(&mut renderer, page_with_empty_layer(painting_shader()));
    let mut page = page_with_empty_layer(painting_shader());
    let Some(RenderNode::Layer(layer)) = page.last_mut() else {
        panic!("effect layer");
    };
    layer.graphics_layer.alpha = 0.0;
    layer.graphics_layer.compositing_strategy = CompositingStrategy::ModulateAlpha;
    let actual = capture(&mut renderer, page);
    assert_eq!(actual.pixels, expected.pixels);
    assert!(
        renderer
            .last_frame_stats()
            .expect("frame statistics")
            .shader_pixels
            > 0
    );
}
