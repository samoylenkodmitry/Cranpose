use cranpose_core::NodeId;
use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawPrimitiveNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderGraph, RenderNode, TextPrimitiveNode,
    },
};
use cranpose_ui::{TextLayoutOptions, TextStyle, text::SpanStyle};
use cranpose_ui_graphics::{Brush, Color, Rect, RenderEffect};

use crate::support;

fn card_layer(node_id: NodeId, y: f32) -> LayerNode {
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 96.0,
        height: 28.0,
    };
    let mut layer = support::contract_layer(
        Some(node_id),
        CachePolicy::Auto,
        local_bounds,
        ProjectiveTransform::translation(12.0, y),
        vec![painted_rect(local_bounds, Color(0.15, 0.35, 0.85, 1.0))],
    );
    layer.graphics_layer.alpha = 0.85;
    layer
}

fn painted_rect(rect: Rect, color: Color) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
            primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                rect,
                brush: Brush::solid(color),
                stroke: None,
            },
            clip: None,
        })),
    })
}

fn live_backdrop_graph(color: Color, policy: CachePolicy, nested: bool) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 128.0,
        height: 160.0,
    };
    let mut card = card_layer(80_001, 20.0);
    card.cache_policy = policy;
    card.graphics_layer.render_effect = Some(RenderEffect::blur(1.0));
    if nested {
        let mut glass = support::contract_layer(
            Some(80_002),
            CachePolicy::None,
            card.local_bounds,
            ProjectiveTransform::identity(),
            Vec::new(),
        );
        glass.graphics_layer.backdrop_effect = Some(RenderEffect::blur(6.0));
        card.children.push(RenderNode::Layer(Box::new(glass)));
    } else {
        card.graphics_layer.backdrop_effect = Some(RenderEffect::blur(6.0));
    }
    RenderGraph::new(support::contract_layer(
        Some(80_000),
        CachePolicy::None,
        bounds,
        ProjectiveTransform::identity(),
        vec![
            painted_rect(bounds, color),
            RenderNode::Layer(Box::new(card)),
        ],
    ))
}

fn verify_live_backdrop_content_cache(nested: bool) {
    let Ok(mut renderer) = support::headless_renderer() else {
        return;
    };
    let mut fresh = support::headless_renderer_beside_locked().expect("reference renderer");
    for _ in 0..6 {
        renderer.scene_mut().graph =
            Some(live_backdrop_graph(Color::BLACK, CachePolicy::Auto, nested));
        renderer.capture_frame(128, 160).expect("warm glass");
    }
    support::wait_for_background_compiler_idle();
    for color in [Color::WHITE, Color(0.2, 0.7, 0.4, 1.0), Color::BLACK] {
        renderer.scene_mut().graph = Some(live_backdrop_graph(color, CachePolicy::Auto, nested));
        let frame = renderer.capture_frame(128, 160).expect("changed backdrop");
        let stats = renderer.last_frame_stats().expect("frame stats");
        if nested {
            assert!(
                stats.isolated_layer_renders > 0,
                "nested glass must read the changing page"
            );
        } else {
            assert!(
                stats.layer_cache_hits > 0,
                "unchanged foreground must be retained"
            );
        }
        fresh.scene_mut().graph = Some(live_backdrop_graph(color, CachePolicy::None, nested));
        let reference = fresh.capture_frame(128, 160).expect("uncached reference");
        if !nested {
            let fresh_stats = fresh.last_frame_stats().expect("uncached frame stats");
            assert!(
                stats.isolated_layer_renders < fresh_stats.isolated_layer_renders,
                "retaining the foreground must save its redraw: {stats:?} vs {fresh_stats:?}"
            );
        }
        support::assert_same_bytes("live backdrop", 128, &reference.pixels, &frame.pixels);
    }
}

#[test]
fn a_live_backdrop_reuses_its_unchanged_foreground_without_freezing_the_page() {
    verify_live_backdrop_content_cache(false);
}

#[test]
fn a_nested_backdrop_keeps_its_container_content_live() {
    verify_live_backdrop_content_cache(true);
}

