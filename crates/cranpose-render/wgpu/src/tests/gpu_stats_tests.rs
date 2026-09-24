use super::*;

fn test_layer_cache_key() -> LayerRasterCacheKey {
    LayerRasterCacheKey::source_content(
        None,
        0,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 5.0,
            height: 6.0,
        },
        (5, 6),
        cranpose_render_common::raster_cache::RasterScale::from_scale(1.0),
        cranpose_ui_graphics::Point::default(),
    )
}

#[test]
fn layer_cache_counters_accumulate_and_reset() {
    let stats = FrameStats::default();
    stats.record_command_stats(FrameCommandStats {
        encoder_count: 1,
        submit_count: 1,
        pass_count: 2,
        transient_texture_bytes: 256,
        retained_texture_bytes: 128,
        upload_bytes: 64,
        ..FrameCommandStats::default()
    });
    stats.bump_shapes();
    stats.shape_pipeline_fallback_draws.set(3);
    stats.shape_specialized_draws.set(5);
    stats.shader_pipeline_fallback_draws.set(2);
    stats.shader_specialized_draws.set(4);
    stats.blur_passes.set(1);
    stats.offscreen_total_bytes.set(1024);
    stats.offscreen_pool_bytes.set(2048);
    stats.record_layer_cache_hit(&test_layer_cache_key(), 10, 20);
    stats.record_layer_cache_hit(&test_layer_cache_key(), 3, 4);
    stats.record_layer_cache_miss(&test_layer_cache_key(), 5, 6);
    stats.record_shadow_shape_cache_hit(72);
    stats.record_shadow_shape_cache_miss(10, 11);
    stats.record_shadow_text_blur_fallback();
    stats.record_text_image_cache_hit(13, 17);
    stats.record_text_image_cache_miss(19, 23);

    assert_eq!(stats.layer_cache_hits.get(), 2);
    assert_eq!(stats.layer_cache_misses.get(), 1);
    assert_eq!(stats.layer_cache_hit_pixels.get(), 212);
    assert_eq!(stats.layer_cache_miss_pixels.get(), 30);
    assert_eq!(stats.shadow_shape_cache_hits.get(), 1);
    assert_eq!(stats.shadow_shape_cache_misses.get(), 1);
    assert_eq!(stats.shadow_shape_cache_hit_pixels.get(), 72);
    assert_eq!(stats.shadow_shape_cache_miss_pixels.get(), 110);
    assert_eq!(stats.shadow_text_blur_fallbacks.get(), 1);

    stats.record_isolated_layer_render(
        7,
        8,
        Some(9),
        Rect {
            x: 2.0,
            y: 3.0,
            width: 4.0,
            height: 5.0,
        },
    );
    let snapshot = stats.snapshot();

    assert_eq!(snapshot.shape_pipeline_fallback_draws, 3);
    assert_eq!(snapshot.shape_specialized_draws, 5);
    assert_eq!(snapshot.shader_pipeline_fallback_draws, 2);
    assert_eq!(snapshot.shader_specialized_draws, 4);
    assert_eq!(snapshot.isolated_layer_renders, 1);
    assert_eq!(snapshot.isolated_layer_pixels, 56);
    assert_eq!(snapshot.upload_bytes, 64);
    assert_eq!(snapshot.encoder_count, 1);
    assert_eq!(snapshot.submit_count, 1);
    assert_eq!(snapshot.pass_count, 2);
    assert_eq!(snapshot.transient_texture_bytes, 1280);
    assert_eq!(snapshot.retained_texture_bytes, 2176);
    assert_eq!(snapshot.layer_cache_hits, 2);
    assert_eq!(snapshot.layer_cache_misses, 1);
    assert_eq!(snapshot.shadow_shape_cache_hits, 1);
    assert_eq!(snapshot.shadow_shape_cache_misses, 1);
    assert_eq!(snapshot.shadow_shape_cache_hit_pixels, 72);
    assert_eq!(snapshot.shadow_shape_cache_miss_pixels, 110);
    assert_eq!(snapshot.shadow_text_blur_fallbacks, 1);
    assert_eq!(snapshot.text_image_cache_hits, 1);
    assert_eq!(snapshot.text_image_cache_misses, 1);
    assert_eq!(snapshot.text_image_cache_hit_pixels, 221);
    assert_eq!(snapshot.text_image_cache_miss_pixels, 437);
    assert_eq!(snapshot.text_image_raster_bytes, 1748);
    let top_layers = snapshot.top_isolated_layers().collect::<Vec<_>>();
    assert_eq!(top_layers.len(), 1);
    assert_eq!(top_layers[0].node_id, Some(9));
    assert_eq!(stats.layer_cache_hits.get(), 2);
    assert_eq!(stats.layer_cache_misses.get(), 1);

    stats.reset();

    assert_eq!(stats.snapshot().shape_pipeline_fallback_draws, 0);
    assert_eq!(stats.snapshot().shape_specialized_draws, 0);
    assert_eq!(stats.snapshot().shader_pipeline_fallback_draws, 0);
    assert_eq!(stats.snapshot().shader_specialized_draws, 0);
    assert_eq!(stats.layer_cache_hits.get(), 0);
    assert_eq!(stats.layer_cache_misses.get(), 0);
    assert_eq!(stats.layer_cache_hit_pixels.get(), 0);
    assert_eq!(stats.layer_cache_miss_pixels.get(), 0);
    assert_eq!(stats.shadow_shape_cache_hits.get(), 0);
    assert_eq!(stats.shadow_shape_cache_misses.get(), 0);
    assert_eq!(stats.shadow_shape_cache_hit_pixels.get(), 0);
    assert_eq!(stats.shadow_shape_cache_miss_pixels.get(), 0);
    assert_eq!(stats.shadow_text_blur_fallbacks.get(), 0);
    assert_eq!(stats.text_image_cache_hits.get(), 0);
    assert_eq!(stats.text_image_cache_misses.get(), 0);
    assert_eq!(stats.text_image_cache_hit_pixels.get(), 0);
    assert_eq!(stats.text_image_cache_miss_pixels.get(), 0);
    assert_eq!(stats.text_image_raster_bytes.get(), 0);
    assert_eq!(stats.isolated_layer_renders.get(), 0);
    assert_eq!(stats.isolated_layer_pixels.get(), 0);
    assert_eq!(stats.top_isolated_layer_count.get(), 0);
}

