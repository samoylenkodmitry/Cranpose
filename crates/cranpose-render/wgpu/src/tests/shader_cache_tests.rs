use std::sync::atomic::Ordering;

use cranpose_ui_graphics::{
    GRADIENT_BLUR_WGSL, GRADIENT_CUT_MASK_WGSL, GRADIENT_FADE_DST_OUT_WGSL, LIQUID_GLASS_WGSL,
    ROUNDED_ALPHA_MASK_WGSL, RuntimeShader,
};
use web_time::{Duration, Instant};

use super::{
    FRAMEWORK_RUNTIME_SHADERS, RuntimeShaderPipelineMode, ShaderPipelineCache, ShaderPipelineFit,
    validate_runtime_shader_source,
};
use crate::{
    effect_renderer::EffectRenderer,
    pipeline::GPU_TEXT_BRUSH_EFFECT_SHADER,
    pipeline_compiler::{CompileLane, PipelineCompiler, tests::hold_warm_ups},
    pipeline_records::ShaderPipelineRecord,
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
        crate::pipeline_recorder::PipelineRecorder::default(),
    );
    ShaderPipelineCache::new(
        device,
        compiler,
        None,
        device.adapter_info().backend,
        FORMAT,
        [
            &renderer.effect_texture_bind_group_layout,
            &renderer.effect_uniform_bind_group_layout,
        ],
        crate::pipeline_recorder::PipelineRecorder::default(),
    )
}

fn material_shader() -> RuntimeShader {
    let mut shader =
        RuntimeShader::new(&format!("{}\noverride RED: bool = false;", valid_shader()));
    shader.set_override("RED", 0.0);
    shader.set_specialization_exact(true);
    shader
}

/// How `shader`'s draw was served.
fn draw(
    cache: &mut ShaderPipelineCache,
    shader: &RuntimeShader,
    mode: RuntimeShaderPipelineMode,
) -> ShaderPipelineFit {
    cache
        .get_or_create(shader, shader.draw_specialization(0), mode)
        .expect("valid shader")
        .1
}

fn builds(cache: &ShaderPipelineCache) -> (usize, usize) {
    (
        cache.factory.counters.modules.load(Ordering::Relaxed),
        cache.factory.counters.pipelines.load(Ordering::Relaxed),
    )
}

#[test]
fn warm_pipeline_lookups_do_not_rebuild_constants() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::inactive());
    let shader = material_shader();
    for forced in [false, true, false] {
        cache.set_forced_flags(forced.then_some("RED").into_iter());
        for _ in 0..12 {
            let fit = draw(&mut cache, &shader, RuntimeShaderPipelineMode::Replace);
            assert_eq!(fit, ShaderPipelineFit::Specialized);
        }
    }
    assert_eq!(cache.pipelines.len(), 2);
    assert_eq!(
        builds(&cache),
        (1, 2),
        "one module, one build per forced set"
    );
}

/// Whether `shader` draws now; when it does not, it asks for its pipeline.
fn ready(
    cache: &mut ShaderPipelineCache,
    shader: &RuntimeShader,
    mode: RuntimeShaderPipelineMode,
) -> bool {
    cache.ready_to_draw(shader, shader.draw_specialization(0), mode)
}

/// Builds `shader`'s general pipeline: the unspecialized draw.
fn build_general(
    cache: &mut ShaderPipelineCache,
    shader: &RuntimeShader,
    mode: RuntimeShaderPipelineMode,
) {
    let general = RuntimeShader::new(shader.source());
    cache
        .get_or_create(&general, general.draw_specialization(0), mode)
        .expect("valid general shader");
}

/// The general draws a material several times slower than its own
/// pipeline (a glass on a Mali: ~300 ms frames), so a material waits for
/// its own even when the general is built.
#[test]
fn a_new_material_waits_for_its_own_pipeline_rather_than_draw_with_the_general() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::spawn(wgpu::Backend::Vulkan));
    let shader = material_shader();
    let mode = RuntimeShaderPipelineMode::Replace;
    build_general(&mut cache, &shader, mode);
    assert!(
        !ready(&mut cache, &shader, mode),
        "a built general does not stand in"
    );
    let deadline = Instant::now() + SETTLE;
    while !ready(&mut cache, &shader, mode) {
        assert!(Instant::now() < deadline, "the pipeline never landed");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        draw(&mut cache, &shader, mode),
        ShaderPipelineFit::Specialized,
        "the material draws with its own pipeline"
    );
    assert_eq!(builds(&cache), (1, 2), "the general and the material's own");
}

/// A material of a shader declaring two flags, told apart by `green`.
fn two_flag_material(text: &'static str, green: f64) -> RuntimeShader {
    let mut shader = RuntimeShader::new(text);
    shader.set_override("RED", 0.0);
    shader.set_override("GREEN", green);
    shader.set_specialization_exact(true);
    shader
}

