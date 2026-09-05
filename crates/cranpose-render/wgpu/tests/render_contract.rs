mod support;

use std::path::{Path, PathBuf};

use cranpose_render_common::{
    Renderer,
    render_contract::{ALL_SHARED_RENDER_CASES, RenderedFrame},
};

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("failed to read source directory") {
        let entry = entry.expect("failed to read source entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn assert_frame_graph_owns_calls(calls: &[&str], entry_point: Option<&str>) {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = crate_dir.join("src");
    let frame_graph = src_dir.join("frame_graph.rs");
    let mut rust_files = Vec::new();
    collect_rust_files(&src_dir, &mut rust_files);
    let violations: Vec<_> = rust_files
        .into_iter()
        .filter(|path| *path != frame_graph && !path.starts_with(src_dir.join("frame_graph")))
        .filter(|path| {
            let source = std::fs::read_to_string(path).expect("renderer source");
            calls.iter().any(|call| source.contains(call))
        })
        .map(|path| {
            path.strip_prefix(crate_dir)
                .unwrap_or(&path)
                .display()
                .to_string()
        })
        .collect();
    assert!(
        violations.is_empty(),
        "{calls:?} must stay inside the frame_graph module: {violations:?}"
    );
    if let Some(entry_point) = entry_point {
        let source = std::fs::read_to_string(frame_graph).expect("frame graph source");
        assert!(
            source.contains(entry_point),
            "missing executor entry point: {entry_point}"
        );
    }
}

#[test]
fn wgpu_command_buffers_are_owned_by_frame_graph_executor() {
    assert_frame_graph_owns_calls(&[".submit(", ".create_command_encoder("], None);
}

#[test]
fn texture_uploads_are_owned_by_frame_graph_executor() {
    assert_frame_graph_owns_calls(&[".write_texture("], Some("pub(crate) fn upload_texture("));
}

#[test]
fn buffer_uploads_are_owned_by_frame_graph_executor() {
    assert_frame_graph_owns_calls(&[".write_buffer("], Some("pub(crate) fn write_buffer("));
}

#[test]
fn frame_graph_executor_does_not_export_submit_or_encoder_creation_helpers() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");

    for helper in ["create_command_encoder", "submit"] {
        assert!(
            !source.contains(&format!("pub(crate) fn {helper}(")),
            "frame graph executor helper `{helper}` must stay private so queue submission cannot leak into helper renderers"
        );
    }
}

#[test]
fn wasm_frame_encoder_is_consumed_on_finish_without_submitted_option_postcondition() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");

    assert!(
        source.contains("pub(crate) fn finish(self) -> FrameGraphExecution"),
        "WASM frame encoder finish must consume the encoder owner instead of leaving a submitted shell behind"
    );
    assert!(
        !source.contains("frame encoder already submitted"),
        "WASM frame encoder access must not rely on an Option postcondition panic"
    );
}

#[test]
fn renderer_warning_state_is_renderer_owned() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");

    assert!(
        !source.contains("static REPORTED_UNSUPPORTED_WGPU_"),
        "WGPU unsupported-feature warning suppression must be owned by the renderer instance"
    );
}

#[test]
fn render_effect_scratch_mismatches_return_errors() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let frame_source = std::fs::read_to_string(crate_dir.join("src/frame.rs"))
        .expect("failed to read WGPU frame executor source");

    assert!(
        effect_source.contains("fn next(&mut self) -> Result<&'target OffscreenTarget, String>"),
        "effect scratch acquisition must report mismatches through the render error path"
    );
    assert!(
        effect_source.contains("fn assert_consumed(&self) -> Result<(), String>"),
        "effect scratch consumption mismatches must report through the render error path"
    );
    assert!(
        effect_source.contains(") -> Result<u32, String>"),
        "effect encoding must be fallible so scratch-plan errors do not panic"
    );
    assert!(
        !effect_source.contains("expect(\"render effect scratch target was not acquired\")"),
        "effect scratch mismatch must not panic"
    );
    assert!(
        frame_source.contains("refs.assert_consumed()"),
        "recorded effect paths must propagate scratch consumption errors"
    );
}

