use std::sync::atomic::Ordering;

use cranpose_ui_graphics::{
    GRADIENT_BLUR_WGSL, GRADIENT_CUT_MASK_WGSL, GRADIENT_FADE_DST_OUT_WGSL, LIQUID_GLASS_WGSL,
    ROUNDED_ALPHA_MASK_WGSL, RuntimeShader,
};
use web_time::{Duration, Instant};

use super::{
    RuntimeShaderPipelineMode, ShaderDrawVariant, ShaderPipelineCache, ShaderPipelineFit,
    validate_runtime_shader_source,
};
use crate::{
    effect_renderer::EffectRenderer,
    pipeline::GPU_TEXT_BRUSH_EFFECT_SHADER,
    pipeline_compiler::{CompileLane, PipelineCompiler},
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SETTLE: Duration = Duration::from_secs(30);

fn cache(device: &wgpu::Device, compiler: PipelineCompiler) -> ShaderPipelineCache {
    let renderer = EffectRenderer::new(
        device,
        PipelineCompiler::inactive(),
        None,
        FORMAT,
        device.adapter_info().backend,
    );
    ShaderPipelineCache::new(
        device,
        compiler,
        None,
        device.adapter_info().backend,
        FORMAT,
        &renderer.effect_texture_bind_group_layout,
        &renderer.effect_uniform_bind_group_layout,
    )
}

fn split_shader() -> RuntimeShader {
    let mut shader = RuntimeShader::new(&format!(
        "{}\noverride RED: bool = false;\noverride SPLIT: i32 = 0;",
        valid_shader()
    ));
    shader.set_override("RED", 0.0);
    shader.set_draw_split(Some("SPLIT"));
    shader.set_specialization_exact(true);
    shader
}

fn builds(cache: &ShaderPipelineCache) -> (usize, usize) {
    (
        cache.factory.counters.modules.load(Ordering::Relaxed),
        cache.factory.counters.pipelines.load(Ordering::Relaxed),
    )
}

fn settle(cache: &mut ShaderPipelineCache, shader: &RuntimeShader) {
    let deadline = Instant::now() + SETTLE;
    loop {
        let pending = [
            ShaderDrawVariant::Whole,
            ShaderDrawVariant::Interior,
            ShaderDrawVariant::Rim,
        ]
        .into_iter()
        .any(|variant| {
            let (_, fit) = cache
                .get_or_create(shader, RuntimeShaderPipelineMode::Replace, variant)
                .expect("valid shader");
            fit == ShaderPipelineFit::Fallback
        });
        if !pending {
            return;
        }
        assert!(Instant::now() < deadline, "specializations never landed");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn warm_pipeline_lookups_do_not_rebuild_constants() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::inactive());
    let shader = split_shader();
    for forced in [false, true, false] {
        cache.set_forced_flags(forced.then_some("RED").into_iter());
        for _ in 0..12 {
            for variant in [
                ShaderDrawVariant::Whole,
                ShaderDrawVariant::Interior,
                ShaderDrawVariant::Rim,
            ] {
                let (_, fit) = cache
                    .get_or_create(&shader, RuntimeShaderPipelineMode::Replace, variant)
                    .expect("valid shader");
                assert_eq!(fit, ShaderPipelineFit::Specialized);
            }
        }
    }
    assert_eq!(cache.pipelines.len(), 6);
    assert_eq!(builds(&cache), (1, 6), "one module, one build per variant");
}

#[test]
fn a_specialization_draws_with_the_general_pipeline_until_it_lands() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::spawn());
    let shader = split_shader();
    let mode = RuntimeShaderPipelineMode::Replace;
    let (general, fit) = cache
        .get_or_create(&shader, mode, ShaderDrawVariant::Interior)
        .expect("valid shader");
    assert_eq!(fit, ShaderPipelineFit::Fallback);
    let general = general.clone();
    let (again, fit) = cache
        .get_or_create(&shader, mode, ShaderDrawVariant::Rim)
        .expect("valid shader");
    assert_eq!(fit, ShaderPipelineFit::Fallback);
    assert!(*again == general, "both draws share the general pipeline");
    settle(&mut cache, &shader);
    for variant in [ShaderDrawVariant::Interior, ShaderDrawVariant::Rim] {
        let (specialized, fit) = cache
            .get_or_create(&shader, mode, variant)
            .expect("valid shader");
        assert_eq!(fit, ShaderPipelineFit::Specialized);
        assert!(*specialized != general, "{variant:?} has its own pipeline");
    }
    let (whole, fit) = cache
        .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
        .expect("valid shader");
    assert_eq!(
        fit,
        ShaderPipelineFit::Specialized,
        "an override set is a specialization"
    );
    assert!(*whole != general);
    assert_eq!(
        builds(&cache),
        (1, 4),
        "the general pipeline plus three variants"
    );
}

