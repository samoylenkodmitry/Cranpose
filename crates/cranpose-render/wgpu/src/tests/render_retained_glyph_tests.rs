use cranpose_ui::text::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::Color;

use super::*;

fn test_renderer() -> (std::sync::MutexGuard<'static, ()>, GpuRenderer) {
    let (lock, device, queue) = crate::frame_graph::upload_test_device();
    let backend = device.adapter_info().backend;
    let renderer = GpuRenderer::new(GpuRendererInit {
        device: Arc::new(device),
        queue: Arc::new(queue),
        surface_format: wgpu::TextureFormat::Rgba8Unorm,
        adapter_backend: backend,
        adapter_downlevel: wgpu::DownlevelFlags::empty(),
        text_fonts: SoftwareTextFontSet::empty(),
        renderer_epoch: 0,
        pipeline_compilation: PipelineCompilation::Background,
    });
    (lock, renderer)
}

fn test_quads() -> [CachedTextGlyphQuad; 1] {
    [CachedTextGlyphQuad {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
        color: (1.0, 1.0, 1.0, 1.0),
        uv: ImageUvRect {
            min: [0.0, 0.0],
            max: [1.0, 1.0],
            sample_bounds: [0.0, 0.0, 1.0, 1.0],
        },
    }]
}

/// A one-glyph run drawn at `x`: a white 2×2 glyph over the atlas's
/// top-left corner.
type TestRun = ([SoftwareGlyphAtlasPlacement; 1], [GlyphAtlasEntry; 1]);

fn test_run(x: i32) -> TestRun {
    let key = SoftwareGlyphAtlasKey {
        font_hash: 0,
        glyph_id: 0,
        scale_x_bits: 0,
        scale_y_bits: 0,
        embolden_px_bits: 0,
        slant_bits: 0,
    };
    (
        [SoftwareGlyphAtlasPlacement {
            key,
            x,
            y: 0,
            width: 2,
            height: 2,
            color: Color(1.0, 1.0, 1.0, 1.0),
        }],
        [GlyphAtlasEntry {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        }],
    )
}

fn run_glyphs(run: &TestRun) -> RunGlyphs {
    RunGlyphs::of(run.0.iter().copied(), &mut RunGlyphScratch::default())
        .expect("a test run has a compact form")
}

fn run_quads<'a>(
    renderer: &GpuRenderer,
    glyphs: &'a RunGlyphs,
    run: &'a TestRun,
) -> GlyphRunQuads<'a> {
    GlyphRunQuads {
        glyphs,
        entries: &run.1,
        atlas_size: renderer.text_glyph_atlas.size(),
        bounds: GlyphRunBounds::of(run.0),
    }
}

fn queue_glyph(renderer: &mut GpuRenderer, key: u64, x: f32, commands: &mut Vec<GlyphDrawCmd>) {
    let run = test_run(0);
    let glyphs = run_glyphs(&run);
    let quads = run_quads(renderer, &glyphs, &run);
    assert!(renderer.emit_retained_text_glyph_run_if_ready(
        TextGlyphRunCacheKey(key),
        quads,
        ViewportUniformParams {
            width: 8,
            height: 8,
            offset: [0.0, 0.0],
            transform: SegmentTransform::IDENTITY,
            origin: [0.0; 2],
            depth_base: 0.0,
        },
        Rect {
            x,
            y: 0.0,
            width: 8.0,
            height: 8.0
        },
        (0, 0, 8, 8),
        commands,
    ));
}