#[test]
fn command_stats_accumulate_and_reset() {
    let stats = FrameStats::default();

    stats.record_command_stats(FrameCommandStats {
        encoder_count: 2,
        submit_count: 2,
        pass_count: 5,
        transient_texture_bytes: 1024,
        retained_texture_bytes: 2048,
        upload_bytes: 512,
        ..FrameCommandStats::default()
    });
    stats.bump_shapes();

    let snapshot = stats.snapshot();
    assert_eq!(snapshot.submits, 2);
    assert_eq!(snapshot.encoder_count, 2);
    assert_eq!(snapshot.submit_count, 2);
    assert_eq!(snapshot.pass_count, 5);
    assert_eq!(snapshot.transient_texture_bytes, 1024);
    assert_eq!(snapshot.retained_texture_bytes, 2048);
    assert_eq!(snapshot.upload_bytes, 512);

    stats.reset();
    let reset = stats.snapshot();
    assert_eq!(reset.submits, 0);
    assert_eq!(reset.encoder_count, 0);
    assert_eq!(reset.submit_count, 0);
    assert_eq!(reset.pass_count, 0);
    assert_eq!(reset.transient_texture_bytes, 0);
    assert_eq!(reset.retained_texture_bytes, 0);
    assert_eq!(reset.upload_bytes, 0);
}

#[test]
fn snapshot_adds_explicit_readback_command_stats() {
    let stats = FrameStats::default();
    stats.record_command_stats(FrameCommandStats {
        encoder_count: 1,
        submit_count: 1,
        pass_count: 2,
        transient_texture_bytes: 128,
        retained_texture_bytes: 512,
        upload_bytes: 64,
        ..FrameCommandStats::default()
    });
    let snapshot = stats
        .snapshot()
        .with_command_stats_added(FrameCommandStats {
            encoder_count: 1,
            submit_count: 1,
            pass_count: 1,
            ..FrameCommandStats::default()
        });

    assert_eq!(snapshot.submits, 2);
    assert_eq!(snapshot.encoder_count, 2);
    assert_eq!(snapshot.submit_count, 2);
    assert_eq!(snapshot.pass_count, 3);
    assert_eq!(snapshot.transient_texture_bytes, 128);
    assert_eq!(snapshot.retained_texture_bytes, 512);
    assert_eq!(snapshot.upload_bytes, 64);
}

#[test]
fn maybe_print_snapshot_only_advances_frame_counter_when_enabled() {
    let stats = FrameStats::default();
    let snapshot = stats.snapshot();
    let mut frame_count = 0;

    stats.maybe_print_snapshot(snapshot, &mut frame_count, false);
    assert_eq!(frame_count, 0);

    stats.maybe_print_snapshot(snapshot, &mut frame_count, true);
    assert_eq!(frame_count, 1);
}

#[test]
fn top_isolated_layers_keep_largest_runtime_surfaces() {
    let stats = FrameStats::default();
    for index in 0..(TOP_ISOLATED_LAYER_LIMIT + 2) {
        stats.record_isolated_layer_render(
            16 + index as u32,
            8 + index as u32,
            Some(index),
            Rect {
                x: index as f32,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        );
    }

    let snapshot = stats.snapshot();
    let top_layers = snapshot.top_isolated_layers().collect::<Vec<_>>();
    assert_eq!(top_layers.len(), TOP_ISOLATED_LAYER_LIMIT);
    assert_eq!(top_layers[0].node_id, Some(TOP_ISOLATED_LAYER_LIMIT + 1));
    assert_eq!(top_layers[1].node_id, Some(TOP_ISOLATED_LAYER_LIMIT));
}
