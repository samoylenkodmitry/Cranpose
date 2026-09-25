use super::*;

#[test]
fn raster_scale_normalizes_invalid_values() {
    assert_eq!(
        RasterScale::from_scale(0.0).raw(),
        RasterScale::from_scale(1.0).raw()
    );
    assert_eq!(
        RasterScale::from_scale(-3.0).raw(),
        RasterScale::from_scale(1.0).raw()
    );
    assert_eq!(
        RasterScale::from_scale(f32::NAN).raw(),
        RasterScale::from_scale(1.0).raw()
    );
}

#[test]
fn raster_scale_tells_apart_scales_a_thousandth_apart() {
    assert_ne!(
        RasterScale::from_scale(1.0),
        RasterScale::from_scale(1.001),
        "a raster drawn at 1.001 is a pixel wider than one drawn at 1.0 across 1000 pixels"
    );
    assert_eq!(RasterScale::from_scale(0.875).raw(), 0.875f32.to_bits());
}

#[test]
fn keys_that_differ_only_in_scale_or_phase_draw_the_same_content() {
    let key = |content: u64, width: f32, scale: f32, phase: f32| {
        let pixels = (width * scale).ceil() as u32;
        LayerRasterCacheKey::source_content(
            Some(7),
            content,
            Rect {
                x: 0.0,
                y: 0.0,
                width,
                height: 52.0,
            },
            (pixels, pixels),
            RasterScale::from_scale(scale),
            Point::new(phase, 0.0),
        )
    };
    let base = key(1, 52.0, 3.0, 0.0);
    assert!(!base.draws_other_content(key(1, 52.0, 3.0, 0.5)));
    assert!(!base.draws_other_content(key(1, 52.0, 2.5, 0.0)));
    assert!(base.draws_other_content(key(2, 52.0, 3.0, 0.0)));
    assert!(base.draws_other_content(key(1, 48.0, 3.0, 0.0)));
}

#[test]
fn source_content_keys_at_nearby_scales_are_distinct() {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 52.0,
        height: 52.0,
    };
    let key = |scale: f32| {
        LayerRasterCacheKey::source_content(
            Some(7),
            42,
            bounds,
            (156, 156),
            RasterScale::from_scale(scale),
            Point::default(),
        )
    };
    assert_ne!(
        key(2.7),
        key(2.701),
        "a tile animating its scale must not be served the raster of a neighbouring frame"
    );
    assert_eq!(key(2.7).raster_scale(), RasterScale::from_scale(2.7));
}

#[test]
fn layer_raster_cache_key_captures_bounds_and_pixel_size() {
    let rect = Rect {
        x: 1.0,
        y: 2.0,
        width: 30.0,
        height: 40.0,
    };
    let base = LayerRasterCacheKey::source_content(
        Some(7),
        11,
        rect,
        (30, 40),
        RasterScale::from_scale(1.0),
        Point::default(),
    );
    let moved = LayerRasterCacheKey::source_content(
        Some(7),
        11,
        Rect { x: 2.0, ..rect },
        (30, 40),
        RasterScale::from_scale(1.0),
        Point::default(),
    );
    let resized = LayerRasterCacheKey::source_content(
        Some(7),
        11,
        rect,
        (60, 80),
        RasterScale::from_scale(2.0),
        Point::default(),
    );

    assert_ne!(base, moved);
    assert_ne!(base, resized);
    assert_eq!(base.stable_id(), Some(7));
    assert_eq!(base.pixel_size(), (30, 40));
}

#[test]
fn layer_raster_cache_key_captures_the_device_phase() {
    let rect = Rect {
        x: 1.0,
        y: 2.0,
        width: 30.0,
        height: 40.0,
    };
    let key = |phase: Point| {
        LayerRasterCacheKey::source_content(
            Some(7),
            11,
            rect,
            (30, 40),
            RasterScale::from_scale(1.0),
            phase,
        )
    };
    assert_ne!(key(Point::default()), key(Point::new(0.5, 0.0)));
    assert_eq!(key(Point::new(0.5, 0.25)), key(Point::new(1.5, -0.75)));
    assert_eq!(key(Point::new(0.01, 0.0)), key(Point::default()));
}