fn large_backdrops(radius: f32) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 520.0,
        height: 700.0,
    };
    let mut children = vec![painted_rect(bounds, Color::WHITE)];
    for (index, blur) in [radius, 6.0].into_iter().enumerate() {
        let pane = Rect {
            x: 10.0,
            y: 10.0 + index as f32 * 350.0,
            width: 500.0,
            height: 300.0,
        };
        children.push(painted_rect(
            Rect {
                width: 250.0,
                ..pane
            },
            Color::BLACK,
        ));
        let mut glass = support::contract_layer(
            Some(90_001 + index),
            CachePolicy::Auto,
            pane,
            ProjectiveTransform::identity(),
            Vec::new(),
        );
        glass.graphics_layer.backdrop_effect = Some(RenderEffect::blur(blur));
        let mut layer = glass;
        if index == 0 {
            layer = support::contract_layer(
                Some(90_010),
                CachePolicy::None,
                pane,
                ProjectiveTransform::identity(),
                vec![RenderNode::Layer(Box::new(layer))],
            );
            layer.graphics_layer.alpha = 0.9;
        }
        children.push(RenderNode::Layer(Box::new(layer)));
    }
    RenderGraph::new(support::contract_layer(
        Some(90_000),
        CachePolicy::None,
        bounds,
        ProjectiveTransform::identity(),
        children,
    ))
}

#[test]
fn a_changing_large_backdrop_does_not_starve_the_still_backdrop_after_it() {
    let Ok(mut renderer) = support::headless_renderer() else {
        return;
    };
    let mut fresh = support::headless_renderer_beside_locked().expect("reference renderer");
    for step in 0..8 {
        let radius = 4.0 + step as f32;
        renderer.scene_mut().graph = Some(large_backdrops(radius));
        let frame = renderer.capture_frame(520, 700).expect("glass frame");
        let stats = renderer.last_frame_stats().expect("frame stats");
        if step >= 2 {
            assert_eq!(
                stats.blur_passes, 1,
                "only the changing first pane may blur: {stats:?}"
            );
        }
        fresh.scene_mut().graph = Some(large_backdrops(radius));
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", Some("1"));
        let reference = fresh.capture_frame(520, 700).expect("uncached frame");
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", None);
        support::assert_same_bytes("independent glass", 520, &reference.pixels, &frame.pixels);
    }
}

