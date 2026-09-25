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
    renderer.viewport_uniforms.flush(&renderer.queue);
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
        renderer.draw_glyph_cmds(&mut pass, None, 0, commands, None)
    });
    WgpuFrameGraphExecutor::new()
        .execute_recorded_graph(&renderer.device, &renderer.queue, graph)
        .expect("draw queued glyphs");
    target
}

#[test]
fn queued_glyph_draw_keeps_its_atlas_after_growth() {
    let (_lock, mut renderer) = test_renderer();
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
    let pixels = crate::frame_graph::read_test_texture(&renderer.device, &renderer.queue, &target);
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
            if x < 2 {
                assert_eq!(pixel, white, "queued glyph must retain its original atlas");
            } else {
                assert!(
                    pixel.iter().all(|byte| *byte == 0),
                    "new draws must use the new atlas"
                );
            }
        }
    }
}

#[test]
fn queued_glyph_draw_keeps_buffers_after_cache_eviction() {
    let (_lock, mut renderer) = test_renderer();
    renderer.text_glyph_gpu_run_cache = BoundedLruCache::with_capacity_at_least_one(1);
    let mut commands = Vec::new();
    queue_glyph(&mut renderer, 1, 0.0, &mut commands);
    assert!(renderer.ensure_retained_text_glyph_run(TextGlyphRunCacheKey(2), &test_quads()));
    assert!(
        renderer
            .text_glyph_gpu_run_cache
            .peek(&TextGlyphRunCacheKey(1))
            .is_none()
    );
    draw_queued(&mut renderer, &commands);
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
    assert_eq!(identity.quad_margin, 0.0);
    assert_eq!(identity.transform, [1.0, 0.0, 0.0, 1.0]);
    let turned = Uniforms::of(viewport(quarter_turn()), PlacementData::zeroed());
    assert_eq!(turned.quad_margin, TRANSFORMED_QUAD_MARGIN);
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