#[test]
fn source_content_keys_separate_by_content_hash() {
    let rect = Rect {
        x: 1.0,
        y: 2.0,
        width: 30.0,
        height: 40.0,
    };
    let scale = RasterScale::from_scale(1.0);
    let source =
        LayerRasterCacheKey::source_content(Some(7), 11, rect, (30, 40), scale, Point::default());
    let other =
        LayerRasterCacheKey::source_content(Some(7), 12, rect, (30, 40), scale, Point::default());

    assert_ne!(source, other);
    assert_eq!(source.identity(), other.identity());
}

#[test]
fn backdrop_effect_keys_do_not_collide_with_layer_surface_keys() {
    let rect = Rect {
        x: 1.0,
        y: 2.0,
        width: 30.0,
        height: 40.0,
    };
    let scale = RasterScale::from_scale(1.0);
    let backdrop = LayerRasterCacheKey::backdrop_effect(Some(7), 11, 13, rect, (30, 40), scale);
    let source =
        LayerRasterCacheKey::source_content(Some(7), 11, rect, (30, 40), scale, Point::default());

    assert_ne!(backdrop, source);
    assert_ne!(backdrop.identity(), source.identity());
}

#[test]
fn layer_effect_keys_do_not_collide_with_backdrop_effect_keys() {
    let rect = Rect {
        x: 1.0,
        y: 2.0,
        width: 30.0,
        height: 40.0,
    };
    let scale = RasterScale::from_scale(1.0);
    let effect = LayerRasterCacheKey::layer_effect(Some(7), 11, 13, rect, (30, 40), scale);
    let backdrop = LayerRasterCacheKey::backdrop_effect(Some(7), 11, 13, rect, (30, 40), scale);
    let other_effect = LayerRasterCacheKey::layer_effect(Some(7), 11, 17, rect, (30, 40), scale);
    let other_input = LayerRasterCacheKey::layer_effect(Some(7), 12, 13, rect, (30, 40), scale);

    assert_ne!(effect, backdrop);
    assert_ne!(effect.identity(), backdrop.identity());
    assert_ne!(effect, other_effect);
    assert_ne!(effect, other_input);
    assert_eq!(effect.kind_slot(), LAYER_RASTER_CACHE_KIND_COUNT - 1);
    assert_eq!(LAYER_RASTER_CACHE_KIND_LABELS[effect.kind_slot()], "effect");
    assert!(!effect.is_source_content());
    assert!(!effect.is_scene_range());
}

#[test]
fn prefix_snapshot_keys_share_the_scene_range_partition_but_never_a_key() {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 240.0,
    };
    let scale = RasterScale::from_scale(1.0);
    let prefix = LayerRasterCacheKey::prefix_snapshot(11, 7, rect, (320, 240), scale);
    let range = LayerRasterCacheKey::scene_range(11, rect, (320, 240), scale);
    let longer = LayerRasterCacheKey::prefix_snapshot(11, 8, rect, (320, 240), scale);

    assert!(prefix.is_scene_range());
    assert_ne!(prefix, range);
    assert_ne!(prefix, longer);
    assert_eq!(prefix.identity(), None);
    assert_eq!(prefix.pixel_size(), (320, 240));
}

#[test]
fn scene_range_keys_do_not_collide_with_layer_surface_keys() {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 240.0,
    };
    let scale = RasterScale::from_scale(1.0);
    let range = LayerRasterCacheKey::scene_range(11, rect, (320, 240), scale);
    let source =
        LayerRasterCacheKey::source_content(None, 11, rect, (320, 240), scale, Point::default());

    assert_ne!(range, source);
    assert_eq!(range.identity(), None);
}