fn scrolling_fill_graph(y: f32, mode: usize, phase: usize) -> RenderGraph {
    use cranpose_render_common::{
        graph::{DrawCommandId, DrawRunNode},
        style_shared::DrawPlacement,
    };
    use cranpose_ui_graphics::DrawPrimitive;
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 240.0,
    };
    let color = if phase == 0 {
        Color::WHITE
    } else {
        Color(0.2, 0.7, 0.4, 1.0)
    };
    let brush = match mode {
        1 => Brush::linear_gradient(vec![color, Color::BLACK]),
        2 => Brush::solid(color.with_alpha(0.6)),
        _ => Brush::solid(color),
    };
    let run = |node_id, primitives| {
        RenderNode::DrawRun(DrawRunNode::for_command(
            PrimitivePhase::BeforeChildren,
            Some(DrawCommandId {
                node_id,
                command_index: 0,
                placement: DrawPlacement::Behind,
            }),
            primitives,
        ))
    };
    let mut children = vec![run(
        91_003,
        vec![DrawPrimitive::Rect {
            rect: bounds,
            brush: Brush::solid(Color(0.1, 0.1, 0.2, 1.0)),
            stroke: None,
        }],
    )];
    let mut primitives = vec![DrawPrimitive::Rect {
        rect: Rect {
            x: 30.25,
            width: 140.75,
            ..bounds
        },
        brush,
        stroke: None,
    }];
    if mode == 3 {
        primitives.push(DrawPrimitive::Rect {
            rect: Rect {
                x: 50.0,
                y: 85.0 + phase as f32,
                width: 40.0,
                height: 20.0,
            },
            brush: Brush::solid(Color::BLACK),
            stroke: None,
        });
    }
    let mut fill = support::contract_layer(
        Some(91_002),
        CachePolicy::None,
        Rect {
            x: 35.5,
            y: if phase == 2 { 80.25 } else { 0.0 },
            width: 130.25,
            height: 240.0,
        },
        ProjectiveTransform::identity(),
        vec![run(91_004, primitives)],
    );
    fill.clip_to_bounds = true;
    children.push(RenderNode::Layer(Box::new(fill)));
    let mut pane = support::contract_layer(
        Some(91_001),
        CachePolicy::Auto,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 70.0,
        },
        ProjectiveTransform::translation(10.0, y),
        Vec::new(),
    );
    pane.graphics_layer.backdrop_effect = Some(RenderEffect::blur(6.0));
    children.push(RenderNode::Layer(Box::new(pane)));
    RenderGraph::new(support::contract_layer(
        Some(91_000),
        CachePolicy::None,
        bounds,
        ProjectiveTransform::identity(),
        children,
    ))
}
#[test]
fn moving_glass_reuses_only_unchanged_clipped_solid_pixels() {
    let mut renderer = support::headless_renderer().expect("renderer");
    let mut fresh = support::headless_renderer_beside_locked().expect("reference renderer");
    for scale in [1.0, 2.75] {
        for mode in 0..4 {
            for step in 0..12 {
                let y = 60.0 + step as f32 / scale;
                let phase = step / 4;
                renderer.scene_mut().graph = Some(scrolling_fill_graph(y, mode, phase));
                let width = (200.0 * scale) as u32;
                let height = (240.0 * scale) as u32;
                let actual = renderer
                    .capture_frame_with_scale(width, height, scale)
                    .expect("cached frame");
                if mode == 0 && phase < 2 && step % 4 >= 2 {
                    assert_eq!(
                        renderer.last_frame_stats().expect("stats").blur_passes,
                        0,
                        "scale={scale} step={step}"
                    );
                }
                fresh.scene_mut().graph = Some(scrolling_fill_graph(y, mode, phase));
                cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", Some("1"));
                let expected = fresh
                    .capture_frame_with_scale(width, height, scale)
                    .expect("uncached frame");
                cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", None);
                support::assert_same_bytes(
                    &format!("scale={scale} mode={mode} step={step}"),
                    width,
                    &expected.pixels,
                    &actual.pixels,
                );
            }
        }
    }
}

fn scroll_like_graph(offsets: &[f32]) -> RenderGraph {
    let children = offsets
        .iter()
        .enumerate()
        .map(|(index, y)| RenderNode::Layer(Box::new(card_layer(index + 1, *y))))
        .collect();
    RenderGraph::new(support::contract_layer(
        Some(10_000),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 128.0,
            height: 160.0,
        },
        ProjectiveTransform::identity(),
        children,
    ))
}

fn text_scroll_like_graph(y: f32) -> RenderGraph {
    RenderGraph::new(support::contract_layer(
        Some(20_000),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 140.0,
        },
        ProjectiveTransform::identity(),
        vec![RenderNode::Layer(Box::new(text_layer(
            77,
            16.0,
            y,
            "Markdown text row cache reuse",
        )))],
    ))
}

fn repeated_text_graph() -> RenderGraph {
    RenderGraph::new(support::contract_layer(
        Some(20_001),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 360.0,
            height: 160.0,
        },
        ProjectiveTransform::identity(),
        vec![
            RenderNode::Layer(Box::new(text_layer(
                77,
                16.0,
                20.0,
                "Repeated markdown heading",
            ))),
            RenderNode::Layer(Box::new(text_layer(
                78,
                16.0,
                64.0,
                "Repeated markdown heading",
            ))),
        ],
    ))
}

fn text_layer(node_id: NodeId, x: f32, y: f32, text_value: &str) -> LayerNode {
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 260.0,
        height: 36.0,
    };
    let text = PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
            node_id,
            rect: local_bounds,
            text: cranpose_ui::text::shared_plain_annotated_string(text_value),
            render_text: cranpose_ui::text::shared_plain_render_string(text_value),
            text_style: std::sync::Arc::new(TextStyle::from_span_style(SpanStyle {
                color: Some(Color(0.88, 0.90, 0.96, 1.0)),
                ..Default::default()
            })),
            font_size: 14.0,
            layout_options: TextLayoutOptions::default(),
            clip: None,
        })),
    };
    support::contract_layer(
        Some(node_id),
        CachePolicy::None,
        local_bounds,
        ProjectiveTransform::translation(x, y),
        vec![RenderNode::Primitive(text)],
    )
}

