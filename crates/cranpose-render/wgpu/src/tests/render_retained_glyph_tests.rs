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
            offset: [0.0, 0.0]
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
        &renderer.image_nearest_sampler,
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
