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

#[test]
fn wgpu_command_buffers_are_owned_by_frame_graph_executor() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = crate_dir.join("src");
    let frame_graph = src_dir.join("frame_graph.rs");
    let mut rust_files = Vec::new();
    collect_rust_files(&src_dir, &mut rust_files);

    let mut violations = Vec::new();
    for path in rust_files {
        if path == frame_graph {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("failed to read renderer source");
        if source.contains(".submit(") || source.contains(".create_command_encoder(") {
            violations.push(
                path.strip_prefix(crate_dir)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }

    assert!(
        violations.is_empty(),
        "WGPU command encoder creation and queue submission must stay inside frame_graph.rs: {violations:?}"
    );
}

#[test]
fn texture_uploads_are_owned_by_frame_graph_executor() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = crate_dir.join("src");
    let frame_graph = src_dir.join("frame_graph.rs");
    let mut rust_files = Vec::new();
    collect_rust_files(&src_dir, &mut rust_files);

    let mut violations = Vec::new();
    for path in rust_files {
        if path == frame_graph {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("failed to read renderer source");
        if source.contains(".write_texture(") {
            violations.push(
                path.strip_prefix(crate_dir)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }

    let frame_graph_source =
        std::fs::read_to_string(frame_graph).expect("failed to read WGPU frame graph source");
    assert!(
        frame_graph_source.contains("pub(crate) fn upload_texture("),
        "retained texture uploads must enter through an executor-owned API"
    );
    assert!(
        violations.is_empty(),
        "WGPU texture queue uploads must stay inside frame_graph.rs: {violations:?}"
    );
}

#[test]
fn buffer_uploads_are_owned_by_frame_graph_executor() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = crate_dir.join("src");
    let frame_graph = src_dir.join("frame_graph.rs");
    let mut rust_files = Vec::new();
    collect_rust_files(&src_dir, &mut rust_files);

    let mut violations = Vec::new();
    for path in rust_files {
        if path == frame_graph {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("failed to read renderer source");
        if source.contains(".write_buffer(") {
            violations.push(
                path.strip_prefix(crate_dir)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }

    let frame_graph_source =
        std::fs::read_to_string(frame_graph).expect("failed to read WGPU frame graph source");
    assert!(
        frame_graph_source.contains("pub(crate) fn upload_buffer("),
        "buffer uploads must enter through an executor-owned API"
    );
    assert!(
        violations.is_empty(),
        "WGPU buffer queue uploads must stay inside frame_graph.rs: {violations:?}"
    );
}

#[test]
fn frame_upload_slots_grow_instead_of_panicking_on_payload_size() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frame_graph_source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");

    assert!(
        !frame_graph_source.contains("upload payload exceeds allocator slot size"),
        "frame upload allocation must not panic on a larger runtime payload"
    );
    assert!(
        frame_graph_source.contains("fn required_slot_size(")
            && frame_graph_source.contains("if self.slots[index].size < required_size")
            && frame_graph_source
                .contains("self.slots[index] = self.create_slot(device, required_size);"),
        "frame upload allocation must grow undersized retained slots before writing data"
    );
    assert!(
        frame_graph_source.contains("fn should_retain_slot_size(")
            && frame_graph_source.contains(".retain(|slot| Self::should_retain_slot_size"),
        "frame upload allocation must not retain oversized one-frame upload slots forever"
    );
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
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");

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
        render_source.contains("effect_scratch_refs.assert_consumed()?"),
        "recorded effect paths must propagate scratch consumption errors"
    );
}

#[test]
fn blurred_shadow_path_encodes_blur_and_composite_before_submit() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let start = render_source
        .find("fn encode_shadow_draw<")
        .expect("shadow draw encoder must exist");
    let end = render_source[start..]
        .find("fn encode_shadow_shape_source_passes<")
        .map(|offset| start + offset)
        .expect("shadow source encoder helper must exist");
    let body = &render_source[start..end];

    assert!(
        body.contains(".encode_blur_scissored_ping_pong_passes("),
        "blurred shadows must record blur passes into the frame encoder"
    );
    assert!(
        body.contains(".encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode("),
        "blurred shadows must record composite passes into the frame encoder"
    );
    assert!(
        !body.contains(".apply_blur_scissored("),
        "blurred shadows must not submit through the immediate blur helper"
    );
    assert!(
        !body.contains(".composite_to_view_scissored_with_alpha_and_mask_and_blend_mode("),
        "blurred shadows must not submit through the immediate composite helper"
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
    let release_count = render_source.matches(".release_offscreen(").count();
    assert_eq!(
        release_count, 1,
        "renderer code must not return offscreen targets to the pool from encode paths before the pending command buffer is submitted"
    );
}

#[test]
fn retained_layer_surfaces_have_dedicated_cache_owner() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let cache_source = std::fs::read_to_string(crate_dir.join("src/layer_surface_cache.rs"))
        .expect("failed to read WGPU layer surface cache source");

    assert!(
        render_source.contains("layer_surface_cache: LayerSurfaceCache"),
        "GpuRenderer must depend on a retained layer-surface cache boundary, not own layer-cache maps directly"
    );
    for forbidden in [
        "BoundedLruCache<LayerRasterCacheKey, CachedLayerSurface>",
        "layer_surface_cache_identity:",
        "layer_surface_cache_bytes:",
        "layer_cache_seen_this_frame:",
    ] {
        assert!(
            !render_source.contains(forbidden),
            "GpuRenderer must not own retained layer cache internals: `{forbidden}`"
        );
    }
    for required in [
        "struct LayerSurfaceCache",
        "entries: BoundedLruCache<LayerRasterCacheKey, CachedLayerSurface>",
        "identity: HashMap<LayerRasterCacheIdentity, LayerRasterCacheKey>",
        "seen_this_frame: HashSet<usize>",
        "fn finish_frame(",
    ] {
        assert!(
            cache_source.contains(required),
            "LayerSurfaceCache must own retained layer cache state and per-frame bookkeeping: `{required}`"
        );
    }
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
            && frame_graph_source.contains("fn upload_vertex("),
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
fn render_effect_scratch_is_owned_by_frame_recorder() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let start = effect_source
        .find("pub(crate) fn encode_effect")
        .expect("encode_effect must exist");
    let end = effect_source[start..]
        .find("pub(crate) fn encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode")
        .map(|offset| start + offset)
        .expect("composite encoder must follow encode_effect");
    let body = &effect_source[start..end];

    assert!(
        effect_source.contains("struct RecordedEffectScratchTargets")
            && effect_source.contains("fn acquire_recorded_effect_scratch_textures_into")
            && effect_source.contains("\"Render Effect Blur Scratch\"")
            && effect_source.contains("\"Render Effect Chain Scratch\""),
        "effect scratch textures must be acquired as frame-recorder transient resources"
    );
    assert!(
        body.contains("scratch_targets.next()?"),
        "effect encoding must consume graph-allocated scratch targets"
    );
    assert!(
        !body.contains("offscreen_pool.acquire("),
        "effect encoding must not allocate hidden offscreen scratch during recorded pass execution"
    );
    assert!(
        render_source.contains("acquire_recorded_effect_scratch_targets(")
            && render_source
                .contains("let mut effect_scratch_refs = effect_scratch_targets.refs();")
            && render_source.contains("effect_scratch_refs.assert_consumed()?;")
            && render_source.contains("effect_scratch_targets.release_into(self.recorder);"),
        "combined effect composite paths must wire declared scratch targets through frame recording"
    );
}

#[test]
fn axis_aligned_composite_has_encode_only_entry_point() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let start = effect_source
        .find("pub(crate) fn encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode")
        .expect("axis-aligned composite encoder must exist");
    let body = &effect_source[start..];

    assert!(
        body.contains("self.encode_composite_to_view_pass("),
        "axis-aligned composites must delegate to the shared composite encoder"
    );
    assert!(
        !effect_source
            .contains("pub fn composite_to_view_scissored_with_alpha_and_mask_and_blend_mode("),
        "axis-aligned composites must not expose an operation-local submit wrapper"
    );
}

#[test]
fn layer_event_clear_records_into_frame_recorder() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let start = render_source
        .find("fn clear_target_view_with_load_op(")
        .expect("clear helper must exist");
    let end = render_source[start..]
        .find("fn prepare_shapes_batch")
        .map(|offset| start + offset)
        .expect("next renderer helper must exist");
    let body = &render_source[start..end];

    assert!(
        body.contains(".recorder")
            && body.contains(".encoder()")
            && body.contains("Layer Event Clear Pass")
            && body.contains("self.recorder.record_pass();"),
        "layer-event clears must be recorded into the caller-owned frame command recorder"
    );
    assert!(
        !render_source.contains("fn execute_renderer_pass_with_stats"),
        "renderer clear paths should not keep the single-pass context wrapper alive"
    );
}

#[test]
fn first_child_composite_consumes_pending_clear_load_op() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
        .expect("failed to read WGPU surface executor source");
    fn block_after<'a>(source: &'a str, marker: &str) -> &'a str {
        let start = source
            .find(marker)
            .unwrap_or_else(|| panic!("missing source marker: {marker}"));
        let body_start = source[start..]
            .find('{')
            .map(|offset| start + offset + 1)
            .unwrap_or_else(|| panic!("missing block body for marker: {marker}"));
        let body_end = source[body_start..]
            .find('}')
            .map(|offset| body_start + offset)
            .unwrap_or_else(|| panic!("missing block end for marker: {marker}"));
        &source[body_start..body_end]
    }

    assert!(
        source.contains("fn flush_pending_clear<"),
        "surface execution must keep explicit clears at target-read boundaries only"
    );
    assert!(
        source.contains("let composite_load_op = next_load_op;")
            && source.contains("dest_quad,")
            && source.contains("composite_load_op,"),
        "first child surface composites should consume the pending clear load-op instead of forcing a standalone clear submit"
    );
    let nested_underlay_start = source
        .find(
            "if child.needs_nested_underlay {\n            flush_pending_shader_layer_composites(\n                backend,\n                &mut pending_shader_composites,\n                &target.view,",
        )
        .expect("nested underlay branch must initialize target before child capture");
    let nested_underlay_end = source[nested_underlay_start..]
        .find("let child_underlay = child.needs_nested_underlay.then")
        .map(|offset| nested_underlay_start + offset)
        .expect("nested underlay capture should follow target initialization");
    let nested_underlay_body = &source[nested_underlay_start..nested_underlay_end];
    let backdrop_body = block_after(&source, "if let Some(backdrop) = &child_surface.backdrop");
    let shadow_body = block_after(&source, "if !resolved_child.shadow_draws.is_empty()");
    assert!(
        nested_underlay_body.contains("flush_pending_clear")
            && backdrop_body.contains("flush_pending_queues_for_backdrop_capture")
            && shadow_body.contains("flush_pending_clear"),
        "target readers such as underlays, backdrop snapshots, and shadow draws must still initialize the target before reading/compositing"
    );
    let capture_flush_start = source
        .find("fn flush_pending_queues_for_backdrop_capture")
        .expect("backdrop captures must go through the shared dependency-gated flush");
    let capture_flush_end = source[capture_flush_start..]
        .find("\npub(crate) fn ")
        .or_else(|| source[capture_flush_start..].find("\nfn "))
        .map(|offset| capture_flush_start + offset)
        .expect("the capture flush helper must be followed by another item");
    let capture_flush_body = &source[capture_flush_start..capture_flush_end];
    assert!(
        capture_flush_body.contains("flush_pending_clear"),
        "the dependency-gated capture flush must still initialize the target before the snapshot copy"
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
fn projective_composite_has_encode_only_entry_point() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let start = effect_source
        .find("pub(crate) fn encode_composite_to_view_projective")
        .expect("projective composite encoder must exist");
    let body = &effect_source[start..];

    assert!(
        effect_source.contains("pub(crate) fn encode_composite_to_view_projective"),
        "projective composites must expose an encode-only path for frame-recorder layer composition"
    );
    assert!(
        body.contains(".encoder()")
            && body.contains("begin_render_pass")
            && body.contains("pass.draw(0..4, 0..1);"),
        "projective composites must encode a WGPU pass into the caller-owned command encoder"
    );
    assert!(
        !effect_source.contains("pub fn composite_to_view_projective("),
        "projective composites must not expose an operation-local submit wrapper"
    );
    assert!(
        !body.contains(".execute_with_context_stats("),
        "projective composites must not hide behind the single-pass context helper"
    );
}