#[test]
fn renderer_defers_offscreen_pool_reuse_until_after_frame_encoding() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");

    assert!(
        render_source.contains("deferred_offscreen_releases: Vec<OffscreenTarget>"),
        "offscreen targets referenced by pending command buffers must be retained until the frame submit boundary"
    );
    assert!(
        render_source.contains("self.flush_deferred_offscreen_releases();"),
        "deferred offscreen targets must be returned to the pool only after render graph execution"
    );
    let flush_start = render_source
        .find("fn flush_deferred_offscreen_releases(")
        .expect("deferred release flush must exist");
    let flush_end = render_source[flush_start..]
        .find("\n    fn ")
        .map(|offset| flush_start + offset)
        .expect("a renderer method must follow the deferred release flush");
    let release_count = render_source.matches(".release_offscreen(").count();
    let flush_release_count = render_source[flush_start..flush_end]
        .matches(".release_offscreen(")
        .count();
    assert_eq!(
        release_count, flush_release_count,
        "renderer code must not return offscreen targets to the pool from encode paths before the pending command buffer is submitted"
    );
}

#[test]
fn effects_use_pass_owned_uploads_for_uniforms() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let frame_graph_source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");

    assert!(
        frame_graph_source.contains("struct FrameUploadAllocators"),
        "effect upload slots must be owned by the frame graph executor"
    );
    assert!(
        frame_graph_source.contains("fn upload_uniform(")
            && frame_graph_source.contains("fn upload_buffer("),
        "frame command recorders must expose upload operations without leaking upload allocator ownership"
    );
    assert!(
        effect_source.contains("C: FrameCommandRecorder"),
        "effect encoders must receive the caller-owned frame command recorder"
    );
    assert!(
        !effect_source.contains("queue: &wgpu::Queue")
            && !effect_source.contains("uploads: &mut FrameUploadAllocators")
            && !effect_source.contains("FrameUploadAllocators"),
        "effect encoders must not receive raw queue or upload allocator handles"
    );
    assert!(
        !frame_graph_source.contains("fn command_parts("),
        "frame command recorders must not expose raw encoder/upload allocator pairs"
    );
    assert!(
        !effect_source.contains("UploadAllocator::uniform("),
        "effect renderer must not own retained upload allocators"
    );
    assert!(
        effect_source.contains("pub(crate) fn encode_effect"),
        "effect chains must have an encode-only entry point"
    );
    assert!(
        !effect_source.contains("write_buffer_at_zero_offset"),
        "effect renderer must not update fixed per-effect buffers before pending passes submit"
    );
    for fixed_buffer in [
        "blur_uniform_buffer_horizontal",
        "blur_uniform_buffer_vertical",
        "offset_uniform_buffer",
        "blit_uniform_buffer",
        "projective_blit_uniform_buffer",
        "projective_blit_vertex_buffer",
        "effect_uniform_buffer",
    ] {
        assert!(
            !effect_source.contains(fixed_buffer),
            "effect renderer must not retain fixed upload target `{fixed_buffer}`"
        );
    }
}

#[test]
fn effect_offscreen_pool_is_owned_by_effect_renderer_api() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");

    assert!(
        !effect_source.contains("pub offscreen_pool:"),
        "EffectRenderer must not expose retained offscreen pool ownership as a field"
    );
    assert!(
        effect_source.contains("pub(crate) fn acquire_offscreen(")
            && effect_source.contains("pub(crate) fn release_offscreen(")
            && effect_source.contains("pub(crate) fn retained_offscreen_bytes("),
        "EffectRenderer must expose explicit retained-surface operations for GpuRenderer"
    );
    assert!(
        !render_source.contains(".offscreen_pool."),
        "GpuRenderer must acquire/release retained effect surfaces through EffectRenderer methods"
    );
}

