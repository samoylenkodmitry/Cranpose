use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    Color, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader, TileMode,
};
use support::solid_rect;

use crate::{
    shared_test_support, support,
    support::glass_scene::{FRAME_HEIGHT, FRAME_WIDTH, GLASS},
};

const SUPPORT: Rect = Rect {
    x: 60.0,
    y: 24.0,
    width: 24.0,
    height: 20.0,
};
const TOGGLE: &str = "CRANPOSE_NO_EFFECT_DOMAINS";
const BLUR: f32 = 6.0;
const SAMPLE_OUTSET: f32 = 18.0;

fn support_mask_wgsl() -> String {
    format!(
        r"    let pos = input.uv * vec2<f32>(textureDimensions(input_texture));
    if pos.x < {x} || pos.x >= {right} || pos.y < {y} || pos.y >= {bottom} {{
        return vec4<f32>(0.0);
    }}
",
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
        r"    let corner = textureSample(input_texture, input_sampler, vec2<f32>(0.02, 0.02));
    return vec4<f32>(corner.rgb, 1.0);
",
    )
}

fn nearby_wgsl(texels: f32) -> String {
    effect_fs_wgsl(&format!(
        r"    let step = vec2<f32>({texels}, {texels}) / vec2<f32>(textureDimensions(input_texture));
    let near = textureSample(input_texture, input_sampler, input.uv + step);
    return vec4<f32>(near.rgb, 1.0);
"
    ))
}

fn nearby_glass(samples: f32, declared: f32) -> RenderNode {
    nearby_glass_with_layer(samples, declared, 1.0, 1.0, 1.0)
}

fn nearby_glass_with_layer(
    samples: f32,
    declared: f32,
    alpha: f32,
    scale_x: f32,
    scale_y: f32,
) -> RenderNode {
    let mut shader = RuntimeShader::new(&nearby_wgsl(samples));
    shader.set_output_support(Some(SUPPORT));
    shader.set_sample_domain(Some(Rect {
        x: SUPPORT.x - declared,
        y: SUPPORT.y - declared,
        width: SUPPORT.width + 2.0 * declared,
        height: SUPPORT.height + 2.0 * declared,
    }));
    let translate_x = GLASS.x + GLASS.width * (1.0 - scale_x) * 0.5;
    let translate_y = GLASS.y + GLASS.height * (1.0 - scale_y) * 0.5;
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, GLASS.width, GLASS.height),
        ProjectiveTransform::from_homogeneous([
            [scale_x, 0.0, translate_x],
            [0.0, scale_y, translate_y],
            [0.0, 0.0, 1.0],
        ]),
        GraphicsLayer {
            alpha,
            backdrop_effect: Some(
                RenderEffect::blur(BLUR).then(RenderEffect::runtime_shader(shader)),
            ),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

fn padded_sample_glass() -> RenderNode {
    let mut shader = RuntimeShader::new(&format!(
        r"{RUNTIME_SHADER_PRELUDE_WGSL}
@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
    let rect = u[{rect_slot}];
    let local_sample = vec2<f32>({sample_x}, {sample_y});
    let source_px = rect.xy + local_sample * rect.zw / vec2<f32>({width}, {height});
    let source_uv = source_px / vec2<f32>(textureDimensions(input_texture));
    return vec4<f32>(textureSample(input_texture, input_sampler, source_uv).rgb, 1.0);
}}
",
        rect_slot = RuntimeShader::EFFECT_RECT_UNIFORM / 4,
        sample_x = GLASS.width + SAMPLE_OUTSET,
        sample_y = GLASS.height * 0.5,
        width = GLASS.width,
        height = GLASS.height,
    ));
    shader.set_input_padding(20.0);
    let scale = 0.5;
    let translate_x = GLASS.x + GLASS.width * (1.0 - scale) * 0.5;
    let translate_y = GLASS.y + GLASS.height * (1.0 - scale) * 0.5;
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, GLASS.width, GLASS.height),
        ProjectiveTransform::from_homogeneous([
            [scale, 0.0, translate_x],
            [0.0, scale, translate_y],
            [0.0, 0.0, 1.0],
        ]),
        GraphicsLayer {
            alpha: 0.9,
            scale_x: scale,
            scale_y: scale,
            backdrop_effect: Some(RenderEffect::runtime_shader(shader)),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

fn padded_sample_page(glass: Option<RenderNode>) -> RenderGraph {
    let sample_x = GLASS.x + GLASS.width * 0.25 + 0.5 * (GLASS.width + SAMPLE_OUTSET);
    let sample_y = GLASS.y + GLASS.height * 0.25 + 0.5 * (GLASS.height * 0.5);
    let mut children = vec![solid_rect(
        rect(0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32),
        Color::from_rgb_u8(255, 0, 255),
    )];
    children.push(solid_rect(
        rect(sample_x - 1.0, sample_y - 6.0, 2.0, 12.0),
        Color::from_rgb_u8(0, 255, 0),
    ));
    if let Some(glass) = glass {
        children.push(glass);
    }
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
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
fn a_scaled_child_backdrop_sample_domain_preserves_pixels() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (without_effect, _) = capture(
        &mut renderer,
        || solid_rect(rect(0.0, 0.0, 0.0, 0.0), Color::TRANSPARENT),
        false,
    );
    for (scale_x, scale_y) in [(0.5, 0.5), (1.5, 1.5), (0.5, 1.5)] {
        let path = format!("child backdrop at scale ({scale_x}, {scale_y})");
        let (pruned, pruned_blur) = capture(
            &mut renderer,
            || nearby_glass_with_layer(4.0, 4.0, 0.9, scale_x, scale_y),
            false,
        );
        let (whole, whole_blur) = capture(
            &mut renderer,
            || nearby_glass_with_layer(4.0, 4.0, 0.9, scale_x, scale_y),
            true,
        );
        let count = differing(&pruned, &whole);
        assert_eq!(
            count, 0,
            "{path}: declared sample-domain pruning changed {count} pixels"
        );
        let changed = differing(&pruned, &without_effect);
        assert!(
            changed > 0,
            "{path}: the shader output must reach the frame; otherwise the pixel comparison is vacuous"
        );
        assert!(
            pruned_blur < whole_blur,
            "{path}: the declared domain should reduce blur work ({pruned_blur} vs {whole_blur})"
        );
    }
}

#[test]
fn a_scaled_child_backdrop_can_read_inside_its_declared_input_padding() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let frame = support::capture_graph(
        &mut renderer,
        padded_sample_page(Some(padded_sample_glass())),
        FRAME_WIDTH,
        FRAME_HEIGHT,
    );
    let center_x = (GLASS.x + GLASS.width * 0.5) as usize;
    let center_y = (GLASS.y + GLASS.height * 0.5) as usize;
    let pixel = &frame.pixels[(center_y * frame.width as usize + center_x) * 4..][..4];
    assert!(
        i16::from(pixel[1]) > i16::from(pixel[0]) + 80
            && i16::from(pixel[1]) > i16::from(pixel[2]) + 80,
        "the shader samples the green page stripe {SAMPLE_OUTSET} logical pixels past the pane; got {pixel:?}"
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