#[test]
fn capture_frame_reuses_cached_child_layers_during_rigid_scroll() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping rigid scroll cache reuse assertion because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(scroll_like_graph(&[8.0, 44.0, 80.0, 116.0]));
    renderer
        .capture_frame(160, 180)
        .expect("first capture should succeed");
    let first_stats = renderer.last_frame_stats().expect("first frame stats");

    renderer.scene_mut().graph = Some(scroll_like_graph(&[15.25, 51.25, 87.25, 123.25]));
    renderer
        .capture_frame(160, 180)
        .expect("second capture should succeed");
    let second_stats = renderer.last_frame_stats().expect("second frame stats");

    assert_eq!(first_stats.layer_cache_hits, 0);
    assert_eq!(first_stats.layer_cache_misses, 4);
    assert_eq!(second_stats.layer_cache_hits, 4);
    assert_eq!(second_stats.layer_cache_misses, 0);
    assert!(
        second_stats.isolated_layer_renders < first_stats.isolated_layer_renders,
        "cached child layers should avoid repainting isolated child surfaces"
    );
    assert!(
        second_stats.isolated_layer_pixels < first_stats.isolated_layer_pixels,
        "rigid scroll should reduce isolated layer repaint area after cache warmup"
    );
    assert!(
        second_stats.offscreen_acquires < first_stats.offscreen_acquires,
        "cache reuse should reduce offscreen acquisitions on the second frame"
    );
}

#[test]
fn static_text_glyph_atlas_reuses_raster_under_scroll_translation() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping static text glyph atlas assertion because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(text_scroll_like_graph(20.0));
    renderer
        .capture_frame(320, 140)
        .expect("first text capture should succeed");
    let first_stats = renderer.last_frame_stats().expect("first frame stats");

    renderer.scene_mut().graph = Some(text_scroll_like_graph(54.0));
    renderer
        .capture_frame(320, 140)
        .expect("translated text capture should succeed");
    let second_stats = renderer.last_frame_stats().expect("second frame stats");

    assert_eq!(first_stats.text_image_cache_misses, 0);
    assert!(
        first_stats.text_glyph_atlas_misses > 0,
        "first frame must populate the text glyph atlas: {first_stats:?}"
    );
    assert!(
        second_stats.text_glyph_atlas_hits >= first_stats.text_glyph_atlas_misses,
        "translated static text should reuse first-frame atlas glyphs: {second_stats:?}"
    );
    assert_eq!(
        second_stats.text_glyph_atlas_misses, 0,
        "scroll translation alone must not upload new atlas glyphs: {second_stats:?}"
    );
    assert_eq!(
        second_stats.text_image_raster_bytes, 0,
        "atlas hit frame should not allocate CPU text image raster bytes: {second_stats:?}"
    );
}

#[test]
fn text_glyph_atlas_reuses_identical_content_across_node_ids() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping repeated text glyph atlas assertion because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(repeated_text_graph());
    renderer
        .capture_frame(360, 160)
        .expect("repeated text capture should succeed");
    let stats = renderer.last_frame_stats().expect("frame stats");

    assert_eq!(
        stats.text_image_cache_misses, 0,
        "atlas-safe text should not populate whole text image cache: {stats:?}"
    );
    assert!(
        stats.text_glyph_atlas_hits > 0,
        "second identical text node should hit shared atlas glyphs: {stats:?}"
    );
    assert!(
        stats.text_glyph_atlas_misses > 0,
        "first repeated text node should populate atlas glyphs: {stats:?}"
    );
}

fn animated_shader_wgsl() -> String {
    format!(
        "{}\n{}",
        cranpose_ui_graphics::RUNTIME_SHADER_PRELUDE_WGSL,
        r"@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(u[0][0], 0.0, 0.0, 1.0);
}
"
    )
}