#[test]
fn effect_layer_axis_aligned_composite_records_effect_and_composite_together() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let surface_executor_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
            .expect("failed to read surface executor source");
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let surface_backend_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/backend.rs"))
            .expect("failed to read surface backend source");

    assert!(
        surface_executor_source.contains("backend.apply_effect_and_composite_to_view("),
        "axis-aligned render-effect layer paths must record effect and final composite together"
    );
    assert!(
        render_source.contains("fn record_effect_composite("),
        "recording surface backend must expose a combined effect+composite recording path"
    );
    assert!(
        render_source.contains("\"Render Effect Composite Scratch\"")
            && render_source.contains("acquire_recorded_effect_scratch_targets(")
            && render_source.contains("encode_effect(")
            && render_source
                .contains("encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(")
            && render_source
                .contains("self.recorder.record_passes(effect_passes.saturating_add(1));"),
        "combined render-effect composite path must record effect and final composite into the frame recorder"
    );
    assert!(
        !effect_source.contains("EffectCompositeRequest")
            && !effect_source.contains("ShaderCompositeRequest"),
        "effect renderer must not keep request wrappers for operation-local submissions"
    );
    assert!(
        render_source.contains("fn record_shader_composite(")
            && render_source.contains("direct_shader_composite_viewport(")
            && render_source.contains("encode_shader_src_over_to_view(")
            && render_source.contains("fn record_effect_with_direct_shader_tail_composite(")
            && render_source.contains("direct_shader_tail_composite(")
            && render_source.contains("\"Shader Effect Composite Scratch\""),
        "axis-aligned alpha-1 shader composites and shader-tail effect chains must record direct blended shader passes and keep scratch only as fallback"
    );
    assert!(
        !surface_backend_source.contains("intermediate: &OffscreenTarget"),
        "axis-aligned combined effect/shader composite APIs must not require caller-owned intermediate targets"
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
        render_source.contains("if draw_action == TextGlyphDrawAction::PrewarmOffscreen {")
            && render_source
                .contains("self.ensure_retained_text_glyph_run(run_key, prewarm_quads.as_ref());"),
        "offscreen prewarm remains the retained-buffer creation path"
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
fn native_fused_segments_prewarm_offscreen_text_runs_explicitly() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");

    assert!(
        render_source.contains("fn prewarm_offscreen_text_glyph_draws_in_chunk("),
        "native fused segment rendering needs an explicit offscreen text prewarm boundary"
    );
    assert!(
        render_source.contains("self.prewarm_offscreen_text_glyph_draws_in_chunk("),
        "fused segments should prewarm near-viewport text before visible draw batching"
    );
    assert!(
        render_source.contains("enum TextGlyphPrewarmDecision")
            && render_source.contains("fn text_glyph_prewarm_decision(")
            && render_source.contains("text_draw_should_prewarm_in_viewport(")
            && render_source.contains("!text_draw_is_visible_in_viewport("),
        "prewarm candidates must be near the viewport but not visible draw work"
    );
    assert!(
        render_source.contains("self.append_text_glyph_draws(")
            && render_source.contains("std::iter::once(text_draw),")
            && render_source.contains("staged_uploads.truncate("),
        "offscreen prewarm should reuse the glyph path without leaking draw commands or staged visible uploads"
    );
}

