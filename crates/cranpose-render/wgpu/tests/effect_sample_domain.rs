mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    Color, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader, TileMode,
};
use support::solid_rect;

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 120;
const GLASS: Rect = Rect {
    x: 80.0,
    y: 30.0,
    width: 96.0,
    height: 60.0,
};
const SUPPORT: Rect = Rect {
    x: 60.0,
    y: 24.0,
    width: 24.0,
    height: 20.0,
};
const TOGGLE: &str = "CRANPOSE_NO_EFFECT_DOMAINS";
const BLUR: f32 = 6.0;

fn support_mask_wgsl() -> String {
    format!(
        r#"    let pos = input.uv * vec2<f32>(textureDimensions(input_texture));
    if pos.x < {x} || pos.x >= {right} || pos.y < {y} || pos.y >= {bottom} {{
        return vec4<f32>(0.0);
    }}
"#,
        x = SUPPORT.x + BLUR,
        right = SUPPORT.x + SUPPORT.width + BLUR,
        y = SUPPORT.y + BLUR,
        bottom = SUPPORT.y + SUPPORT.height + BLUR,
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

fn effect_fs_wgsl(body: &str) -> String {
    format!(
        "{}\n@fragment\nfn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{\n{}{}}}\n",
        RUNTIME_SHADER_PRELUDE_WGSL,
        support_mask_wgsl(),
        body
    )
}

fn far_corner_wgsl() -> String {
    effect_fs_wgsl(
        r#"    let corner = textureSample(input_texture, input_sampler, vec2<f32>(0.02, 0.02));
    return vec4<f32>(corner.rgb, 1.0);
"#,
    )
}

fn nearby_wgsl(texels: f32) -> String {
    effect_fs_wgsl(&format!(
        r#"    let step = vec2<f32>({texels}, {texels}) / vec2<f32>(textureDimensions(input_texture));
    let near = textureSample(input_texture, input_sampler, input.uv + step);
    return vec4<f32>(near.rgb, 1.0);
"#
    ))
}

fn nearby_glass(samples: f32, declared: f32) -> RenderNode {
    let mut shader = RuntimeShader::new(&nearby_wgsl(samples));
    shader.set_output_support(Some(SUPPORT));
    shader.set_sample_domain(Some(Rect {
        x: SUPPORT.x - declared,
        y: SUPPORT.y - declared,
        width: SUPPORT.width + 2.0 * declared,
        height: SUPPORT.height + 2.0 * declared,
    }));
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, GLASS.width, GLASS.height),
        ProjectiveTransform::translation(GLASS.x, GLASS.y),
        GraphicsLayer {
            backdrop_effect: Some(
                RenderEffect::blur(BLUR).then(RenderEffect::runtime_shader(shader)),
            ),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

fn wrapped_corner_glass(alpha: f32) -> RenderNode {
    let mut shader = RuntimeShader::new(&far_corner_wgsl());
    shader.set_output_support(Some(SUPPORT));
    shader.set_sample_domain(Some(rect(-BLUR, -BLUR, 4.0, 4.0)));
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, GLASS.width, GLASS.height),
        ProjectiveTransform::translation(GLASS.x, GLASS.y),
        GraphicsLayer {
            backdrop_effect: Some(
                RenderEffect::blur_xy(BLUR, BLUR, TileMode::Repeated)
                    .then(RenderEffect::runtime_shader(shader)),
            ),
            alpha,
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

fn far_corner_glass(alpha: f32) -> RenderNode {
    let mut shader = RuntimeShader::new(&far_corner_wgsl());
    shader.set_output_support(Some(SUPPORT));
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, GLASS.width, GLASS.height),
        ProjectiveTransform::translation(GLASS.x, GLASS.y),
        GraphicsLayer {
            backdrop_effect: Some(
                RenderEffect::blur(BLUR).then(RenderEffect::runtime_shader(shader)),
            ),
            alpha,
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

fn page(glass: RenderNode) -> RenderGraph {
    let mut children = vec![solid_rect(
        rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
        Color::from_rgb_u8(20, 24, 40),
    )];
    for i in 0..12 {
        let x = GLASS.x - 8.0 + i as f32 * 9.0;
        children.push(solid_rect(
            rect(x, GLASS.y - 8.0, 5.0, GLASS.height + 16.0),
            Color::from_rgb_u8(250 - i * 15, 200, 40 + i * 12),
        ));
    }
    children.push(glass);
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn capture(
    renderer: &mut support::LockedRenderer,
    glass: impl Fn() -> RenderNode,
    whole: bool,
) -> (CapturedFrame, u64) {
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, whole.then_some("1"));
    let frame = support::capture_graph(renderer, page(glass()), FRAME_WIDTH, FRAME_HEIGHT);
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);
    let blur_pixels = renderer
        .last_frame_stats()
        .map_or(0, |stats| stats.blur_pixels);
    (frame, blur_pixels)
}

fn differing(a: &CapturedFrame, b: &CapturedFrame) -> usize {
    a.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count()
}

fn assert_whole_rect_is_read(alpha: f32, path: &str) {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (pruned, pruned_blur) = capture(&mut renderer, || far_corner_glass(alpha), false);
    let (whole, whole_blur) = capture(&mut renderer, || far_corner_glass(alpha), true);
    let count = differing(&pruned, &whole);
    assert_eq!(
        count, 0,
        "{path}: a shader that declares only where it writes may still sample anywhere in its \
         rect; {count} pixels differ when its blur is pruned to that support"
    );
    assert_eq!(
        pruned_blur, whole_blur,
        "{path}: without a sample domain the blur writes its whole region"
    );
}

fn assert_wrapped_taps_are_written(alpha: f32, path: &str) {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (pruned, _) = capture(&mut renderer, || wrapped_corner_glass(alpha), false);
    let (whole, _) = capture(&mut renderer, || wrapped_corner_glass(alpha), true);
    let count = differing(&pruned, &whole);
    assert_eq!(
        count, 0,
        "{path}: a repeating blur's taps at the domain's edge wrap to the opposite edge of \
         the capture, which the pass before must have written; {count} pixels differ"
    );
}

#[test]
fn a_page_backdrops_repeating_blur_writes_the_rows_its_wrapped_taps_read() {
    assert_wrapped_taps_are_written(1.0, "page backdrop");
}

#[test]
fn a_child_backdrops_repeating_blur_writes_the_rows_its_wrapped_taps_read() {
    assert_wrapped_taps_are_written(0.9, "child backdrop");
}

#[test]
fn a_declared_sample_domain_prunes_the_blur_to_it_and_lands_on_the_same_pixels() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (pruned, pruned_blur) = capture(&mut renderer, || nearby_glass(4.0, 4.0), false);
    let (whole, whole_blur) = capture(&mut renderer, || nearby_glass(4.0, 4.0), true);
    let count = differing(&pruned, &whole);
    assert_eq!(
        count, 0,
        "{count} pixels differ with the blur pruned to the declared domain"
    );
    assert!(
        pruned_blur < whole_blur,
        "the blur must write less inside the domain: {pruned_blur} against {whole_blur}"
    );
}

#[test]
fn a_sample_domain_smaller_than_what_the_shader_reads_shows_in_the_pixels() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (pruned, _) = capture(&mut renderer, || nearby_glass(12.0, 4.0), false);
    let (whole, _) = capture(&mut renderer, || nearby_glass(12.0, 4.0), true);
    let count = differing(&pruned, &whole);
    assert!(
        count > 0,
        "a shader reading 12 texels past a domain it declared 4 wide must render differently \
         when the blur is pruned to the declaration, else the pruning is not live"
    );
}

#[test]
fn a_page_backdrops_output_support_does_not_prune_what_its_shader_may_sample() {
    assert_whole_rect_is_read(1.0, "page backdrop");
}

#[test]
fn a_child_backdrops_output_support_does_not_prune_what_its_shader_may_sample() {
    assert_whole_rect_is_read(0.9, "child backdrop");
}