fn shaded_runtime_shader_layer(node_id: NodeId, time: f32) -> LayerNode {
    let shaded_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 48.0,
    };
    let mut shader = cranpose_ui_graphics::RuntimeShader::new(&animated_shader_wgsl());
    shader.set_position_independent(true);
    shader.set_float(0, time);
    let mut shaded = support::contract_layer(
        Some(node_id),
        CachePolicy::Auto,
        shaded_bounds,
        ProjectiveTransform::translation(8.0, 8.0),
        vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
                primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                    rect: shaded_bounds,
                    brush: Brush::solid(Color(0.0, 0.0, 0.0, 1.0)),
                    stroke: None,
                },
                clip: None,
            })),
        })],
    );
    shaded.graphics_layer.render_effect =
        Some(cranpose_ui_graphics::RenderEffect::runtime_shader(shader));
    shaded
}

fn shader_inside_cached_container_graph(time: f32) -> RenderGraph {
    let shaded = shaded_runtime_shader_layer(30_001, time);

    let mut container = support::contract_layer(
        Some(30_000),
        CachePolicy::Auto,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 96.0,
            height: 80.0,
        },
        ProjectiveTransform::translation(4.0, 4.0),
        vec![RenderNode::Layer(Box::new(shaded))],
    );
    container.clip_to_bounds = true;
    container.isolation.shape_clip = true;

    RenderGraph::new(support::contract_layer(
        Some(30_002),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 128.0,
            height: 96.0,
        },
        ProjectiveTransform::identity(),
        vec![RenderNode::Layer(Box::new(container))],
    ))
}

fn shader_layer_graph(time: f32) -> RenderGraph {
    let shaded = shaded_runtime_shader_layer(30_101, time);
    RenderGraph::new(support::contract_layer(
        Some(30_102),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 128.0,
            height: 96.0,
        },
        ProjectiveTransform::identity(),
        vec![RenderNode::Layer(Box::new(shaded))],
    ))
}

#[test]
fn a_runtime_shader_layer_with_stable_uniforms_is_served_from_the_layer_cache() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping runtime-shader cache assertion because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(shader_layer_graph(0.25));
    let first = renderer
        .capture_frame(128, 96)
        .expect("first shader capture should succeed");
    renderer.scene_mut().graph = Some(shader_layer_graph(0.25));
    renderer
        .capture_frame(128, 96)
        .expect("the second sighting admits the layer into the cache");
    renderer.scene_mut().graph = Some(shader_layer_graph(0.25));
    let second = renderer
        .capture_frame(128, 96)
        .expect("second shader capture should succeed");
    let stats = renderer.last_frame_stats().expect("frame stats");
    let cached_pass_count = stats.pass_count;
    assert_eq!(second.pixels, first.pixels);
    assert!(
        stats.layer_cache_hits >= 1 && stats.isolated_layer_renders == 0,
        "a runtime shader's output depends only on its uniforms, source and layer rect, all in \
         the cache key, so a repeated frame must not re-run it: {stats:?}"
    );

    renderer.scene_mut().graph = Some(shader_layer_graph(0.75));
    let third = renderer
        .capture_frame(128, 96)
        .expect("third shader capture should succeed");
    let stats = renderer.last_frame_stats().expect("frame stats");
    assert!(
        third.pixels != second.pixels,
        "a changed uniform must re-run the shader: {stats:?}"
    );
    assert!(
        stats.layer_cache_hits >= 1 && stats.isolated_layer_renders == 0,
        "the cache holds the layer's content, not its effect: a changed uniform re-runs the \
         shader over the cached content and re-renders nothing: {stats:?}"
    );
    assert_eq!(
        stats.pass_count, cached_pass_count,
        "a runtime shader over cached content draws in the final pass, not in a pass of its \
         own: {stats:?}"
    );
}

#[test]
fn an_animated_shader_keeps_animating_inside_a_cacheable_container() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping animated-shader cache assertion because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(shader_inside_cached_container_graph(0.0));
    let first = renderer
        .capture_frame(128, 96)
        .expect("first shader capture should succeed");

    renderer.scene_mut().graph = Some(shader_inside_cached_container_graph(1.0));
    let second = renderer
        .capture_frame(128, 96)
        .expect("second shader capture should succeed");

    let changed = first
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(second.pixels.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 0,
        "an animated shader inside a cacheable container must still animate: \
         the container cannot cache a subtree whose output changes every frame"
    );
}