#[test]
fn transformed_shader_layer_records_shader_and_projective_composite_together() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let surface_executor_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
            .expect("failed to read surface executor source");
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let surface_backend_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/backend.rs"))
            .expect("failed to read surface backend source");

    assert!(
        surface_backend_source.contains("fn apply_shader_and_composite_to_view_projective("),
        "surface backends must expose a combined transformed shader composite path"
    );
    assert!(
        surface_executor_source.contains("backend.apply_shader_and_composite_to_view_projective("),
        "transformed deferred shader layers must not allocate a caller-owned intermediate before projective composite"
    );
    assert!(
        !surface_executor_source
            .contains("let intermediate = backend.acquire_offscreen(source.width, source.height);"),
        "transformed deferred shader layers must not split shader and projective composite into separate submissions"
    );
    assert!(
        render_source.contains("fn record_shader_projective_composite(")
            && render_source.contains("\"Shader Projective Composite Scratch\"")
            && render_source.contains("self.recorder.record_pass();")
            && render_source.contains("self.renderer.effect_renderer.record_composite_pass();"),
        "transformed shader composites must record shader and projective composite passes through the frame recorder"
    );
    assert!(
        !effect_source.contains("ShaderProjectiveCompositeRequest")
            && !effect_source.contains("pub(crate) fn apply_shader_and_composite_to_view_projective(")
            && effect_source.contains("self.encode_shader(")
            && render_source.contains("self.renderer\n                .effect_renderer\n                .encode_composite_to_view_projective("),
        "effect renderer must stay encode-only while recording surface backend combines transformed shader compositing"
    );
}