#[test]
fn a_first_screens_shared_stand_in_draws_its_materials_before_their_own() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let compiler = PipelineCompiler::spawn(wgpu::Backend::Vulkan);
    let holds = hold_warm_ups(&compiler);
    let mut cache = cache(&device, compiler);
    let text: &'static str = Box::leak(
        format!(
            "{}\noverride RED: bool = false;\noverride GREEN: bool = false;",
            valid_shader()
        )
        .into_boxed_str(),
    );
    let mode = RuntimeShaderPipelineMode::Replace;
    let materials = [0.0, 1.0].map(|green| two_flag_material(text, green));
    let records: Vec<ShaderPipelineRecord> = materials
        .iter()
        .map(|shader| {
            let specialization = shader.draw_specialization(0);
            ShaderPipelineRecord {
                source: shader.source_hash(),
                overrides: specialization.overrides_hash(),
                mode: mode.disk_byte(),
                constants: specialization
                    .overrides()
                    .iter()
                    .map(|&(name, value)| (name.to_owned(), value))
                    .collect(),
            }
        })
        .collect();
    cache.warm_first_screen(&records, [text]);
    for shader in &materials {
        let (_, fit) = cache
            .get_or_create(shader, shader.draw_specialization(0), mode)
            .expect("valid shader");
        assert_eq!(
            fit,
            ShaderPipelineFit::Fallback,
            "a recorded material stands in with the pipeline the first screen shares"
        );
    }
    assert_eq!(
        builds(&cache),
        (1, 1),
        "one shared pipeline: neither material's own nor the general"
    );
    for shader in &materials {
        let own = cache.key(shader, shader.draw_specialization(0), mode);
        assert!(
            !cache.pipelines.contains_key(&own),
            "a covered material's own pipeline waits for the first frame to be drawn"
        );
    }
    cache.queue_after_first_frame();
    drop(holds);
    let deadline = Instant::now() + SETTLE;
    loop {
        let landed = materials
            .iter()
            .all(|shader| draw(&mut cache, shader, mode) == ShaderPipelineFit::Specialized);
        if landed {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the materials' own pipelines land"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        builds(&cache),
        (1, 3),
        "the shared pipeline and each material's own"
    );
}

#[test]
fn an_override_that_is_not_a_fold_compiles_inside_the_frame() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::spawn(wgpu::Backend::Vulkan));
    let mut shader = material_shader();
    shader.set_specialization_exact(false);
    let fit = draw(&mut cache, &shader, RuntimeShaderPipelineMode::Replace);
    assert_eq!(fit, ShaderPipelineFit::Specialized);
    assert_eq!(
        builds(&cache),
        (1, 1),
        "the requested pipeline itself was built, and nothing else"
    );
}

#[test]
fn a_warmed_general_pipeline_is_ready_before_its_first_draw() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::spawn(wgpu::Backend::Vulkan));
    let shader = RuntimeShader::new(&valid_shader());
    let mode = RuntimeShaderPipelineMode::PremultipliedSrcOver;
    cache.warm(&shader, mode);
    cache.warm(&shader, mode);
    let deadline = Instant::now() + SETTLE;
    while !cache.ready(cache.key(&shader, shader.draw_specialization(0), mode)) {
        assert!(Instant::now() < deadline, "the warm-up never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(builds(&cache), (1, 1), "one warm-up per general pipeline");
    assert_eq!(draw(&mut cache, &shader, mode), ShaderPipelineFit::General);
    assert_eq!(
        builds(&cache),
        (1, 1),
        "the draw found the warm-up's pipeline"
    );
}

#[test]
fn a_drawn_specialization_does_not_wait_for_its_queued_warm_up() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let compiler = PipelineCompiler::spawn(wgpu::Backend::Vulkan);
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    compiler.enqueue(CompileLane::WarmUp, move || {
        let _ = blocked.recv();
    });
    let mut cache = cache(&device, compiler.clone());
    let shader = material_shader();
    let mode = RuntimeShaderPipelineMode::Replace;
    build_general(&mut cache, &shader, mode);
    cache.warm(&shader, mode);
    assert_eq!(
        draw(&mut cache, &shader, mode),
        ShaderPipelineFit::Specialized
    );
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
        "the general and the drawn specialization, which its warm-up found built"
    );
}

#[test]
fn an_invalid_shader_is_validated_once() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut cache = cache(&device, PipelineCompiler::spawn(wgpu::Backend::Vulkan));
    let mut shader = RuntimeShader::new("this is not wgsl");
    shader.set_override("RED", 1.0);
    let mode = RuntimeShaderPipelineMode::Replace;
    assert!(
        cache
            .get_or_create(&shader, shader.draw_specialization(0), mode)
            .is_none()
    );
    assert!(
        cache
            .get_or_create(&shader, shader.draw_specialization(0), mode)
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

// The framework's runtime shaders validate for every backend, and each one's
// listed position independence is what the analysis finds: the web skips
// both for these sources.
#[test]
fn framework_runtime_shaders_validate_as_listed() {
    for (index, &(source, listed)) in FRAMEWORK_RUNTIME_SHADERS.iter().enumerate() {
        for backend in [
            wgpu::Backend::Gl,
            wgpu::Backend::BrowserWebGpu,
            wgpu::Backend::Vulkan,
        ] {
            let (_, position_independent) = validate_runtime_shader_source(source, backend)
                .unwrap_or_else(|error| panic!("shader {index} on {backend:?}: {error}"));
            assert_eq!(
                position_independent, listed,
                "shader {index} on {backend:?}"
            );
        }
    }
}