#[derive(Clone, Copy, PartialEq)]
enum ShaderCoordinates {
    Uv,
    Fragment,
    ForwardedFragment,
    FragmentArgument,
}

fn scaled_shader_graph(
    scale: f32,
    separate_pass: bool,
    coordinates: ShaderCoordinates,
    empty: bool,
    declared: bool,
) -> RenderGraph {
    let mut shaded = shaded_runtime_shader_layer(30_201, 0.25);
    shaded.cache_policy = CachePolicy::None;
    shaded.graphics_layer.scale_x = scale;
    shaded.graphics_layer.scale_y = scale;
    shaded.transform_to_parent = ProjectiveTransform::uniform_scale(scale)
        .then(ProjectiveTransform::translation(13.25, 9.5));
    shaded.children = if empty {
        Vec::new()
    } else {
        vec![
            painted_rect(shaded.local_bounds, Color(0.2, 0.6, 0.9, 0.7)),
            painted_rect(
                Rect {
                    x: 7.25,
                    y: 4.5,
                    width: 29.5,
                    height: 21.25,
                },
                Color(0.9, 0.3, 0.1, 0.6),
            ),
        ]
    };
    let (helper, argument, light) = match coordinates {
        ShaderCoordinates::Uv => ("", "input: VertexOutput", "input.uv.x * input.uv.y"),
        ShaderCoordinates::Fragment => (
            "",
            "input: VertexOutput",
            "fract(input.position.x / 17.0) * fract(input.position.y / 11.0)",
        ),
        ShaderCoordinates::ForwardedFragment => (
            "fn lighting(input: VertexOutput) -> f32 { return fract(input.position.x / 17.0) * fract(input.position.y / 11.0); }",
            "input: VertexOutput",
            "lighting(input)",
        ),
        ShaderCoordinates::FragmentArgument => (
            "",
            "@builtin(position) position: vec4<f32>",
            "fract(position.x / 17.0) * fract(position.y / 11.0)",
        ),
    };
    let sample = if coordinates == ShaderCoordinates::Uv && !empty {
        "textureSample(input_texture, input_sampler, input.uv)"
    } else {
        "vec4<f32>(0.2, 0.3, 0.4, 0.7)"
    };
    let source = format!(
        "{}\n{helper}\n@fragment fn effect_fs({argument}) -> @location(0) vec4<f32> {{
            let source = {sample};
            let light = 0.1 + 0.4 * {light};
            return vec4<f32>(mix(source.rgb * source.rgb, vec3<f32>(source.a), light), source.a);
        }}",
        cranpose_ui_graphics::RUNTIME_SHADER_PRELUDE_WGSL,
    );
    let mut shader = cranpose_ui_graphics::RuntimeShader::new(&source);
    if coordinates == ShaderCoordinates::Uv && declared {
        shader.set_position_independent(true);
    }
    let effect = RenderEffect::runtime_shader(shader);
    shaded.graphics_layer.render_effect = Some(if separate_pass {
        effect.then(RenderEffect::offset(0.0, 0.0))
    } else {
        effect
    });
    support::page_graph(180, 140, vec![RenderNode::Layer(Box::new(shaded))])
}