#[test]
fn transformed_effect_layer_records_effect_and_projective_composite_together() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let surface_executor_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
            .expect("failed to read surface executor source");
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let surface_backend_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/backend.rs"))
            .expect("failed to read surface backend source");
    let start = surface_executor_source
        .find("pub(crate) fn render_effect_layer_to_target")
        .expect("effect layer renderer must exist");
    let end = surface_executor_source[start..]
        .find("pub(crate) fn apply_backdrop_layer_to_target")
        .map(|offset| start + offset)
        .expect("backdrop layer renderer must follow effect layer renderer");
    let body = &surface_executor_source[start..end];

    assert!(
        surface_backend_source.contains("fn apply_effect_and_composite_to_view_projective("),
        "surface backends must expose a combined transformed effect composite path"
    );
    assert!(
        body.contains("backend.apply_effect_and_composite_to_view_projective("),
        "transformed render-effect layers must record effect and projective composite together"
    );
    assert!(
        !body.contains("let effect_dest = backend.acquire_offscreen(effect_width, effect_height);")
            && !body.contains(
                "let fallback_dest = backend.acquire_offscreen(effect_width, effect_height);"
            ),
        "transformed render-effect layers must not allocate caller-owned intermediate targets"
    );
    assert!(
        render_source.contains("fn record_effect_projective_composite(")
            && render_source.contains("\"Render Effect Projective Composite Scratch\"")
            && render_source.contains("acquire_recorded_effect_scratch_targets(")
            && render_source.contains("effect_scratch_targets.release_into(self.recorder);"),
        "transformed effect composites must allocate scratch through the frame recorder and release it at the recording boundary"
    );
    assert!(
        !effect_source.contains("EffectProjectiveCompositeRequest")
            && !effect_source.contains("pub(crate) fn apply_effect_and_composite_to_view_projective(")
            && effect_source.contains("self.encode_effect(")
            && render_source.contains("self.renderer\n                .effect_renderer\n                .encode_composite_to_view_projective("),
        "effect renderer must stay encode-only while recording surface backend combines transformed effect compositing"
    );
}