#[test]
fn effect_renderer_paths_are_encode_only() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");

    assert!(
        !effect_source.contains("execute_recorded_graph(")
            && !effect_source.contains("WgpuFrameGraphExecutor"),
        "effect renderer code must not own command submission; it only encodes into caller-owned frame commands"
    );
    assert!(
        !effect_source.contains(".execute_with_context_stats("),
        "effect renderer paths must declare graph resources instead of hiding behind the single-pass context helper"
    );
    assert!(
        !effect_source.contains("WgpuFrameGraphExecutor::execute_graph("),
        "effect renderer must not create one-shot recorded graph executors"
    );
    assert!(
        !effect_source.contains("WgpuFrameGraphExecutor::create_command_encoder("),
        "effect renderer must not create command encoders directly"
    );
    assert!(
        !effect_source.contains("WgpuFrameGraphExecutor::submit("),
        "effect renderer must not submit command buffers directly"
    );
}

#[test]
fn screenshot_readback_copy_is_explicit_command_pass() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let start = render_source
        .find("pub fn render_to_rgba_pixels(")
        .expect("screenshot/readback render path must exist");
    let end = render_source[start..]
        .find("fn convert_surface_pixels_to_rgba")
        .map(|offset| start + offset)
        .expect("pixel conversion helper must follow screenshot capture");
    let body = &render_source[start..end];

    assert!(
        body.contains("let mut graph = WgpuFrameGraph::new(Some(\"Screenshot Copy Encoder\"));")
            && body.contains("screenshot-copy-source")
            && body.contains("graph.add_fallible_command_pass(")
            && body.contains("executor.execute_recorded_graph(&device, &queue, graph)")
            && body.contains("let execution = execution.map_err(|error| error.to_string())?;")
            && body.contains("let submission_index = execution.submission;")
            && body.contains("let copy_stats = execution.stats;"),
        "screenshot readback must be an explicit recorded command pass with counted submit stats"
    );
    assert!(
        !render_source.contains("execute_renderer_pass_with_submission_stats"),
        "readback copy should not keep a renderer-local single-pass context wrapper"
    );
}

#[test]
fn cached_text_glyph_runs_recover_missing_gpu_atlas_entries() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let common_source = std::fs::read_to_string(
        crate_dir
            .join("../common/src/software_text_raster.rs")
            .canonicalize()
            .expect("failed to resolve common text raster source"),
    )
    .expect("failed to read common text raster source");

    assert!(
        common_source.contains("pub fn atlas_glyph_for_placement("),
        "software glyph cache must expose retained mask recovery for placement-only cached runs"
    );
    assert!(
        render_source.contains("fn glyph_atlas_entry_for_placement(")
            && render_source
                .contains("self.text_glyph_mask_cache.atlas_glyph_for_placement(glyph)")
            && render_source.contains("self.glyph_atlas_entry_for(&upload_glyph)"),
        "WGPU text glyph cache hits with missing atlas entries must upload from retained masks instead of falling back to text images"
    );
    assert!(
        !render_source.contains("let Some(entry) = self.glyph_atlas_entry_for_cached(glyph) else"),
        "cached text glyph runs must not bail out when the GPU atlas entry is absent"
    );
}

#[test]
fn cached_visible_text_glyph_runs_promote_large_runs_to_retained_buffers() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");

    assert!(
        render_source.contains("fn emit_retained_text_glyph_run_if_ready("),
        "cached visible text rendering should promote large paragraph/code runs through a single retained-buffer helper"
    );
    assert!(
        render_source.contains("!self.retained_text_glyph_run_ready(cache_key)")
            && render_source.contains("!self.ensure_retained_text_glyph_run(cache_key, quads)"),
        "retained text promotion must reuse ready GPU runs and create missing runs through the retained helper"
    );
    assert!(
        render_source
            .matches("emit_retained_text_glyph_run_if_ready(")
            .count()
            >= 2,
        "the visible cached glyph path should use retained-buffer promotion"
    );
    let cached_branch_start = render_source
        .find("if let Some(quad_run) = cached_quad_run.as_ref()")
        .expect("cached visible glyph branch exists");
    let cached_branch_end = render_source[cached_branch_start..]
        .find("let index_start = image_indices.len() as u32;")
        .map(|offset| cached_branch_start + offset)
        .expect("cached visible glyph branch boundary exists");
    assert!(
        render_source[cached_branch_start..cached_branch_end]
            .contains("self.emit_retained_text_glyph_run_if_ready("),
        "cached visible glyph runs should use retained-buffer promotion"
    );
    let miss_branch_start = render_source
        .find("let Ok(quad_run) = self.prepare_text_glyph_quads(")
        .expect("visible miss glyph preparation branch exists");
    let miss_branch_end = render_source[miss_branch_start..]
        .find("let index_count = image_indices.len() as u32 - index_start;")
        .map(|offset| miss_branch_start + offset)
        .expect("visible miss glyph preparation branch boundary exists");
    assert!(
        !render_source[miss_branch_start..miss_branch_end]
            .contains("emit_retained_text_glyph_run_if_ready"),
        "newly prepared visible misses must not synchronously create retained buffers in the slow frame"
    );
}