#[test]
fn child_shader_compositing_preserves_coordinates_at_the_display_scale() {
    let mut renderer = support::headless_renderer().expect("headless renderer");
    for density in [1.0, 1.5, 3.0] {
        let width = (180.0 * density) as u32;
        let height = (140.0 * density) as u32;
        renderer.scene_mut().graph = Some(scaled_shader_graph(
            1.0,
            false,
            ShaderCoordinates::Uv,
            false,
            true,
        ));
        renderer
            .capture_frame_with_scale(width, height, density)
            .expect("translated shader composite");
        let translated_passes = renderer
            .last_frame_stats()
            .expect("translated stats")
            .pass_count;
        for (coordinates, empty) in [
            (ShaderCoordinates::Uv, false),
            (ShaderCoordinates::Fragment, false),
            (ShaderCoordinates::Fragment, true),
            (ShaderCoordinates::ForwardedFragment, true),
            (ShaderCoordinates::FragmentArgument, true),
        ] {
            for scale in [1.0, 0.85, 1.03, 1.25, 2.0] {
                renderer.scene_mut().graph =
                    Some(scaled_shader_graph(scale, true, coordinates, empty, true));
                let expected = renderer
                    .capture_frame_with_scale(width, height, density)
                    .expect("separate shader pass");
                renderer.scene_mut().graph =
                    Some(scaled_shader_graph(scale, false, coordinates, empty, true));
                let actual = renderer
                    .capture_frame_with_scale(width, height, density)
                    .expect("shader composite");
                support::assert_bytes_within(
                    "child shader composite",
                    width,
                    &expected.pixels,
                    &actual.pixels,
                    1,
                );
                if coordinates == ShaderCoordinates::Uv {
                    let stats = renderer.last_frame_stats().expect("frame stats");
                    assert_eq!(
                        stats.pass_count, translated_passes,
                        "a position-independent shader already on the display grid needs no extra pass: scale={scale}, density={density}, {stats:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn uv_only_child_shaders_need_no_opt_in_to_avoid_extra_surface_passes() {
    let mut renderer = support::headless_renderer().expect("headless renderer");
    for density in [1.0, 1.5, 3.0] {
        let width = (180.0 * density) as u32;
        let height = (140.0 * density) as u32;
        for empty in [false, true] {
            for scale in [1.0, 0.85, 1.25] {
                renderer.scene_mut().graph = Some(scaled_shader_graph(
                    scale,
                    false,
                    ShaderCoordinates::Uv,
                    empty,
                    true,
                ));
                renderer
                    .capture_frame_with_scale(width, height, density)
                    .expect("prepare transparent shader input");
                let expected = renderer
                    .capture_frame_with_scale(width, height, density)
                    .expect("declared shader");
                let expected_stats = renderer.last_frame_stats().expect("declared stats");
                renderer.scene_mut().graph = Some(scaled_shader_graph(
                    scale,
                    false,
                    ShaderCoordinates::Uv,
                    empty,
                    false,
                ));
                let actual = renderer
                    .capture_frame_with_scale(width, height, density)
                    .expect("undeclared shader");
                let actual_stats = renderer.last_frame_stats().expect("undeclared stats");
                assert_eq!(
                    actual_stats.pass_count, expected_stats.pass_count,
                    "UV-only shaders must fuse without a declaration: empty={empty}, scale={scale}, density={density}, {actual_stats:?}"
                );
                support::assert_bytes_within(
                    "inferred UV shader",
                    width,
                    &expected.pixels,
                    &actual.pixels,
                    0,
                );
            }
        }
    }
}

fn clipped_text_graph(y: f32) -> RenderGraph {
    let mut layer = text_layer(77, 16.25, y, "Cached short label");
    let RenderNode::Primitive(PrimitiveEntry {
        node: PrimitiveNode::Text(text),
        ..
    }) = &mut layer.children[0]
    else {
        panic!("text primitive")
    };
    text.clip = Some(Rect {
        x: 7.25,
        y: 2.0,
        width: 91.5,
        height: 19.0,
    });
    support::page_graph(320, 140, vec![RenderNode::Layer(Box::new(layer))])
}

fn nested_shadow_graph(padded: bool, scale: f32, shader: bool) -> RenderGraph {
    let caster = Rect {
        x: 0.0,
        y: 0.0,
        width: 44.0,
        height: 28.0,
    };
    let shadow = RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
            primitive: cranpose_ui_graphics::DrawPrimitive::Shadow(
                cranpose_ui_graphics::ShadowPrimitive::Drop {
                    shape: Box::new(cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: caster,
                        brush: Brush::solid(Color::BLACK.with_alpha(0.4)),
                        stroke: None,
                    }),
                    cutout: None,
                    blur_radius: 6.0,
                    blend_mode: cranpose_ui_graphics::BlendMode::SrcOver,
                },
            ),
            clip: None,
        })),
    });
    let inner = support::contract_layer(
        Some(90_002),
        CachePolicy::None,
        caster,
        ProjectiveTransform::identity(),
        vec![shadow],
    );
    let bounds = if padded {
        Rect {
            x: -32.0,
            y: -32.0,
            width: 108.0,
            height: 92.0,
        }
    } else {
        caster
    };
    let mut outer = support::contract_layer(
        Some(90_001),
        CachePolicy::None,
        bounds,
        ProjectiveTransform::uniform_scale(scale)
            .then(ProjectiveTransform::translation(54.0, 50.0)),
        vec![RenderNode::Layer(Box::new(inner))],
    );
    outer.graphics_layer.scale_x = scale;
    outer.graphics_layer.scale_y = scale;
    if shader {
        let mut effect = cranpose_ui_graphics::RuntimeShader::new(&format!(
            "{}\n@fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{ return textureSample(input_texture, input_sampler, input.uv); }}",
            cranpose_ui_graphics::RUNTIME_SHADER_PRELUDE_WGSL,
        ));
        effect.set_position_independent(true);
        outer.graphics_layer.render_effect = Some(RenderEffect::runtime_shader(effect));
        outer.isolation.effect = true;
    }
    support::page_graph(
        180,
        140,
        vec![
            painted_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 180.0,
                    height: 140.0,
                },
                Color::WHITE,
            ),
            RenderNode::Layer(Box::new(outer)),
        ],
    )
}