#[test]
fn projective_backdrop_copies_record_as_ordered_surface_graph() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let surface_executor_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
            .expect("failed to read surface executor source");
    let effect_source = std::fs::read_to_string(crate_dir.join("src/effect_renderer.rs"))
        .expect("failed to read WGPU effect renderer source");
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let surface_backend_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/backend.rs"))
            .expect("failed to read surface backend source");

    assert!(
        surface_backend_source.contains("fn composite_projective_surfaces_to_view("),
        "surface backends must expose a batched projective surface composite API"
    );
    assert!(
        !surface_executor_source.contains("fn copy_surface_region_to_view("),
        "nested backdrop and underlay paths must not keep the per-copy projective helper"
    );
    assert!(
        surface_executor_source.contains("copy_projective_backdrop_inputs_to_view")
            && surface_executor_source
                .contains("backend.composite_surface_batch_to_view(\n            dest_view,",),
        "axis-aligned backdrop snapshot copies must be recorded through one backend surface-batch graph operation"
    );
    assert!(
        surface_executor_source.contains("let composites = [ancestor_composite, parent_composite];")
            && surface_executor_source.contains(
                "backend.composite_projective_surfaces_to_view(&underlay.view, (width, height), &composites)",
            ),
        "projected child underlays must record ancestor and parent copies as ordered graph entries"
    );
    assert!(
        effect_source.contains("ProjectiveSurfaceComposite")
            && !effect_source.contains("pub(crate) fn composite_projective_surfaces_to_view(")
            && !effect_source.contains("pub fn composite_to_view_projective("),
        "batched projective surface composites must use the backend recorder, not effect-renderer submit wrappers"
    );
    assert!(
        render_source.contains("fn composite_projective_surfaces_to_view(")
            && render_source.contains("self.recorder.record_passes(composite_count);"),
        "recording surface backend must count batched projective surface composite passes"
    );
}

#[test]
fn backdrop_effect_records_effect_and_composite_together() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let surface_executor_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
            .expect("failed to read surface executor source");
    let start = surface_executor_source
        .find("pub(crate) fn apply_backdrop_layer_to_target")
        .expect("backdrop layer renderer must exist");
    let end = surface_executor_source[start..]
        .find("#[allow(clippy::too_many_arguments)]\npub(crate) fn composite_surface_to_view")
        .map(|offset| start + offset)
        .expect("composite helper must follow backdrop layer renderer");
    let body = &surface_executor_source[start..end];

    assert!(
        body.contains("backend.apply_effect_and_composite_to_view("),
        "supported backdrop effects must record effect and final composite together"
    );
    assert!(
        body.contains("if let RenderEffect::Shader { shader } = &layer.effect")
            && body.contains("backend.apply_shader_and_composite_to_view("),
        "supported backdrop shader effects must use direct shader composite instead of effect scratch plus final composite"
    );
    assert!(
        !body.contains("let dest = backend.acquire_offscreen(backdrop_width, backdrop_height);"),
        "backdrop effects must not allocate a caller-owned destination surface before final composite"
    );
    assert!(
        !body.contains("backend.apply_effect("),
        "backdrop effects must not split effect and composite into separate backend operations"
    );
    assert!(
        body.contains("backend.composite_to_view_scissored_with_alpha_and_mask_and_blend_mode("),
        "unsupported backdrop effects must directly composite the snapshot to the target"
    );
}