#[test]
fn frame_graph_executor_runs_recorded_pass_nodes() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frame_graph_source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");

    for required in [
        "struct FrameCommandStats",
        "struct FrameGraphExecution",
        "enum FrameGraphError",
        "struct WgpuFrameGraphExecutor",
        "struct TransientTexturePool",
        "struct WgpuFrameGraph",
        "struct ResourceGraph",
        "struct TextureHandle",
        "struct FrameTextureDescriptor",
        "enum PassNode",
        "struct CommandPassNode",
        "struct PassContext",
        "fn add_fallible_command_pass(",
        "fn add_fallible_recorded_command_pass(",
        "fn import_surface(",
        "fn execute_recorded_graph(",
        "fn build_pass_schedule(",
        "trait FrameCommandRecorder",
        "impl FrameCommandRecorder for PassContext<'_>",
    ] {
        assert!(
            frame_graph_source.contains(required),
            "frame graph source must define `{required}`"
        );
    }
    assert!(
        frame_graph_source.contains("Self::Command(pass)")
            && frame_graph_source.contains("(pass.encode)(context).map_err"),
        "recorded graph execution must run explicit command pass nodes"
    );
    assert!(
        frame_graph_source.contains("-> Result<FrameGraphExecution, FrameGraphError>")
            && frame_graph_source.contains("FrameGraphError::EmptyGraph")
            && frame_graph_source.contains("FrameGraphError::NoDeclaredPasses")
            && frame_graph_source.contains("FrameGraphError::ScheduledPassTwice")
            && frame_graph_source.contains("FrameGraphError::CyclicPassDependencies"),
        "recorded graph execution must report scheduler and declaration failures through the frame graph error path"
    );
    assert!(
        !frame_graph_source.contains("execute_with_context_submission_stats"),
        "generic single-pass context submission must not hide command/copy paths"
    );
    assert!(
        !frame_graph_source.contains("fn execute_graph("),
        "recorded graph execution must require an executor instance so retained resources cannot be hidden behind a temporary executor"
    );
    assert!(
        !frame_graph_source.contains("execute_with_submission_and_stats"),
        "explicit submission helpers must use the retained executor instance instead of a temporary executor"
    );
    assert!(
        frame_graph_source.contains("let mut pass_count = 0u32;")
            && frame_graph_source.contains("fn encode_pass_node(")
            && frame_graph_source
                .contains("let recorded_pass_count = pass.encode(pass_index, &mut context)?;")
            && frame_graph_source
                .contains("pass_count = pass_count.saturating_add(recorded_pass_count);"),
        "graph execution must expose pass counts at the executor boundary"
    );
    assert!(
        frame_graph_source.contains("fn record_passes(&mut self, count: u32)")
            && frame_graph_source.contains("fn recorded_pass_count(&self) -> u32"),
        "explicit frame command recorder sessions must expose pass accounting"
    );
    assert!(
        frame_graph_source.contains("transient_texture_bytes"),
        "graph execution must expose transient resource bytes at the executor boundary"
    );
    assert!(
        frame_graph_source.contains("retained_texture_bytes: self.retained_texture_bytes(),")
            || frame_graph_source.contains("retained_texture_bytes,"),
        "graph execution must expose retained transient-pool bytes through command stats"
    );
    assert!(
        frame_graph_source.contains("transient_textures: TransientTexturePool"),
        "recorded graph execution must retain transient texture storage on the executor"
    );
    assert!(
        frame_graph_source.contains("release_pending_transients(&mut self.transient_textures"),
        "recorded command sessions must return transient textures to the executor pool after submit"
    );
    assert!(
        frame_graph_source.contains("last_access")
            && frame_graph_source.contains("add_pass_dependency"),
        "pass scheduling must preserve write-after-read hazards, not only read-after-write hazards"
    );
    assert!(
        !frame_graph_source.contains("queue: &'pass wgpu::Queue"),
        "recorded pass contexts must not expose the queue"
    );
    assert!(
        frame_graph_source.contains("fn finish(self) -> FrameGraphExecution"),
        "explicit frame encoder sessions must consume the encoder owner and report execution stats"
    );
    assert!(
        frame_graph_source.contains("pub(crate) fn record_passes(&mut self, count: u32)"),
        "explicit frame encoder sessions must account for recorded render passes"
    );
    assert!(
        frame_graph_source.contains("let pass_count = self.pass_count;")
            && frame_graph_source
                .contains("let transient_texture_bytes = self.transient_texture_bytes;"),
        "explicit frame encoder sessions must transfer pass and resource counts at submission"
    );

    let gpu_stats_source = std::fs::read_to_string(crate_dir.join("src/gpu_stats.rs"))
        .expect("failed to read WGPU stats source");
    assert!(
        !gpu_stats_source.contains("fn bump_submits("),
        "renderer submit accounting must consume executor-returned FrameCommandStats"
    );
}