#[test]
fn an_override_that_is_not_a_fold_compiles_inside_the_frame() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::spawn());
    let mut shader = split_shader();
    shader.set_specialization_exact(false);
    let (_, fit) = cache
        .get_or_create(
            &shader,
            RuntimeShaderPipelineMode::Replace,
            ShaderDrawVariant::Interior,
        )
        .expect("valid shader");
    assert_eq!(fit, ShaderPipelineFit::Specialized);
    assert_eq!(
        builds(&cache),
        (1, 1),
        "the requested variant itself was built, and nothing else"
    );
}

#[test]
fn a_warmed_general_pipeline_is_ready_before_its_first_draw() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::spawn());
    let shader = RuntimeShader::new(&valid_shader());
    let mode = RuntimeShaderPipelineMode::PremultipliedSrcOver;
    cache.warm(&shader, mode);
    cache.warm(&shader, mode);
    let deadline = Instant::now() + SETTLE;
    while !cache.ready(cache.key(&shader, mode, ShaderDrawVariant::Whole)) {
        assert!(Instant::now() < deadline, "the warm-up never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(builds(&cache), (1, 1), "one warm-up per general pipeline");
    let (_, fit) = cache
        .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
        .expect("valid shader");
    assert_eq!(fit, ShaderPipelineFit::General);
    assert_eq!(
        builds(&cache),
        (1, 1),
        "the draw found the warm-up's pipeline"
    );
}

#[test]
fn a_drawn_specialization_does_not_wait_for_its_queued_warm_up() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let compiler = PipelineCompiler::spawn();
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    compiler.enqueue(CompileLane::WarmUp, move || {
        let _ = blocked.recv();
    });
    let mut cache = cache(&device, compiler.clone());
    let shader = split_shader();
    let mode = RuntimeShaderPipelineMode::Replace;
    cache.warm(&shader, mode);
    let (_, fit) = cache
        .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
        .expect("valid shader");
    assert_eq!(fit, ShaderPipelineFit::Fallback);
    let key = cache.key(&shader, mode, ShaderDrawVariant::Whole);
    let deadline = Instant::now() + SETTLE;
    while !cache.ready(key) {
        assert!(
            Instant::now() < deadline,
            "the drawn specialization waited behind the warm-up lane"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    release.send(()).expect("the blocking warm-up is waiting");
    let (drained, warm_ups_done) = std::sync::mpsc::channel();
    compiler.enqueue(CompileLane::WarmUp, move || {
        let _ = drained.send(());
    });
    warm_ups_done
        .recv_timeout(SETTLE)
        .expect("the warm-up lane drains");
    assert_eq!(
        builds(&cache),
        (1, 2),
        "the general stand-in and the specialization, each built once"
    );
}

#[test]
fn an_invalid_shader_disables_every_variant_once() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::spawn());
    let mut shader = RuntimeShader::new("this is not wgsl");
    shader.set_override("RED", 1.0);
    let mode = RuntimeShaderPipelineMode::Replace;
    assert!(
        cache
            .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
            .is_none()
    );
    assert!(
        cache
            .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
            .is_none()
    );
    assert_eq!(
        builds(&cache),
        (1, 0),
        "validation ran once and built nothing"
    );
}

fn valid_shader() -> String {
    format!(
        "{}\n{}",
        cranpose_ui_graphics::RUNTIME_SHADER_PRELUDE_WGSL,
        r"@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
return textureSample(input_texture, input_sampler, input.uv);
}
"
    )
}

#[test]
fn validator_accepts_valid_runtime_shader() {
    assert!(validate_runtime_shader_source(&valid_shader(), wgpu::Backend::Vulkan).is_ok());
}

#[test]
fn validator_rejects_invalid_wgsl() {
    let invalid = "this is not wgsl";
    assert!(validate_runtime_shader_source(invalid, wgpu::Backend::Vulkan).is_err());
}

#[test]
fn validator_rejects_missing_required_entry_points() {
    let missing_effect_fs = r"
@vertex
fn fullscreen_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
let x = f32(i32(i & 1u) * 2 - 1);
let y = f32(i32(i >> 1u) * 2 - 1);
return vec4<f32>(x, y, 0.0, 1.0);
}
";
    assert!(validate_runtime_shader_source(missing_effect_fs, wgpu::Backend::Vulkan).is_err());
}

#[test]
fn validator_accepts_gl_portable_builtin_runtime_shaders() {
    for (name, source) in [
        ("gradient_blur", GRADIENT_BLUR_WGSL),
        ("gradient_cut_mask", GRADIENT_CUT_MASK_WGSL),
        ("rounded_alpha_mask", ROUNDED_ALPHA_MASK_WGSL),
        ("gradient_fade_dst_out", GRADIENT_FADE_DST_OUT_WGSL),
        ("liquid_glass", LIQUID_GLASS_WGSL),
        ("gpu_text_brush_effect", GPU_TEXT_BRUSH_EFFECT_SHADER),
    ] {
        let result = validate_runtime_shader_source(source, wgpu::Backend::Gl);
        assert!(
            result.is_ok(),
            "{name} should remain GL-portable: {}",
            result.err().unwrap_or_default()
        );
    }
}