#[test]
fn isolated_layer_effects_defer_to_final_composite() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let surface_executor_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
            .expect("failed to read surface executor source");
    let layer_start = surface_executor_source
        .find("fn render_layer_surface_uncached")
        .expect("layer surface renderer must exist");
    let layer_end = surface_executor_source[layer_start..]
        .find("pub(crate) fn render_effect_layer_to_target")
        .map(|offset| layer_start + offset)
        .expect("effect layer renderer must follow layer surface renderer");
    let layer_body = &surface_executor_source[layer_start..layer_end];
    let composite_start = surface_executor_source
        .find("fn composite_layer_surface_to_view")
        .expect("layer-surface composite helper must exist");
    let composite_body = &surface_executor_source[composite_start..];

    assert!(
        layer_body.contains("deferred_effect = Some(effect.clone());"),
        "all supported isolated layer effects must be deferred to the final composite boundary"
    );
    assert!(
        !layer_body.contains("matches!(effect, RenderEffect::Shader")
            && !layer_body.contains("backend.apply_effect("),
        "isolated non-shader effects must not be baked into caller-owned offscreen targets"
    );
    assert!(
        layer_body.contains("if deferred_effect.is_none()")
            && layer_body.contains("backend.insert_cached_layer_surface("),
        "final layer-surface cache insertion must be limited to already-baked surfaces"
    );
    assert!(
        composite_body.contains("backend.apply_effect_and_composite_to_view(")
            && composite_body.contains("backend.apply_effect_and_composite_to_view_projective("),
        "deferred non-shader effects must compose through combined axis-aligned and projective graph paths"
    );
}

#[test]
fn surface_executor_allocations_are_semantic() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let surface_executor_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/render_paths.rs"))
            .expect("failed to read surface executor source");
    let surface_backend_source =
        std::fs::read_to_string(crate_dir.join("src/surface_executor/backend.rs"))
            .expect("failed to read surface backend source");
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read renderer source");
    let frame_graph_source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read frame graph source");

    assert!(
        surface_backend_source.contains("fn acquire_retained_surface(")
            && surface_backend_source.contains("fn acquire_frame_surface(")
            && surface_backend_source.contains("fn release_frame_surface("),
        "surface backends must distinguish retained layer surfaces from frame-temporary surfaces"
    );
    assert!(
        !surface_backend_source.contains("fn acquire_offscreen(")
            && !surface_executor_source.contains(".acquire_offscreen("),
        "surface execution paths must not request anonymous offscreen targets"
    );
    assert!(
        surface_executor_source.contains("backend.acquire_retained_surface(width, height)")
            && surface_executor_source.contains("backend.acquire_frame_surface(width, height)")
            && surface_executor_source
                .contains("backend.acquire_frame_surface(effect_width, effect_height)")
            && surface_executor_source
                .contains("backend.acquire_frame_surface(backdrop_width, backdrop_height)")
            && surface_executor_source.contains("backend.release_frame_surface("),
        "layer source, child-underlay, effect-source, and backdrop-snapshot surfaces must declare and release their ownership intent"
    );
    assert!(
        !surface_executor_source
            .contains("effect-free layers return before allocating a destination surface")
            && surface_executor_source
                .contains("effect layer destination requested without render effect")
            && surface_executor_source.contains("backend.release_frame_surface(source);"),
        "effect-layer destination failures must release frame surfaces and return renderer errors"
    );
    assert!(
        render_source.contains("fn acquire_retained_surface(&mut self, width: u32, height: u32)")
            && render_source
                .contains("fn acquire_frame_surface(&mut self, width: u32, height: u32)")
            && render_source
                .contains(".acquire_transient_offscreen(&self.renderer.device, descriptor)")
            && render_source.contains(".release_transient_offscreen(descriptor, target)")
            && render_source
                .contains("let composite_target = backend.acquire_frame_surface(width, height);")
            && render_source.contains("backend.release_frame_surface(composite_target);")
            && render_source.contains("self.acquire_retained_surface(bounds_w, bounds_h)"),
        "renderer-owned root composite and cacheable shadow surfaces must also use semantic allocation helpers, with frame surfaces owned by the executor pool"
    );
    assert!(
        frame_graph_source.contains("trait FrameCommandRecorder")
            && frame_graph_source.contains("fn acquire_transient_offscreen(")
            && frame_graph_source.contains("self.transient_textures.acquire(device, descriptor)")
            && frame_graph_source.contains("fn release_transient_offscreen(")
            && frame_graph_source.contains("release_pending_transients"),
        "frame-temporary surfaces must be allocated from the executor transient texture pool"
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
}

