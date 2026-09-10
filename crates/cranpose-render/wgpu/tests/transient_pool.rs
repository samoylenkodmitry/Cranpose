mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode,
    },
};
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, DrawPrimitive, GraphicsLayer, LayerShape, Rect, RenderEffect,
    RoundedCornerShape, ShadowPrimitive,
};
use support::{
    glass_page::{
        BLUR_RADIUS, FRAME_HEIGHT, FRAME_WIDTH, GLASS_HEIGHT, GLASS_LEFT, GLASS_PITCH,
        GLASS_RADIUS, GLASS_TOP, GLASS_WIDTH, glass_shader,
    },
    solid_rect,
};

const STALE_SHADOWS: usize = 9;
const BAND: Color = Color(0.85, 0.15, 0.10, 1.0);

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn glass_layer(index: usize) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, GLASS_WIDTH, GLASS_HEIGHT),
        ProjectiveTransform::translation(GLASS_LEFT + index as f32 * GLASS_PITCH, GLASS_TOP),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(BLUR_RADIUS).then(glass_shader())),
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(GLASS_RADIUS)),
            ..GraphicsLayer::default()
        },
        vec![solid_rect(
            rect(14.0, 14.0, GLASS_WIDTH - 28.0, GLASS_HEIGHT - 28.0),
            Color::from_rgba_u8(255, 255, 255, 40),
        )],
    )))
}

fn drop_shadow(caster: Rect, blur_radius: f32) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Shadow(ShadowPrimitive::Drop {
                shape: Box::new(DrawPrimitive::Rect {
                    rect: caster,
                    brush: Brush::solid(Color::BLACK),
                    stroke: None,
                }),
                cutout: None,
                blur_radius,
                blend_mode: BlendMode::SrcOver,
            }),
            clip: None,
        }),
    })
}

fn stale_page() -> RenderGraph {
    let mut children = support::striped_page(FRAME_WIDTH, FRAME_HEIGHT);
    for index in 0..STALE_SHADOWS {
        let width = 10.0 + index as f32 * 7.0;
        let height = 8.0 + index as f32 * 5.0;
        children.push(drop_shadow(
            rect(4.0 + index as f32 * 20.0, 20.0, width, height),
            4.0 + index as f32,
        ));
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn glass_page_over(band: Option<Color>) -> RenderGraph {
    let mut children = support::striped_page(FRAME_WIDTH, FRAME_HEIGHT);
    if let Some(color) = band {
        children.push(solid_rect(
            rect(
                0.0,
                GLASS_TOP - 8.0,
                FRAME_WIDTH as f32,
                GLASS_HEIGHT + 16.0,
            ),
            color,
        ));
    }
    for index in 0..3 {
        children.push(glass_layer(index));
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn glass_page() -> RenderGraph {
    glass_page_over(None)
}

#[test]
fn a_repeated_frame_of_glasses_creates_no_transient_textures() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    renderer.scene_mut().graph = Some(stale_page());
    renderer
        .render_current_scene_to_texture(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("render should succeed");
    renderer.scene_mut().graph = Some(glass_page());
    let first = renderer
        .render_current_scene_to_texture(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("render should succeed");
    assert_eq!(
        first.offscreen_news, 3,
        "three blurred glasses of one stage resolve through one atlas, one scratch and one \
         result on their first frame, yet {} textures were created",
        first.offscreen_news
    );
    let second = renderer
        .render_current_scene_to_texture(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("render should succeed");
    assert_eq!(
        second.offscreen_news, 0,
        "the same page of glasses created {} transient textures on its second frame \
         (acquired {}), instead of reusing the first frame's",
        second.offscreen_news, second.offscreen_acquires
    );
}

#[test]
fn a_frame_through_reused_transients_matches_a_renderer_that_never_pooled() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    renderer.scene_mut().graph = Some(stale_page());
    renderer
        .render_current_scene_to_texture(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("render should succeed");
    support::capture_graph(&mut renderer, glass_page(), FRAME_WIDTH, FRAME_HEIGHT);
    let reused = support::capture_graph(
        &mut renderer,
        glass_page_over(Some(BAND)),
        FRAME_WIDTH,
        FRAME_HEIGHT,
    );
    let stats = renderer.last_frame_stats().expect("stats");
    assert_eq!(
        stats.offscreen_news, 0,
        "the band frame must resolve through reused transients for this to test reuse: {stats:?}"
    );
    let mut fresh = support::headless_renderer_beside_locked().expect("second headless renderer");
    fresh.scene_mut().graph = Some(glass_page_over(Some(BAND)));
    let reference = fresh
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("capture should succeed");
    support::assert_same_bytes(
        "glasses resolved through reused transients",
        FRAME_WIDTH,
        &reused.pixels,
        &reference.pixels,
    );
}
