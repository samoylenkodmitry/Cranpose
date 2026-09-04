mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode, TextPrimitiveNode,
    },
    image_compare::image_difference_stats,
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    TextLayoutOptions,
    text::{AnnotatedString, SpanStyle, TextStyle, TextUnit},
};
use cranpose_ui_graphics::{
    Color, DrawPrimitive, GraphicsLayer, ImageBitmap, ImageSampling, RUNTIME_SHADER_PRELUDE_WGSL,
    Rect, RenderEffect, RuntimeShader,
};
use support::{distinct_colors, region_pixels, solid_rect};

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 120;
const GLASS: Rect = Rect {
    x: 80.0,
    y: 30.0,
    width: 96.0,
    height: 60.0,
};
const ICON_SIZE: u32 = 32;

fn passthrough_wgsl() -> String {
    format!(
        "{}\n{}",
        RUNTIME_SHADER_PRELUDE_WGSL,
        r#"@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(input_texture, input_sampler, input.uv);
}
"#
    )
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn checker_icon() -> ImageBitmap {
    let mut pixels = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let light = (x / 4 + y / 4) % 2 == 0;
            let rgba = if light {
                [240, 220, 90, 255]
            } else {
                [40, 90, 160, 255]
            };
            let index = ((y * ICON_SIZE + x) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&rgba);
        }
    }
    ImageBitmap::from_rgba8(ICON_SIZE, ICON_SIZE, pixels).expect("valid icon")
}

fn primitive(primitive: DrawPrimitive) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive,
            clip: None,
        }),
    })
}

/// A text across the glass's left edge, an image across its top edge and an
/// isolated child across its right edge: each is drawn partly inside the
/// capture and would be dropped by a cull that judges by the wrong rect.
fn straddling_page() -> Vec<RenderNode> {
    let text_style = TextStyle::from_span_style(SpanStyle {
        color: Some(Color::WHITE),
        font_size: TextUnit::Sp(18.0),
        ..Default::default()
    });
    vec![
        solid_rect(
            rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
            Color::from_rgb_u8(24, 28, 40),
        ),
        RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 7,
                rect: rect(GLASS.x - 40.0, GLASS.y + 20.0, 90.0, 24.0),
                text: std::rc::Rc::new(AnnotatedString::from("Straddle")),
                text_style,
                font_size: 18.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        }),
        primitive(DrawPrimitive::Image {
            rect: rect(
                GLASS.x + 20.0,
                GLASS.y - 16.0,
                ICON_SIZE as f32,
                ICON_SIZE as f32,
            ),
            image: checker_icon(),
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Nearest,
            src_rect: None,
        }),
        RenderNode::Layer(Box::new(shared_test_support::layer_node(
            rect(0.0, 0.0, 40.0, 40.0),
            ProjectiveTransform::translation(GLASS.x + GLASS.width - 20.0, GLASS.y + 12.0),
            GraphicsLayer {
                alpha: 0.8,
                ..GraphicsLayer::default()
            },
            vec![solid_rect(
                rect(0.0, 0.0, 40.0, 40.0),
                Color::from_rgb_u8(250, 120, 40),
            )],
        ))),
    ]
}

fn passthrough_glass() -> RenderNode {
    passthrough_glass_at(GLASS)
}

fn page(with_glass: bool) -> RenderGraph {
    let mut children = straddling_page();
    if with_glass {
        children.push(passthrough_glass());
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn capture(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> CapturedFrame {
    support::capture_graph(renderer, graph, FRAME_WIDTH, FRAME_HEIGHT)
}

#[test]
fn a_capture_holds_every_text_image_and_composite_that_reaches_into_it() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let without = capture(&mut renderer, page(false));
    let with = capture(&mut renderer, page(true));
    let inside = rect(
        GLASS.x + 2.0,
        GLASS.y + 2.0,
        GLASS.width - 4.0,
        GLASS.height - 4.0,
    );
    let expected = region_pixels(&without, inside);
    assert!(
        distinct_colors(&expected) > 8,
        "the page under the glass must carry the text, the icon and the child"
    );
    let stats = image_difference_stats(
        &expected,
        &region_pixels(&with, inside),
        inside.width as u32,
        inside.height as u32,
        1,
    );
    assert_eq!(
        stats.differing_pixels, 0,
        "a pass-through glass must show exactly the page beneath it: differing_pixels={} \
         max_diff={} first={:?}",
        stats.differing_pixels, stats.max_difference, stats.first_difference
    );
}

const SECOND_GLASS: Rect = Rect {
    x: 8.0,
    y: 60.0,
    width: 60.0,
    height: 50.0,
};

fn passthrough_glass_at(bounds: Rect) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, bounds.width, bounds.height),
        ProjectiveTransform::translation(bounds.x, bounds.y),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::runtime_shader(RuntimeShader::new(
                &passthrough_wgsl(),
            ))),
            clip: true,
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

