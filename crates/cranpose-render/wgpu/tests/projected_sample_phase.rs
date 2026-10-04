use cranpose_render_common::{
    graph::{CachePolicy, LayerNode, ProjectiveTransform, RenderGraph, RenderNode},
    layer_transform::layer_transform_to_parent,
};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui_graphics::{
    Color, CompositingStrategy, GraphicsLayer, Point, RUNTIME_SHADER_PRELUDE_WGSL, Rect,
    RenderEffect, RuntimeShader,
};

use crate::support;

const FRAME: u32 = 192;
const INNER_BOUNDS: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 72.0,
    height: 72.0,
};
const EXPANDED_BOUNDS: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 96.0,
    height: 96.0,
};

#[test]
fn transparent_surface_extent_does_not_shift_projected_source_samples() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping projected sample phase test: {err}");
            return;
        }
    };

    for (case, transform) in [
        projected_transform(0.217, 1.137, 0.913, 45.37, 48.29),
        projected_transform(-0.163, 0.887, 1.129, 51.61, 44.23),
        projected_transform(0.341, 1.071, 1.083, 43.19, 52.47),
    ]
    .into_iter()
    .enumerate()
    {
        let inner =
            support::capture_graph(&mut renderer, graph(INNER_BOUNDS, transform), FRAME, FRAME);
        let expanded = support::capture_graph(
            &mut renderer,
            graph(EXPANDED_BOUNDS, transform),
            FRAME,
            FRAME,
        );
        let colored_pixels = inner
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] > 120 && pixel[2] > 120)
            .count();
        assert!(
            colored_pixels > 20,
            "transform {case} must leave visible painted content in the frame, found {colored_pixels} pixels"
        );
        assert_eq!(inner.pixels.len(), expanded.pixels.len());
        let mut differing_pixels = 0;
        let mut first_differences = Vec::with_capacity(6);
        for (index, (before, after)) in inner
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expanded.pixels.as_chunks::<4>().0.iter())
            .enumerate()
        {
            if before != after {
                differing_pixels += 1;
                if first_differences.len() < 6 {
                    first_differences.push((
                        (index % FRAME as usize, index / FRAME as usize),
                        [
                            i16::from(after[0]) - i16::from(before[0]),
                            i16::from(after[1]) - i16::from(before[1]),
                            i16::from(after[2]) - i16::from(before[2]),
                            i16::from(after[3]) - i16::from(before[3]),
                        ],
                    ));
                }
            }
        }
        assert_eq!(
            differing_pixels, 0,
            "transparent surface bounds changed {differing_pixels} pixels for transform {case}; first coordinates and channel deltas: {first_differences:?}"
        );
    }
}

fn projected_transform(
    angle: f32,
    scale_x: f32,
    scale_y: f32,
    translate_x: f32,
    translate_y: f32,
) -> ProjectiveTransform {
    let (sin, cos) = angle.sin_cos();
    ProjectiveTransform::from_homogeneous([
        [scale_x * cos, -scale_y * sin, translate_x],
        [scale_x * sin, scale_y * cos, translate_y],
        [0.000_17, -0.000_11, 1.0],
    ])
}

fn graph(bounds: Rect, transform: ProjectiveTransform) -> RenderGraph {
    let painted = [
        (26.0, 25.0, 2.4, Color(0.95, 0.12, 0.72, 1.0)),
        (30.1, 25.0, 3.1, Color(0.15, 0.82, 0.92, 1.0)),
        (35.0, 25.0, 2.6, Color(0.95, 0.82, 0.12, 1.0)),
        (39.2, 25.0, 3.7, Color(0.75, 0.2, 0.92, 1.0)),
        (26.0, 32.0, 18.0, Color(0.95, 0.12, 0.72, 1.0)),
    ]
    .into_iter()
    .map(|(x, y, width, color)| {
        support::solid_rect(
            Rect {
                x,
                y,
                width,
                height: if y == 25.0 { 19.0 } else { 1.7 },
            },
            color,
        )
    })
    .collect();
    let child = LayerNode {
        local_bounds: bounds,
        transform_to_parent: transform,
        cache_policy: CachePolicy::None,
        children: painted,
        ..LayerNode::default()
    };
    RenderGraph::new(LayerNode {
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: FRAME as f32,
            height: FRAME as f32,
        },
        children: vec![RenderNode::Layer(Box::new(child))],
        ..LayerNode::default()
    })
}

#[test]
fn reduced_group_backdrop_uses_the_group_raster_scale_once() {
    assert_scaled_group_backdrop(0.5, 0.0);
}

#[test]
fn enlarged_group_backdrop_uses_the_group_raster_scale_once() {
    assert_scaled_group_backdrop(2.0, 0.0);
}

#[test]
fn rotated_group_backdrop_uses_the_group_raster_scale_once() {
    assert_scaled_group_backdrop(1.0, 20.0);
}

fn assert_scaled_group_backdrop(scale: f32, angle: f32) {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping scaled group backdrop test: {err}");
            return;
        }
    };

    let (actual, actual_stats) = capture_scaled_backdrop(&mut renderer, scale, angle, false);
    let (reference, reference_stats) = capture_scaled_backdrop(&mut renderer, scale, angle, true);
    let painted_group_pixels = actual
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| {
            (pixel[0] > 170 && (45..130).contains(&pixel[1]) && pixel[2] < 110)
                || (pixel[0] < 90 && pixel[1] > 120 && pixel[2] > 150)
        })
        .count();
    assert!(
        painted_group_pixels > 20,
        "scale {scale} must leave visible group content, found {painted_group_pixels} pixels"
    );
    assert!(
        actual_stats.blur_pixels > 0 && reference_stats.blur_pixels > 0,
        "scale {scale} must render the backdrop in both scenes: actual {}, reference {} blur pixels",
        actual_stats.blur_pixels,
        reference_stats.blur_pixels
    );
    let differing_pixels = actual
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(reference.pixels.as_chunks::<4>().0)
        .filter(|(actual, reference)| actual != reference)
        .count();
    eprintln!(
        "scaled backdrop scale={scale}, angle={angle}: isolated pixels actual={} reference={}, blur pixels actual={} reference={}",
        actual_stats.isolated_layer_pixels,
        reference_stats.isolated_layer_pixels,
        actual_stats.blur_pixels,
        reference_stats.blur_pixels,
    );
    assert_eq!(
        differing_pixels, 0,
        "scale {scale}, angle {angle} changed {differing_pixels} pixels against the explicit parent-alpha / unit-inner-scale reference"
    );
}

