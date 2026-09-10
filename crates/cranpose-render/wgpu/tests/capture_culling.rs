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
    BlendMode, Brush, Color, DrawPrimitive, GraphicsLayer, ImageBitmap, ImageSampling,
    RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader, ShadowPrimitive,
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
    let texel = vec2<i32>(input.uv * vec2<f32>(textureDimensions(input_texture)));
    return textureLoad(input_texture, texel, 0);
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
/// finds them on the page, plus a vertical stripe through each glass so a
/// capture shifted along either axis reads differently.
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
        solid_rect(
            rect(SECOND_GLASS.x + 12.0, 0.0, 6.0, FRAME_HEIGHT as f32),
            Color::from_rgb_u8(230, 60, 90),
        ),
        solid_rect(
            rect(GLASS.x + 40.0, 0.0, 6.0, FRAME_HEIGHT as f32),
            Color::from_rgb_u8(90, 140, 240),
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

fn passes_and_copies(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> (u32, u32) {
    capture(renderer, graph);
    let stats = renderer.last_frame_stats().expect("frame stats");
    (stats.pass_count, stats.copy_count)
}

#[test]
fn a_capture_of_the_page_is_a_copy_that_records_no_pass() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let plain = capture(&mut renderer, glazed_page(false));
    let (plain_passes, plain_copies) = passes_and_copies(&mut renderer, glazed_page(false));
    let glazed = capture(&mut renderer, glazed_page(true));
    let (glazed_passes, glazed_copies) = passes_and_copies(&mut renderer, glazed_page(true));
    assert_eq!(plain_copies, 0, "a page without captures copies nothing");
    assert_eq!(
        glazed_copies, 2,
        "each glass over the finished page reads it through one copy"
    );
    assert_eq!(
        glazed_passes,
        plain_passes + 1,
        "captures with nothing to fix up add only the stratum that draws the glasses"
    );
    for glass in [SECOND_GLASS, GLASS] {
        support::assert_same_bytes(
            "a copied capture shows the page's texels",
            glass.width as u32,
            &region_pixels(&plain, glass),
            &region_pixels(&glazed, glass),
        );
    }
}

#[test]
fn a_fix_up_draws_over_the_copies_in_one_loaded_pass() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (plain_passes, _) = passes_and_copies(&mut renderer, staged_page(false));
    let (staged_passes, staged_copies) = passes_and_copies(&mut renderer, staged_page(true));
    assert_eq!(staged_copies, 2, "both glasses of the stage copy the page");
    assert_eq!(
        staged_passes,
        plain_passes + 2,
        "the stripe under the later glass is drawn over the copies in one pass, \
         and the glasses draw in one more stratum"
    );
}

fn tint_wgsl() -> String {
    format!(
        "{}\n{}",
        RUNTIME_SHADER_PRELUDE_WGSL,
        r#"@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let source = textureSample(input_texture, input_sampler, input.uv);
    return vec4<f32>(source.rgb * 0.5 + vec3<f32>(0.5, 0.0, 0.0), 1.0);
}
"#
    )
}

/// A glass at `bounds` that halves what it reads and adds red, so what is
/// drawn over it and what it reads are told apart by the red channel.
fn tint_glass_at(bounds: Rect) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, bounds.width, bounds.height),
        ProjectiveTransform::translation(bounds.x, bounds.y),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::runtime_shader(RuntimeShader::new(
                &tint_wgsl(),
            ))),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

fn drop_shadow(shape: Rect, alpha: f32, blur_radius: f32) -> RenderNode {
    primitive(DrawPrimitive::Shadow(ShadowPrimitive::Drop {
        shape: Box::new(DrawPrimitive::Rect {
            rect: shape,
            brush: Brush::solid(Color(0.0, 0.0, 0.0, alpha)),
            stroke: None,
        }),
        cutout: None,
        blur_radius,
        blend_mode: BlendMode::SrcOver,
    }))
}