#[test]
fn transformed_and_shaded_layers_preserve_descendant_shadow_extents() {
    let mut renderer = support::headless_renderer().expect("headless renderer");
    for density in [1.0, 2.0] {
        for scale in [1.0, 1.25] {
            for shader in [false, true] {
                let width = (180.0 * density) as u32;
                let height = (140.0 * density) as u32;
                renderer.scene_mut().graph = Some(nested_shadow_graph(true, scale, shader));
                let reference = renderer
                    .capture_frame_with_scale(width, height, density)
                    .expect("padded reference");
                let reference_stats = renderer.last_frame_stats().expect("reference stats");
                renderer.scene_mut().graph = Some(nested_shadow_graph(false, scale, shader));
                let actual = renderer
                    .capture_frame_with_scale(width, height, density)
                    .expect("interaction shadow");
                support::assert_bytes_within(
                    &format!("descendant shadow density={density} scale={scale} shader={shader}"),
                    width,
                    &reference.pixels,
                    &actual.pixels,
                    1,
                );
                let stats = renderer.last_frame_stats().expect("shadow stats");
                assert!(stats.pass_count <= reference_stats.pass_count);
                assert!(stats.pass_pixels <= reference_stats.pass_pixels);
            }
        }
    }
}

#[test]
fn retained_short_text_matches_fresh_clipped_pixels_after_translation() {
    let mut renderer = support::headless_renderer().expect("headless renderer");
    for scale in [1.0, 1.5, 3.0] {
        let width = (320.0 * scale) as u32;
        let height = (140.0 * scale) as u32;
        for y in [20.25, -4.5, 72.75, 20.25] {
            let mut fresh = support::headless_renderer_beside_locked().expect("reference renderer");
            let context = cranpose_ui::AppContext::new();
            fresh.attach_app_context_services(&context);
            fresh.scene_mut().graph = Some(clipped_text_graph(y));
            let expected = context
                .enter(|| fresh.capture_frame_with_scale(width, height, scale))
                .expect("reference pixels");
            assert!(
                expected
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[0] > 128)
            );
            renderer.scene_mut().graph = Some(clipped_text_graph(y));
            for _ in 0..2 {
                let actual = renderer
                    .capture_frame_with_scale(width, height, scale)
                    .expect("retained pixels");
                let diff = cranpose_render_common::image_compare::image_difference_stats(
                    &expected.pixels,
                    &actual.pixels,
                    width,
                    height,
                    0,
                );
                assert_eq!(diff.differing_pixels, 0, "scale={scale} y={y}: {diff:?}");
            }
        }
    }
}