/// Two glasses that do not overlap share one stage; a stripe drawn between
/// them in z reaches under the later one and must show through it.
fn staged_page(with_glasses: bool) -> RenderGraph {
    let mut children = vec![solid_rect(
        rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
        Color::from_rgb_u8(24, 28, 40),
    )];
    if with_glasses {
        children.push(passthrough_glass_at(SECOND_GLASS));
    }
    children.push(solid_rect(
        rect(0.0, SECOND_GLASS.y + 10.0, FRAME_WIDTH as f32, 12.0),
        Color::from_rgb_u8(250, 200, 40),
    ));
    if with_glasses {
        children.push(passthrough_glass_at(GLASS));
    }
    children.push(solid_rect(
        rect(0.0, GLASS.y + 30.0, FRAME_WIDTH as f32, 8.0),
        Color::from_rgb_u8(40, 220, 120),
    ));
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

#[test]
fn a_later_glass_of_a_stage_shows_what_was_drawn_after_the_earlier_glass() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let without = capture(&mut renderer, staged_page(false));
    let with = capture(&mut renderer, staged_page(true));
    for (label, glass) in [("first", SECOND_GLASS), ("second", GLASS)] {
        let inside = rect(
            glass.x + 2.0,
            glass.y + 2.0,
            glass.width - 4.0,
            glass.height - 4.0,
        );
        let expected = region_pixels(&without, inside);
        let stats = image_difference_stats(
            &expected,
            &region_pixels(&with, inside),
            inside.width as u32,
            inside.height as u32,
            1,
        );
        assert_eq!(
            stats.differing_pixels, 0,
            "the {label} glass must show the page beneath it, stripe included: \
             differing_pixels={} first={:?}",
            stats.differing_pixels, stats.first_difference
        );
    }
}

/// The stripes of `staged_page` drawn before any glass, so every capture
/// finds them on the page.
fn glazed_page(with_glasses: bool) -> RenderGraph {
    let mut children = vec![
        solid_rect(
            rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
            Color::from_rgb_u8(24, 28, 40),
        ),
        solid_rect(
            rect(0.0, SECOND_GLASS.y + 10.0, FRAME_WIDTH as f32, 12.0),
            Color::from_rgb_u8(250, 200, 40),
        ),
        solid_rect(
            rect(0.0, GLASS.y + 30.0, FRAME_WIDTH as f32, 8.0),
            Color::from_rgb_u8(40, 220, 120),
        ),
    ];
    if with_glasses {
        children.push(passthrough_glass_at(SECOND_GLASS));
        children.push(passthrough_glass_at(GLASS));
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

#[test]
fn a_capture_adds_no_shape_fill_of_its_own() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let fill = |renderer: &mut support::LockedRenderer, graph: RenderGraph| {
        capture(renderer, graph);
        renderer
            .last_frame_stats()
            .expect("frame stats")
            .shape_fill_pixels
    };
    let plain = fill(&mut renderer, glazed_page(false));
    let glazed = fill(&mut renderer, glazed_page(true));
    assert_eq!(
        glazed, plain,
        "a capture reads the page's pixels; it never draws the page's shapes again"
    );
}

#[test]
fn a_glass_is_shaded_once_over_its_visible_pixels() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    capture(&mut renderer, staged_page(true));
    let shaded = renderer
        .last_frame_stats()
        .expect("frame stats")
        .shader_pixels;
    let visible = (GLASS.width * GLASS.height + SECOND_GLASS.width * SECOND_GLASS.height) as u64;
    assert_eq!(
        shaded, visible,
        "two pass-through glasses shade exactly their visible pixels once"
    );
}