fn draw_queued(renderer: &mut GpuRenderer, commands: &[GlyphDrawCmd]) -> wgpu::Texture {
    let (device, queue) = (Arc::clone(&renderer.device), Arc::clone(&renderer.queue));
    let target = crate::offscreen::create_2d_texture(
        &renderer.device,
        renderer.composition_format,
        8,
        8,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        Some("Queued glyph test"),
    );
    let view = target.create_view(&Default::default());
    let mut graph = WgpuFrameGraph::new(None);
    graph.add_fallible_command_pass(None, &[], &[], |recorder| {
        renderer.stage_frame_uploads(recorder);
        let mut pass = recorder.begin_color_pass(
            "Queued glyph test",
            &view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        renderer.draw_glyph_cmds(
            &mut pass,
            (None, None),
            0,
            commands,
            None,
            crate::render::PassFrame {
                size: (8, 8),
                depth: false,
            },
        )
    });
    WgpuFrameGraphExecutor::new()
        .execute_recorded_graph(&device, &queue, graph)
        .expect("draw queued glyphs");
    target
}

fn whiten_atlas(renderer: &GpuRenderer) {
    let size = renderer.text_glyph_atlas.size();
    WgpuFrameGraphExecutor::new().upload_texture(
        &renderer.queue,
        renderer.text_glyph_atlas.texture.as_image_copy(),
        &vec![255; (size * size) as usize],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size),
            rows_per_image: Some(size),
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
}

/// Asserts the glyph rows of `target` are white in the columns `is_white`
/// accepts and clear elsewhere.
fn assert_white_columns(
    renderer: &GpuRenderer,
    target: &wgpu::Texture,
    is_white: impl Fn(usize) -> bool,
    message: &str,
) {
    let pixels = crate::frame_graph::read_test_texture(&renderer.device, &renderer.queue, target);
    assert_eq!(renderer.device_error_count(), 0);
    let white: &[u8] = &[255; 4];
    for y in 0..2 {
        for x in 0..8 {
            let offset = (y * 8 + x) * white.len();
            let pixel = &pixels[offset..offset + white.len()];
            if is_white(x) {
                assert_eq!(pixel, white, "{message}");
            } else {
                assert!(pixel.iter().all(|byte| *byte == 0), "{message}");
            }
        }
    }
}

#[test]
fn queued_glyph_draw_keeps_its_atlas_after_growth() {
    let (_lock, mut renderer) = test_renderer();
    let size = renderer.text_glyph_atlas.size();
    whiten_atlas(&renderer);
    let mut commands = Vec::new();
    queue_glyph(&mut renderer, 1, 0.0, &mut commands);
    renderer.text_glyph_atlas.reset(
        &renderer.device,
        &renderer.image_bind_group_layout,
        GlyphSamplers {
            nearest: &renderer.image_nearest_sampler,
            linear: &renderer.image_linear_sampler,
        },
    );
    assert_eq!(renderer.text_glyph_atlas.size(), size * 2);
    queue_glyph(&mut renderer, 2, 4.0, &mut commands);
    let target = draw_queued(&mut renderer, &commands);
    assert_white_columns(
        &renderer,
        &target,
        |x| x < 2,
        "the queued glyph keeps its original atlas; new draws use the new one",
    );
}

#[test]
fn queued_glyph_draw_keeps_its_quads_after_cache_eviction() {
    let (_lock, mut renderer) = test_renderer();
    whiten_atlas(&renderer);
    renderer.text_glyph_gpu_run_cache = BoundedLruCache::with_capacity_at_least_one(1);
    let mut commands = Vec::new();
    queue_glyph(&mut renderer, 1, 0.0, &mut commands);
    let moved = test_run(4);
    let glyphs = run_glyphs(&moved);
    let quads = run_quads(&renderer, &glyphs, &moved);
    assert!(renderer.ensure_retained_text_glyph_run(TextGlyphRunCacheKey(2), quads));
    assert!(
        renderer
            .text_glyph_gpu_run_cache
            .peek(&TextGlyphRunCacheKey(1))
            .is_none()
    );
    let target = draw_queued(&mut renderer, &commands);
    assert_white_columns(
        &renderer,
        &target,
        |x| x < 2,
        "the evicted run's quads stay its own for the frame",
    );
}

#[test]
fn retained_glyph_runs_draw_together_and_again_on_later_frames() {
    let (_lock, mut renderer) = test_renderer();
    whiten_atlas(&renderer);
    let mut commands = Vec::new();
    queue_glyph(&mut renderer, 1, 0.0, &mut commands);
    queue_glyph(&mut renderer, 2, 4.0, &mut commands);
    let target = draw_queued(&mut renderer, &commands);
    assert_white_columns(
        &renderer,
        &target,
        |x| x < 2 || (4..6).contains(&x),
        "both runs",
    );
    renderer.text_glyph_run_arena.begin_frame();
    let mut second = Vec::new();
    queue_glyph(&mut renderer, 2, 4.0, &mut second);
    let target = draw_queued(&mut renderer, &second);
    assert_white_columns(
        &renderer,
        &target,
        |x| (4..6).contains(&x),
        "a cached run draws again",
    );
}

fn quarter_turn() -> SegmentTransform {
    SegmentTransform::affine([0.0, -1.0, 1.0, 0.0], [100.0, 0.0]).expect("a turn is invertible")
}

fn viewport(transform: SegmentTransform) -> ViewportUniformParams {
    ViewportUniformParams {
        width: 100,
        height: 50,
        offset: [10.0, 20.0],
        transform,
        origin: [0.0; 2],
        depth_base: 0.0,
    }
}

#[test]
fn the_viewport_uniform_lays_out_as_the_shaders_declare_it() {
    assert_eq!(std::mem::offset_of!(Uniforms, transform), 16);
    assert_eq!(std::mem::offset_of!(Uniforms, inverse), 48);
    assert_eq!(std::mem::offset_of!(Uniforms, origin), 64);
    assert_eq!(std::mem::offset_of!(Uniforms, placement), 80);
    let identity = Uniforms::of(
        viewport(SegmentTransform::IDENTITY),
        PlacementData::zeroed(),
        super::RunTier::Arena,
    );
    assert_eq!(identity.transform, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(identity.translation, [0.0, 0.0]);
    let turned = Uniforms::of(
        viewport(quarter_turn()),
        PlacementData::zeroed(),
        super::RunTier::Arena,
    );
    assert_eq!(turned.transform, [0.0, -1.0, 1.0, 0.0]);
    assert_eq!(turned.translation, [100.0, 0.0]);
    assert_eq!(turned.inverse, [0.0, 1.0, -1.0, 0.0]);
}

#[test]
fn a_turned_viewport_shows_and_scissors_the_bounds_its_turn_maps() {
    let identity = viewport(SegmentTransform::IDENTITY);
    assert_eq!(
        identity.scene_rect(2.0),
        Rect {
            x: 5.0,
            y: 10.0,
            width: 50.0,
            height: 25.0
        }
    );
    let turned = viewport(quarter_turn());
    assert_eq!(
        turned.scene_rect(2.0),
        Rect {
            x: 10.0,
            y: -5.0,
            width: 25.0,
            height: 50.0
        }
    );
    let drawn = Rect {
        x: 5.0,
        y: 10.0,
        width: 10.0,
        height: 5.0,
    };
    assert_eq!(
        scissor_rect_for_rect(drawn, 2.0, identity),
        Some((0, 0, 20, 10))
    );
    assert_eq!(
        scissor_rect_for_rect(drawn, 2.0, turned),
        Some((60, 0, 10, 10))
    );
}

#[test]
fn a_retained_glyph_run_under_a_turn_adds_its_raster_origin_before_the_turn() {
    let raster = Rect {
        x: 7.0,
        y: 3.0,
        width: 8.0,
        height: 8.0,
    };
    let untransformed =
        GpuRenderer::retained_glyph_viewport(viewport(SegmentTransform::IDENTITY), raster);
    assert_eq!(untransformed.offset, [3.0, 17.0]);
    assert_eq!(untransformed.origin, [0.0, 0.0]);
    let turned = GpuRenderer::retained_glyph_viewport(viewport(quarter_turn()), raster);
    assert_eq!(turned.offset, [10.0, 20.0]);
    assert_eq!(turned.transform, quarter_turn());
    assert_eq!(turned.origin, [7.0, 3.0]);
}

#[test]
fn the_glyph_atlas_filters_only_glyphs_a_transform_turns() {
    let (_lock, renderer) = test_renderer();
    let atlas = &renderer.text_glyph_atlas;
    assert!(Rc::ptr_eq(
        &atlas.bind_group(SegmentTransform::IDENTITY),
        &atlas.texel_bind_group
    ));
    assert!(Rc::ptr_eq(
        &atlas.bind_group(quarter_turn()),
        &atlas.filtered_bind_group
    ));
}

#[test]
fn a_draw_samples_as_it_asks_on_the_pixel_grid_and_filtered_off_it() {
    for sampling in [ImageSampling::Nearest, ImageSampling::Linear] {
        assert_eq!(
            sampling_under(sampling, SegmentTransform::IDENTITY),
            sampling
        );
        assert_eq!(
            sampling_under(sampling, quarter_turn()),
            ImageSampling::Linear
        );
    }
}

#[test]
fn a_glyph_run_no_frame_draws_leaves_the_cpu_cache() {
    let (_lock, mut renderer) = test_renderer();
    let run = |renderer: &GpuRenderer| CachedTextGlyphRun {
        glyphs: RunGlyphs::of(std::iter::empty(), &mut RunGlyphScratch::default())
            .expect("an empty run has a compact form"),
        bounds: GlyphRunBounds::of(std::iter::empty()),
        atlas_entries: None,
        atlas_generation: 0,
        last_frame: Cell::new(renderer.text_glyph_run_frame),
    };
    let idle = run(&renderer);
    renderer
        .text_glyph_run_cache
        .put(TextGlyphRunCacheKey(1), idle);
    let drawn = run(&renderer);
    renderer
        .text_glyph_run_cache
        .put(TextGlyphRunCacheKey(2), drawn);
    for _ in 0..TEXT_GLYPH_RUN_IDLE_FRAMES {
        renderer.begin_text_glyph_run_frame();
        let frame = renderer.text_glyph_run_frame;
        if let Some(drawn) = renderer.text_glyph_run_cache.get(&TextGlyphRunCacheKey(2)) {
            drawn.last_frame.set(frame);
        }
    }
    assert!(
        renderer
            .text_glyph_run_cache
            .peek(&TextGlyphRunCacheKey(1))
            .is_some()
    );
    renderer.begin_text_glyph_run_frame();
    assert!(
        renderer
            .text_glyph_run_cache
            .peek(&TextGlyphRunCacheKey(1))
            .is_none(),
        "a run no frame drew leaves after its idle frames"
    );
    assert!(
        renderer
            .text_glyph_run_cache
            .peek(&TextGlyphRunCacheKey(2))
            .is_some(),
        "a run drawn every frame stays"
    );
}

#[test]
fn a_retained_run_no_frame_draws_gives_its_quads_back() {
    let (_lock, mut renderer) = test_renderer();
    let run = test_run(0);
    let glyphs = run_glyphs(&run);
    let quads = run_quads(&renderer, &glyphs, &run);
    assert!(renderer.ensure_retained_text_glyph_run(TextGlyphRunCacheKey(1), quads));
    assert!(renderer.ensure_retained_text_glyph_run(TextGlyphRunCacheKey(2), quads));
    for _ in 0..TEXT_GLYPH_RUN_IDLE_FRAMES {
        renderer.begin_text_glyph_run_frame();
        assert!(
            renderer
                .retained_text_glyph_run(TextGlyphRunCacheKey(2))
                .is_some()
        );
    }
    assert!(
        renderer
            .text_glyph_gpu_run_cache
            .peek(&TextGlyphRunCacheKey(1))
            .is_some()
    );
    renderer.begin_text_glyph_run_frame();
    assert!(
        renderer
            .text_glyph_gpu_run_cache
            .peek(&TextGlyphRunCacheKey(1))
            .is_none(),
        "an idle run leaves after its idle frames"
    );
    assert!(
        renderer
            .text_glyph_gpu_run_cache
            .peek(&TextGlyphRunCacheKey(2))
            .is_some()
    );
}

#[test]
fn consecutive_shared_glyph_quads_draw_as_one_until_a_state_changes() {
    let (_lock, mut renderer) = test_renderer();
    let texel = Rc::clone(&renderer.text_glyph_atlas.texel_bind_group);
    let filtered = Rc::clone(&renderer.text_glyph_atlas.filtered_bind_group);
    let full = (0, 0, 8, 8);
    let half = (0, 0, 4, 8);
    let plain = GlyphQuads::Plain;
    let mut cmds = vec![
        GlyphDrawCmd::shared((0..1, plain), Some(full), full, Rc::clone(&texel)),
        GlyphDrawCmd::shared((1..3, plain), Some(full), full, Rc::clone(&texel)),
        GlyphDrawCmd::shared((3..4, plain), Some(half), half, Rc::clone(&texel)),
        GlyphDrawCmd::shared((4..5, plain), Some(half), half, Rc::clone(&filtered)),
        GlyphDrawCmd::shared((6..7, plain), Some(half), half, Rc::clone(&filtered)),
    ];
    queue_glyph(&mut renderer, 1, 0.0, &mut cmds);
    cmds.push(GlyphDrawCmd::shared(
        (7..8, plain),
        Some(half),
        half,
        Rc::clone(&filtered),
    ));
    cmds.push(GlyphDrawCmd::shared(
        (8..9, GlyphQuads::Aligned),
        Some(half),
        half,
        Rc::clone(&filtered),
    ));

    let draws: Vec<_> = GlyphDraws::new(&cmds)
        .map(|draw| match draw.step {
            GlyphDrawStep::Shared(instances, _) => (Some(instances), draw.scissor),
            GlyphDrawStep::Retained { .. } => (None, draw.scissor),
        })
        .collect();

    assert_eq!(
        draws,
        vec![
            (Some(0..3), Some(full)),
            (Some(3..4), Some(half)),
            (Some(4..5), Some(half)),
            (Some(6..7), Some(half)),
            (None, Some((0, 0, 8, 8))),
            (Some(7..8), Some(half)),
            (Some(8..9), Some(half)),
        ],
        "a new scissor, a new atlas, a gap in the quads, a retained run and a new quad kind each \
         start a draw"
    );
}

fn fonted_renderer() -> (std::sync::MutexGuard<'static, ()>, GpuRenderer) {
    let (lock, device, queue) = crate::frame_graph::upload_test_device();
    let backend = device.adapter_info().backend;
    let font = cranpose_render_common::software_text_raster::default_software_text_font()
        .expect("the embedded default font");
    let renderer = GpuRenderer::new(GpuRendererInit {
        device: Arc::new(device),
        queue: Arc::new(queue),
        surface_format: wgpu::TextureFormat::Rgba8Unorm,
        adapter_backend: backend,
        adapter_downlevel: wgpu::DownlevelFlags::empty(),
        text_fonts: SoftwareTextFontSet::from_font(font),
        renderer_epoch: 0,
        pipeline_compilation: PipelineCompilation::Background,
    });
    (lock, renderer)
}

fn filled_run(x: f32, width: f32, color: Color) -> RunDraw {
    let mut recorder = cranpose_ui_graphics::ShapeRecorder::default();
    recorder.push_primitive(cranpose_ui_graphics::DrawPrimitive::Rect {
        rect: Rect {
            x,
            y: 0.0,
            width,
            height: 32.0,
        },
        brush: cranpose_ui_graphics::Brush::solid(color),
        stroke: None,
    });
    RunDraw::whole(
        Arc::new(recorder),
        crate::scene::Placement::at(Point::default(), None, None),
    )
    .expect("a run with a rect")
}

fn push_label(scene: &mut CompositorScene, x: f32, color: Color) {
    let style = TextStyle {
        paragraph_style: cranpose_ui::text::ParagraphStyle {
            text_motion: Some(cranpose_ui::text::TextMotion::Static),
            ..Default::default()
        },
        ..Default::default()
    };
    scene.push_text(
        1,
        Rect {
            x,
            y: 4.0,
            width: 30.0,
            height: 24.0,
        },
        cranpose_ui::text::shared_plain_render_string("MM"),
        color,
        std::sync::Arc::new(style),
        18.0,
        1.0,
        TextLayoutOptions::default(),
        None,
    );
}

/// Draws `scene` into a 128×32 target; the target's pixels and the draws
/// the pass issued.
fn draw_scene(renderer: &mut GpuRenderer, scene: &CompositorScene) -> (Vec<u8>, u32) {
    let target = crate::offscreen::create_2d_texture(
        &renderer.device,
        renderer.composition_format,
        128,
        32,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        Some("Batched text test"),
    );
    let view = target.create_view(&Default::default());
    renderer.viewport_uniforms.begin_frame();
    renderer.run_store.begin_frame(false);
    renderer.text_glyph_run_arena.begin_frame();
    renderer.frame_stats.draw_calls.set(0);
    let segments = [crate::draw_pass::PassSegment {
        scene,
        ops: &scene.draw_ops,
        composites: &[],
        offset: [0.0, 0.0],
        scissor: None,
        first_run_window: None,
        transform: SegmentTransform::IDENTITY,
        scale: 1.0,
    }];
    let (device, queue) = (Arc::clone(&renderer.device), Arc::clone(&renderer.queue));
    let mut graph = WgpuFrameGraph::new(None);
    graph.add_fallible_command_pass(None, &[], &[], |recorder| {
        renderer.encode_pass(
            recorder,
            crate::draw_pass::PassTarget {
                view: &view,
                width: 128,
                height: 32,
            },
            &segments,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            "Batched text test",
        )?;
        renderer.stage_frame_uploads(recorder);
        renderer.viewport_uniforms.uploads.finish_frame();
        renderer.run_store.finish_frame();
        Ok(())
    });
    WgpuFrameGraphExecutor::new()
        .execute_recorded_graph(&device, &queue, graph)
        .expect("draw the scene");
    let pixels = crate::frame_graph::read_test_texture(&renderer.device, &renderer.queue, &target);
    assert_eq!(renderer.device_error_count(), 0);
    (pixels, renderer.frame_stats.draw_calls.get())
}

fn pixel_at(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    let bytes = COMPOSITION_BYTES_PER_PIXEL as usize;
    let offset = (y * 128 + x) * bytes;
    &pixels[offset..offset + bytes]
}

fn has_text_pixels(pixels: &[u8], columns: std::ops::Range<usize>, background: &[u8]) -> bool {
    columns
        .flat_map(|x| (0..32).map(move |y| (x, y)))
        .any(|(x, y)| pixel_at(pixels, x, y) != background)
}

#[test]
fn labels_between_shapes_they_do_not_touch_draw_in_one_batch() {
    let (_lock, mut renderer) = fonted_renderer();
    let red = Color(1.0, 0.0, 0.0, 1.0);
    let mut scene = CompositorScene::new();
    for x in [0.0, 64.0] {
        scene.push_run(filled_run(x, 40.0, red));
        push_label(&mut scene, x + 4.0, Color::WHITE);
    }
    let (pixels, draws) = draw_scene(&mut renderer, &scene);
    let background = pixel_at(&pixels, 2, 2).to_vec();
    assert!(
        has_text_pixels(&pixels, 4..34, &background),
        "the first label shows over its card"
    );
    assert!(
        has_text_pixels(&pixels, 68..98, &background),
        "the second label shows over its card"
    );
    assert_eq!(
        draws, 3,
        "one interior pre-pass draw, one shape draw and one glyph draw for both cards: \
         a label's quads past its draw rect are cut, so it needs no scissor of its own"
    );
}

#[test]
fn a_shape_over_a_label_still_draws_over_it() {
    let (_lock, mut renderer) = fonted_renderer();
    let red = Color(1.0, 0.0, 0.0, 1.0);
    let blue = Color(0.0, 0.0, 1.0, 1.0);
    let mut scene = CompositorScene::new();
    scene.push_run(filled_run(0.0, 40.0, red));
    push_label(&mut scene, 4.0, Color::WHITE);
    scene.push_run(filled_run(0.0, 128.0, blue));
    let (pixels, _) = draw_scene(&mut renderer, &scene);
    let covered = pixel_at(&pixels, 20, 16).to_vec();
    for x in 0..128 {
        for y in 0..32 {
            assert_eq!(
                pixel_at(&pixels, x, y),
                covered.as_slice(),
                "the blue shape covers the label at {x},{y}"
            );
        }
    }
}

#[test]
fn a_glyph_quad_becomes_one_instance_at_its_raster_origin() {
    let [quad] = test_quads();
    let origin = Rect {
        x: 3.0,
        y: 5.0,
        width: 8.0,
        height: 8.0,
    };
    assert_eq!(
        cached_text_glyph_instance(origin, &quad),
        Some(GlyphInstance {
            rect: [3.0, 5.0, 5.0, 7.0],
            uv: [0.0, 0.0, 1.0, 1.0],
            uv_bounds: [0.0, 0.0, 1.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
        })
    );
    let clear = CachedTextGlyphQuad {
        color: (1.0, 1.0, 1.0, 0.0),
        ..quad
    };
    assert_eq!(cached_text_glyph_instance(origin, &clear), None);
}

#[test]
fn shared_glyphs_inside_their_scissor_draw_unclipped_within_their_bounds() {
    let [quad] = test_quads();
    let glyphs = [
        Rect {
            x: 1.0,
            y: 1.0,
            width: 0.0,
            height: 0.0,
        },
        Rect {
            x: 4.5,
            y: 2.0,
            width: 0.0,
            height: 0.0,
        },
    ]
    .map(|origin| cached_text_glyph_instance(origin, &quad).expect("a drawn quad"));
    let identity = ViewportUniformParams {
        offset: [0.0, 0.0],
        ..viewport(SegmentTransform::IDENTITY)
    };
    assert_eq!(
        shared_glyph_clip(&glyphs, (0, 0, 8, 8), identity),
        (None, (1, 1, 6, 3)),
        "glyphs inside the scissor need none, and draw within their own bounds"
    );
    assert_eq!(
        shared_glyph_clip(&glyphs, (2, 0, 6, 8), identity),
        (Some((2, 0, 6, 8)), (2, 0, 6, 8)),
        "a glyph past the scissor keeps it"
    );
}

#[test]
fn a_runs_quads_are_derived_from_its_glyphs_and_atlas_entries() {
    let (_lock, renderer) = test_renderer();
    let (mut glyphs, mut entries) = test_run(3);
    glyphs[0].color = Color(2.0, 0.5, -1.0, 1.0);
    entries[0] = GlyphAtlasEntry {
        x: 6,
        y: 10,
        width: 4,
        height: 8,
    };
    let run = (glyphs, entries);
    let glyphs = run_glyphs(&run);
    let quads = run_quads(&renderer, &glyphs, &run);
    let atlas_size = renderer.text_glyph_atlas.size();

    let derived: Vec<_> = quads.iter().collect();

    assert_eq!(quads.len(), 1);
    let [quad] = derived.as_slice() else {
        panic!("one glyph derives one quad");
    };
    let expected = cached_text_glyph_quad(&run.0[0], run.1[0], atlas_size);
    assert_eq!((quad.x, quad.y, quad.width, quad.height), (3, 0, 2, 2));
    assert_eq!(quad.color, (1.0, 0.5, 0.0, 1.0), "colour is clamped");
    assert_eq!(quad.uv.min, expected.uv.min);
    assert_eq!(quad.uv.max, expected.uv.max);
    assert_eq!(quad.uv.sample_bounds, expected.uv.sample_bounds);
}

/// A full-height rect across the target, clipped to `columns`.
fn clipped_run(columns: std::ops::Range<f32>, color: Color) -> RunDraw {
    let run = filled_run(0.0, 128.0, color);
    let clip = Rect {
        x: columns.start,
        y: 0.0,
        width: columns.end - columns.start,
        height: 32.0,
    };
    RunDraw {
        placement: crate::scene::Placement::at(Point::default(), None, Some(clip)),
        ..run
    }
}

#[test]
fn runs_under_different_clips_each_keep_their_own_pixels() {
    let (_lock, mut renderer) = fonted_renderer();
    let red = Color(1.0, 0.0, 0.0, 1.0);
    let blue = Color(0.0, 0.0, 1.0, 1.0);
    let mut scene = CompositorScene::new();
    scene.push_run(clipped_run(10.25..40.5, red));
    scene.push_run(clipped_run(60.5..90.0, blue));
    let (pixels, _) = draw_scene(&mut renderer, &scene);
    let black = pixel_at(&pixels, 0, 16).to_vec();
    let red_pixel = pixel_at(&pixels, 20, 16).to_vec();
    let blue_pixel = pixel_at(&pixels, 70, 16).to_vec();
    assert_ne!(red_pixel, black);
    assert_ne!(blue_pixel, black);
    // A pixel shows a run when its centre lies within the run's clip.
    for x in 0..128 {
        let centre = x as f32 + 0.5;
        let expected = if (10.25..=40.5).contains(&centre) {
            &red_pixel
        } else if (60.5..=90.0).contains(&centre) {
            &blue_pixel
        } else {
            &black
        };
        for y in [0, 16, 31] {
            assert_eq!(
                pixel_at(&pixels, x, y),
                expected.as_slice(),
                "pixel {x},{y}"
            );
        }
    }
}
