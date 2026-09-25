use cranpose_ui::text::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::Color;

use super::*;
use crate::offscreen::composition_format;

fn test_renderer() -> (std::sync::MutexGuard<'static, ()>, GpuRenderer) {
    let (lock, device, queue) = crate::frame_graph::upload_test_device();
    let backend = device.adapter_info().backend;
    let renderer = GpuRenderer::new(
        Arc::new(device),
        Arc::new(queue),
        composition_format(),
        backend,
        wgpu::DownlevelFlags::empty(),
        SoftwareTextFontSet::empty(),
        0,
    );
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

fn queue_glyph(renderer: &mut GpuRenderer, key: u64, x: f32, commands: &mut Vec<GlyphDrawCmd>) {
    assert!(renderer.emit_retained_text_glyph_run_if_ready(
        TextGlyphRunCacheKey(key),
        &test_quads(),
        ViewportUniformParams {
            width: 8,
            height: 8,
            offset: [0.0, 0.0],
            transform: SegmentTransform::IDENTITY,
            origin: [0.0; 2],
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
    renderer.flush_frame_uploads();
    let target = crate::offscreen::create_2d_texture(
        &renderer.device,
        composition_format(),
        8,
        8,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        Some("Queued glyph test"),
    );
    let view = target.create_view(&Default::default());
    let mut graph = WgpuFrameGraph::new(None);
    graph.add_fallible_command_pass(None, &[], &[], |recorder| {
        let mut pass = recorder.begin_color_pass(
            "Queued glyph test",
            &view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        renderer.draw_glyph_cmds(&mut pass, None, 0, commands, None, (8, 8))
    });
    WgpuFrameGraphExecutor::new()
        .execute_recorded_graph(&renderer.device, &renderer.queue, graph)
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
    let white: &[u8] = if composition_format() == wgpu::TextureFormat::Rgba8Unorm {
        &[255; 4]
    } else {
        &[0, 60, 0, 60, 0, 60, 0, 60]
    };
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
    let mut moved = test_quads();
    moved[0].x = 4;
    assert!(renderer.ensure_retained_text_glyph_run(TextGlyphRunCacheKey(2), &moved));
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
fn retained_glyph_runs_of_a_frame_draw_from_one_upload() {
    let (_lock, mut renderer) = test_renderer();
    whiten_atlas(&renderer);
    let mut commands = Vec::new();
    queue_glyph(&mut renderer, 1, 0.0, &mut commands);
    queue_glyph(&mut renderer, 2, 4.0, &mut commands);
    crate::frame_graph::take_upload_write_calls();
    renderer.text_glyph_run_arena.flush(&renderer.queue);
    assert_eq!(crate::frame_graph::take_upload_write_calls(), 1);
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
    );
    assert_eq!(identity.transform, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(identity.translation, [0.0, 0.0]);
    let turned = Uniforms::of(viewport(quarter_turn()), PlacementData::zeroed());
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
fn a_transformed_shape_pipeline_falls_back_to_a_transformed_general_one() {
    let untransformed = ShapePipelineKey::general_for(BlendMode::SrcOver, RunTier::Arena);
    let transformed = ShapePipelineKey {
        transformed: true,
        ..untransformed
    };
    assert_ne!(transformed, untransformed);
    assert!(transformed.general().transformed);
    assert!(!untransformed.general().transformed);
}

#[test]
fn a_retained_run_no_frame_draws_gives_its_quads_back() {
    let (_lock, mut renderer) = test_renderer();
    assert!(renderer.ensure_retained_text_glyph_run(TextGlyphRunCacheKey(1), &test_quads()));
    assert!(renderer.ensure_retained_text_glyph_run(TextGlyphRunCacheKey(2), &test_quads()));
    for _ in 0..RETAINED_TEXT_GLYPH_RUN_IDLE_FRAMES {
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
    let mut cmds = vec![
        GlyphDrawCmd::shared(0..6, Some(full), full, Rc::clone(&texel)),
        GlyphDrawCmd::shared(6..18, Some(full), full, Rc::clone(&texel)),
        GlyphDrawCmd::shared(18..24, Some(half), half, Rc::clone(&texel)),
        GlyphDrawCmd::shared(24..30, Some(half), half, Rc::clone(&filtered)),
        GlyphDrawCmd::shared(36..42, Some(half), half, Rc::clone(&filtered)),
    ];
    queue_glyph(&mut renderer, 1, 0.0, &mut cmds);
    cmds.push(GlyphDrawCmd::shared(
        42..48,
        Some(half),
        half,
        Rc::clone(&filtered),
    ));

    let draws: Vec<_> = GlyphDraws::new(&cmds)
        .map(|draw| match draw.step {
            GlyphDrawStep::Shared(indices) => (Some(indices), draw.scissor),
            GlyphDrawStep::Retained { .. } => (None, draw.scissor),
        })
        .collect();

    assert_eq!(
        draws,
        vec![
            (Some(0..18), Some(full)),
            (Some(18..24), Some(half)),
            (Some(24..30), Some(half)),
            (Some(36..42), Some(half)),
            (None, Some((0, 0, 8, 8))),
            (Some(42..48), Some(half)),
        ],
        "a new scissor, a new atlas, a gap in the quads and a retained run each start a draw"
    );
}

fn fonted_renderer() -> (std::sync::MutexGuard<'static, ()>, GpuRenderer) {
    let (lock, device, queue) = crate::frame_graph::upload_test_device();
    let backend = device.adapter_info().backend;
    let font = cranpose_render_common::software_text_raster::default_software_text_font()
        .expect("the embedded default font");
    let renderer = GpuRenderer::new(
        Arc::new(device),
        Arc::new(queue),
        composition_format(),
        backend,
        wgpu::DownlevelFlags::empty(),
        SoftwareTextFontSet::from_font(font),
        0,
    );
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
        Rc::new(cranpose_ui::text::AnnotatedString::from("MM")),
        color,
        style,
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
        composition_format(),
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
            1.0,
            "Batched text test",
        )?;
        renderer.flush_frame_uploads();
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
    let bytes = composition_format()
        .block_copy_size(None)
        .expect("a sized format") as usize;
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
        "one shape draw and one glyph draw for both cards, plus the clear"
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