fn capture_scaled_backdrop(
    renderer: &mut support::LockedRenderer,
    scale: f32,
    angle: f32,
    explicit_parent: bool,
) -> (CapturedFrame, RenderStatsSnapshot) {
    let width = 144.0;
    let height = 96.0;
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let mut graphics = GraphicsLayer {
        alpha: 0.9,
        scale_x: scale,
        scale_y: scale,
        rotation_z: angle,
        ..GraphicsLayer::default()
    };
    let transform = layer_transform_to_parent(bounds, Point::new(24.0, 18.0), &graphics);
    let content = || {
        vec![
            support::solid_rect(
                Rect {
                    x: 18.0,
                    y: 20.0,
                    width: 46.0,
                    height: 58.0,
                },
                Color::from_rgb_u8(240, 90, 35),
            ),
            support::solid_rect(
                Rect {
                    x: 76.0,
                    y: 28.0,
                    width: 48.0,
                    height: 50.0,
                },
                Color::from_rgb_u8(30, 180, 245),
            ),
        ]
    };
    let backdrop = RenderEffect::blur(9.0);
    let children = if explicit_parent {
        let inner = LayerNode {
            local_bounds: bounds,
            graphics_layer: GraphicsLayer {
                compositing_strategy: CompositingStrategy::Offscreen,
                backdrop_effect: Some(backdrop),
                ..GraphicsLayer::default()
            }
            .into(),
            children: content(),
            ..LayerNode::default()
        };
        vec![RenderNode::Layer(Box::new(inner))]
    } else {
        graphics.backdrop_effect = Some(backdrop);
        content()
    };
    let group = LayerNode {
        local_bounds: bounds,
        transform_to_parent: transform,
        graphics_layer: graphics.into(),
        children,
        ..LayerNode::default()
    };
    let mut page = vec![support::solid_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            width: FRAME as f32,
            height: FRAME as f32,
        },
        Color::from_rgb_u8(18, 24, 48),
    )];
    for index in 0..12 {
        page.push(support::solid_rect(
            Rect {
                x: 28.0 + index as f32 * 11.0,
                y: 14.0,
                width: 5.0,
                height: 150.0,
            },
            Color::from_rgb_u8(250 - index * 12, 80 + index * 9, 55 + index * 11),
        ));
    }
    page.push(RenderNode::Layer(Box::new(group)));
    let frame = support::capture_graph(
        renderer,
        RenderGraph::new(LayerNode {
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: FRAME as f32,
                height: FRAME as f32,
            },
            children: page,
            ..LayerNode::default()
        }),
        FRAME,
        FRAME,
    );
    let stats = renderer
        .last_frame_stats()
        .expect("captured frame publishes render statistics");
    (frame, stats)
}

#[test]
fn projected_backdrop_reads_the_valid_parent_branch_past_an_inverse_horizon() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping projected horizon backdrop test: {err}");
            return;
        }
    };
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}",
        r"
@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let rect = u[62];
    let source_px = rect.xy + vec2<f32>(35.0, 10.0) * rect.zw / vec2<f32>(20.0, 20.0);
    let source_uv = source_px / vec2<f32>(textureDimensions(input_texture));
    return vec4<f32>(textureSample(input_texture, input_sampler, source_uv).rgb, 1.0);
}
"
    ));
    shader.set_input_padding(15.0);
    let pane = LayerNode {
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        },
        transform_to_parent: ProjectiveTransform::from_homogeneous([
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.01, 0.0, 1.0],
        ]),
        graphics_layer: GraphicsLayer {
            backdrop_effect: Some(RenderEffect::runtime_shader(shader)),
            ..GraphicsLayer::default()
        }
        .into(),
        cache_policy: CachePolicy::None,
        ..LayerNode::default()
    };
    let graph = RenderGraph::new(LayerNode {
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: FRAME as f32,
            height: FRAME as f32,
        },
        children: vec![
            support::solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: FRAME as f32,
                    height: FRAME as f32,
                },
                Color::from_rgb_u8(255, 0, 255),
            ),
            support::solid_rect(
                Rect {
                    x: 22.0,
                    y: 4.0,
                    width: 8.0,
                    height: 8.0,
                },
                Color::from_rgb_u8(0, 255, 0),
            ),
            RenderNode::Layer(Box::new(pane)),
        ],
        ..LayerNode::default()
    });
    let frame = support::capture_graph(&mut renderer, graph, FRAME, FRAME);
    let pixel_x = 9usize;
    let pixel_y = 9usize;
    let pixel = &frame.pixels[(pixel_y * frame.width as usize + pixel_x) * 4..][..4];
    assert!(
        i16::from(pixel[1]) > i16::from(pixel[0]) + 100
            && i16::from(pixel[1]) > i16::from(pixel[2]) + 100,
        "the pane's local (35, 10) input maps to the green parent stripe at (25.9, 7.4); got {pixel:?}"
    );
}