#[test]
fn explicit_frame_encoder_sessions_account_for_recorded_passes() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");

    for required in [
        "frame_encoder.record_passes(outcome.pass_count)",
        "frame_encoder.record_passes(source_outcome.pass_count)",
        "frame_encoder.record_passes(2)",
        "frame_encoder.record_pass();",
        "pass_count = pass_count.saturating_add(1);",
    ] {
        assert!(
            render_source.contains(required),
            "explicit frame encoder rendering must include `{required}`"
        );
    }
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
fn native_segment_shadows_record_through_executor_command_recorder() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let frame_graph_source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");
    let start = render_source
        .find("fn render_non_effect_segment(")
        .expect("segment renderer must exist");
    let end = render_source[start..]
        .find("fn render_segment_draw_chunk<")
        .map(|offset| start + offset)
        .expect("segment draw chunk renderer must exist");
    let body = &render_source[start..end];

    assert!(
        body.contains("#[cfg(not(target_arch = \"wasm32\"))]"),
        "native segment shadow rendering must have a native encode-only branch"
    );
    assert!(
        body.contains("let mut frame_graph = WgpuFrameGraph::new(Some(\"Renderer Frame Graph\"));")
            && body.contains("frame_graph.add_fallible_recorded_command_pass(")
            && body.contains("executor.execute_recorded_graph(&device, &queue, frame_graph)"),
        "native frames must enter the executor-owned frame graph"
    );
    assert!(
        body.contains("renderer.encode_non_effect_segment_commands("),
        "native segment rendering must record commands without owning a submit-capable encoder"
    );
    assert!(
        frame_graph_source
            .contains("#[cfg(target_arch = \"wasm32\")]\npub(crate) struct WgpuFrameEncoder"),
        "explicit wasm frame encoder sessions must stay isolated to the browser command path"
    );
    assert!(
        !frame_graph_source.contains("submit_and_restart"),
        "wasm frame encoder sessions must not submit mid-segment now that draw and shadow uploads use pass-owned resources"
    );
    assert!(
        frame_graph_source.contains("impl FrameCommandRecorder for PassContext<'_>"),
        "native command recording must use a pass context that cannot submit itself"
    );
    assert!(
        body.contains("#[cfg(target_arch = \"wasm32\")]"),
        "wasm segment rendering keeps explicit shadow/effect boundaries while draw batches use retained resources"
    );
}

#[test]
fn native_segment_uploads_allocate_unique_command_buffer_ranges() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let frame_graph_source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");

    assert!(
        frame_graph_source
            .contains("fn allocate_staged_upload_bytes(&mut self, bytes: u64) -> u64;"),
        "command recorders must own staged upload source ranges for pending command buffers"
    );
    assert!(
        frame_graph_source.contains("staged_upload_cursor: &'pass mut u64"),
        "native pass contexts must retain a shared cursor across all draw chunks in one frame graph"
    );
    assert!(
        render_source.contains(".allocate_staged_upload_bytes(staged_uploads.bytes.len() as u64)"),
        "segment and shadow encoders must allocate unique upload-buffer ranges instead of rewriting offset zero"
    );

    let segment_start = render_source
        .find("fn render_segment_draw_chunk<")
        .expect("segment draw chunk renderer must exist");
    let segment_end = render_source[segment_start..]
        .find("fn viewport_uniforms(")
        .map(|offset| segment_start + offset)
        .expect("viewport helper must follow segment renderer");
    let segment_body = &render_source[segment_start..segment_end];
    assert!(
        !segment_body.contains("let mut upload_buffer_offset"),
        "segment draw chunks must not reset upload-buffer offsets inside one pending command buffer"
    );
    assert!(
        !segment_body.contains("unreachable!(")
            && segment_body.contains("shape batch contains non-shape draw item")
            && segment_body.contains("image batch contains non-image draw item")
            && render_source.contains("text batch contains non-text draw item"),
        "segment draw chunks must report batch planner/type mismatches instead of aborting"
    );
    let plan_start = render_source
        .find("fn segment_batch_plan_at_cursor(")
        .expect("segment batch planner must exist");
    let plan_end = render_source[plan_start..]
        .find("fn collect_non_effect_segment_items(")
        .map(|offset| plan_start + offset)
        .expect("segment item collector must follow batch planner");
    let plan_body = &render_source[plan_start..plan_end];
    assert!(
        plan_body.contains("-> Option<(SegmentBatchPlan, usize)>")
            && plan_body.contains("SegmentDrawItem::Shadow(_) => None"),
        "segment batch planning must represent shadow cursor misses explicitly"
    );
}