#[test]
fn gpu_stats_env_flag_is_not_process_cached() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let gpu_stats_source = std::fs::read_to_string(crate_dir.join("src/gpu_stats.rs"))
        .expect("failed to read WGPU stats source");

    assert!(
        !gpu_stats_source.contains("OnceLock") && !gpu_stats_source.contains("static ENABLED"),
        "GPU stats env flag must not be latched in a process-global cache"
    );

    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    assert!(
        !render_source.contains("gpu_stats_enabled:"),
        "GPU stats env flag must not be latched in a renderer field either — \
         read gpu_stats_enabled() at the use site"
    );
}

#[test]
fn frame_graph_pass_errors_abort_before_submit() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frame_graph_source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");

    assert!(
        frame_graph_source.contains("PassFailed")
            && frame_graph_source.contains("type PassEncodeResult = Result<(), String>")
            && frame_graph_source.contains("fn add_fallible_recorded_command_pass("),
        "frame graph command nodes must carry fallible encode results through the executor"
    );
    assert!(
        frame_graph_source.contains("let recorded_pass_count = pass.encode(pass_index, &mut context)?;")
            && frame_graph_source.contains("Err(error) =>")
            && frame_graph_source.contains("return Err(error);")
            && frame_graph_source.contains(
                "release_pending_transients(&mut self.transient_textures, pending_transient_releases);"
            ),
        "the executor must release transient resources and return pass errors before queue submission"
    );
    assert!(
        !render_source.contains("let mut render_result = None"),
        "native renderer errors must not be smuggled through an out-of-band pass result slot"
    );
    assert!(
        render_source.contains("frame_graph.add_fallible_recorded_command_pass("),
        "native renderer frame recording must use the executor-owned fallible pass boundary"
    );
}

#[test]
fn text_rendering_uses_cached_raster_image_batches() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");

    assert!(
        render_source
            .contains("text_image_cache: BoundedLruCache<TextImageCacheKey, CachedTextImage>")
            && render_source.contains("fn append_text_image_draw_cmds"),
        "WGPU text rendering must use retained raster image batches"
    );
    assert!(
        !render_source.contains("struct TextRendererSlot")
            && !render_source.contains("TextRenderer::new")
            && !render_source.contains("TextAtlas::new"),
        "WGPU render.rs must not carry a second glyph atlas text renderer"
    );
    assert!(
        !render_source.contains("text_viewport: Viewport"),
        "GpuRenderer must not keep a process-wide text viewport for every text pass"
    );
    assert!(
        !render_source.contains("struct EncoderBufferUsage")
            && !render_source.contains("enum BatchKind"),
        "wasm segment batching must not split draw chunks by repeated batch kind"
    );
    assert!(
        render_source.contains("run_store: RunStore")
            && render_source.contains("vertices: BufferUpload")
            && render_source.contains("indices: BufferUpload")
            && render_source.contains("viewport_uniforms: ViewportUniformRing"),
        "draw batches must own retained runs and frame upload ranges"
    );
}