/// A blurred shadow drawn in z between two glasses of one stage, lying under
/// the later glass and clear of the earlier one.
fn shadowed_stage_page(with_glasses: bool) -> RenderGraph {
    let mut children = vec![solid_rect(
        rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
        Color::from_rgb_u8(200, 200, 210),
    )];
    if with_glasses {
        children.push(passthrough_glass_at(SECOND_GLASS));
    }
    children.push(drop_shadow(
        rect(GLASS.x + 20.0, GLASS.y + 10.0, 50.0, 30.0),
        0.8,
        5.0,
    ));
    if with_glasses {
        children.push(passthrough_glass_at(GLASS));
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

#[test]
fn a_shadow_below_a_later_glass_of_the_stage_is_on_the_page_before_that_glass_copies() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let plain = capture(&mut renderer, shadowed_stage_page(false));
    let glazed = capture(&mut renderer, shadowed_stage_page(true));
    let stats = renderer.last_frame_stats().expect("frame stats");
    assert_eq!(stats.copy_count, 2, "both glasses copy the page");
    assert_eq!(
        stats.capture_fixup_passes, 0,
        "the shadow under the later glass is drawn into the page before that glass copies, \
         never replayed into its capture"
    );
    let inside = rect(
        GLASS.x + 2.0,
        GLASS.y + 2.0,
        GLASS.width - 4.0,
        GLASS.height - 4.0,
    );
    assert!(
        distinct_colors(&region_pixels(&plain, inside)) > 4,
        "the shadow must fall inside the later glass"
    );
    support::assert_same_bytes(
        "a pass-through glass over its shadow shows the page beneath it",
        inside.width as u32,
        &region_pixels(&plain, inside),
        &region_pixels(&glazed, inside),
    );
}

const CONTENT: Rect = Rect {
    x: 20.0,
    y: 72.0,
    width: 30.0,
    height: 20.0,
};

/// A tinting glass, content drawn over it, a shadow drawn over it, a stripe
/// under a second glass of the same stage, then that glass: the stripe makes
/// the page settle under the second glass, and the content and shadow over
/// the first must wait for its composite.
fn covered_glass_page() -> RenderGraph {
    support::page_graph(
        FRAME_WIDTH,
        FRAME_HEIGHT,
        vec![
            solid_rect(
                rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
                Color::from_rgb_u8(20, 40, 60),
            ),
            tint_glass_at(SECOND_GLASS),
            solid_rect(CONTENT, Color::from_rgb_u8(30, 220, 30)),
            drop_shadow(
                rect(SECOND_GLASS.x - 10.0, SECOND_GLASS.y + 20.0, 40.0, 60.0),
                1.0,
                4.0,
            ),
            solid_rect(
                rect(GLASS.x - 10.0, GLASS.y + 20.0, GLASS.width + 20.0, 10.0),
                Color::from_rgb_u8(240, 200, 60),
            ),
            passthrough_glass_at(GLASS),
        ],
    )
}

#[test]
fn content_drawn_over_a_glass_stays_over_it_when_a_later_glass_copies_the_page() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let frame = capture(&mut renderer, covered_glass_page());
    let pixel = |x: f32, y: f32| {
        let index = ((y as u32 * frame.width + x as u32) * 4) as usize;
        [
            frame.pixels[index],
            frame.pixels[index + 1],
            frame.pixels[index + 2],
        ]
    };
    let content = pixel(CONTENT.x + 26.0, CONTENT.y + 4.0);
    assert!(
        content[1] > 200 && content[0] < 60,
        "content drawn after the glass keeps its own color over the tint: {content:?}"
    );
    let shadowed = pixel(SECOND_GLASS.x + 8.0, SECOND_GLASS.y + 40.0);
    assert!(
        shadowed[0] < 40,
        "a shadow drawn after the glass darkens the tinted result rather than being tinted: \
         {shadowed:?}"
    );
    let tinted = pixel(SECOND_GLASS.x + 50.0, SECOND_GLASS.y + 8.0);
    assert!(
        tinted[0] > 120,
        "the glass itself shows its red tint where nothing covers it: {tinted:?}"
    );
}