#[test]
fn shadow_temporary_surfaces_are_frame_recorder_transients() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let render_source = std::fs::read_to_string(crate_dir.join("src/render.rs"))
        .expect("failed to read WGPU renderer source");
    let frame_graph_source = std::fs::read_to_string(crate_dir.join("src/frame_graph.rs"))
        .expect("failed to read WGPU frame graph source");

    for required in [
        "fn acquire_transient_offscreen(",
        "fn release_transient_offscreen(",
        "pending_transient_releases",
        "release_pending_transients",
        "transient_texture_bytes",
    ] {
        assert!(
            frame_graph_source.contains(required),
            "frame command recorders must own `{required}` for temporary surfaces"
        );
    }

    let shadow_start = render_source
        .find("fn encode_shadow_draw<")
        .expect("shadow encoder must exist");
    let shadow_end = render_source[shadow_start..]
        .find("fn encode_shadow_shape_source_passes<")
        .map(|offset| shadow_start + offset)
        .expect("shadow source encoder helper must follow shadow encoder");
    let shadow_body = &render_source[shadow_start..shadow_end];
    assert!(
        shadow_body
            .contains("frame_encoder.acquire_transient_offscreen(&device, source_descriptor)")
            && shadow_body
                .contains("frame_encoder.acquire_transient_offscreen(&device, scratch_descriptor)")
            && shadow_body
                .contains("frame_encoder.release_transient_offscreen(scratch_descriptor, scratch)")
            && shadow_body
                .contains("frame_encoder.release_transient_offscreen(source_descriptor, source)"),
        "mixed/text blurred shadow source and scratch textures must belong to the frame recorder"
    );
    assert!(
        !shadow_body.contains("self.acquire_offscreen(bounds_w, bounds_h)"),
        "mixed/text blurred shadow paths must not allocate caller-owned temporary surfaces"
    );

    let shape_start = render_source
        .find("fn encode_shape_only_blurred_shadow_draw<")
        .expect("shape-only shadow encoder must exist");
    let shape_end = render_source[shape_start..]
        .find("fn prepare_shapes_batch")
        .map(|offset| shape_start + offset)
        .expect("next renderer helper must follow shape-only shadow encoder");
    let shape_body = &render_source[shape_start..shape_end];
    assert!(
        shape_body.contains(
            "let scratch = frame_encoder.acquire_transient_offscreen(&device, scratch_descriptor);"
        ) && shape_body.contains("let source_is_cacheable = cache_key.is_some();")
            && shape_body
                .contains("frame_encoder.release_transient_offscreen(scratch_descriptor, scratch)")
            && shape_body
                .contains("frame_encoder.release_transient_offscreen(source_descriptor, source)"),
        "shape-only shadow scratch must be a frame transient, with retained source allocation limited to cache insertion"
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
            && render_source.contains("fn prepare_text_image_draw_cmds"),
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
        render_source.contains("wasm_shape_batches: Vec<ShapeBatchBuffers>")
            && render_source.contains("wasm_image_batches: Vec<ImageBatchBuffers>")
            && render_source.contains("wasm_uniform_batches: Vec<UniformBatchBuffer>"),
        "wasm draw batches must own retained per-batch GPU resources"
    );
}

#[test]
fn wgpu_text_system_uses_one_shared_state_for_measure_and_render() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    // The renderer's producer half lives in frontend.rs (text state, app
    // context, scene); the shared-text-state contract spans both files.
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
            && source.contains("gpu_renderer.render(")
            && source.contains("self.dev_overlay_graph.as_ref()"),
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

/// The stroke/arc contract, isolated from the font-dependent text cases so a
/// missing system font can never mask a geometry regression. These assertions
/// are shared verbatim with the pixels backend, which is what proves the CPU
/// rasterizer is not silently falling back to filled shapes.
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