#[test]
fn wgpu_text_system_uses_one_shared_state_for_measure_and_render() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = format!(
        "{}{}",
        std::fs::read_to_string(crate_dir.join("src/lib.rs")).expect("failed to read lib.rs"),
        std::fs::read_to_string(crate_dir.join("src/frontend.rs"))
            .expect("failed to read frontend.rs"),
    );
    let cargo_toml =
        std::fs::read_to_string(crate_dir.join("Cargo.toml")).expect("failed to read Cargo.toml");

    assert!(
        source.contains("pub struct WgpuTextSystem {\n    software_fonts: SoftwareTextFontSet,")
            && source.contains("text_state: TextSystemState,")
            && source.contains("app_context: Option<Weak<cranpose_ui::AppContext>>,")
            && source.contains("measurer: SoftwareTextMeasurer,"),
        "WGPU text system must attach render-time text layout to the per-app text context"
    );
    let removed_text_backend = ["gly", "phon"].concat();
    let removed_font_system = ["Font", "System"].concat();
    let removed_shared_state = ["Shared", "Text", "System", "State"].concat();
    let removed_measurer = ["Wgpu", "Text", "Measurer"].concat();
    let removed_cache_key = ["Text", "Cache", "Key"].concat();
    let removed_buffer = ["Shared", "Text", "Buffer"].concat();
    assert!(
        !cargo_toml.contains(removed_text_backend.as_str())
            && !source.contains(removed_font_system.as_str())
            && !source.contains(removed_shared_state.as_str())
            && !source.contains(removed_measurer.as_str())
            && !source.contains(removed_cache_key.as_str())
            && !source.contains(removed_buffer.as_str()),
        "WGPU must not carry a second renderer-owned glyph shaping subsystem"
    );
    assert!(
        !source.contains("render_text_state")
            && !source.contains("measure_text_state")
            && !source.contains("cranpose_ui::text::set_text_measurer"),
        "WGPU renderer must not split render and measure text caches"
    );
    assert!(
        source.contains("SoftwareTextMeasurer::from_font_set(")
            && source.contains("self.frontend.text_fonts.clone()"),
        "AppContext text measurer should use the same software font set as WGPU raster text rendering"
    );
    assert!(
        source.contains("cranpose_ui::has_current_app_context()")
            && source.contains("cranpose_ui::text::layout_text(text, style)")
            && source.contains("app_context.enter(|| {")
            && source.contains("gpu_renderer.render("),
        "WGPU render text layout should route through the attached AppContext text service"
    );
}

#[test]
fn wgpu_renderer_matches_shared_render_contracts() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping shared render contract assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    for case in ALL_SHARED_RENDER_CASES {
        let mut frames = Vec::new();
        for fixture in case.fixtures() {
            renderer.scene_mut().graph = Some(fixture.graph);
            let frame = renderer
                .capture_frame(fixture.width, fixture.height)
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to capture shared render case {}: {err:?}",
                        case.name()
                    )
                });
            frames.push(RenderedFrame {
                width: frame.width,
                height: frame.height,
                pixels: frame.pixels,
                normalized_rect: fixture.normalized_rect,
            });
        }
        case.assert_frames(&frames);
    }
}

#[test]
fn wgpu_renderer_matches_shared_stroke_and_arc_contracts() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping shared stroke/arc contract assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    for case in [
        cranpose_render_common::render_contract::SharedRenderCase::StrokedRoundRect,
        cranpose_render_common::render_contract::SharedRenderCase::AnnularSector,
    ] {
        let mut frames = Vec::new();
        for fixture in case.fixtures() {
            renderer.scene_mut().graph = Some(fixture.graph);
            let frame = renderer
                .capture_frame(fixture.width, fixture.height)
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to capture shared render case {}: {err:?}",
                        case.name()
                    )
                });
            frames.push(RenderedFrame {
                width: frame.width,
                height: frame.height,
                pixels: frame.pixels,
                normalized_rect: fixture.normalized_rect,
            });
        }
        case.assert_frames(&frames);
    }
}
