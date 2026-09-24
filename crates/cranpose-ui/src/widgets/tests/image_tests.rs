use super::*;
use crate::layout::core::Alignment;

#[cfg(feature = "svg")]
const RED_RECT_SVG: &[u8] = br##"
    <svg xmlns="http://www.w3.org/2000/svg" width="10" height="20" viewBox="0 0 10 20">
      <rect x="0" y="0" width="10" height="20" fill="#ff0000"/>
    </svg>
"##;

#[cfg(feature = "svg")]
const TRANSPARENT_CENTER_SVG: &[u8] = br##"
    <svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" viewBox="0 0 4 4">
      <rect x="1" y="1" width="2" height="2" fill="#00ff00"/>
    </svg>
"##;

fn sample_bitmap() -> ImageBitmap {
    ImageBitmap::from_rgba8(4, 2, vec![255; 4 * 2 * 4]).expect("bitmap")
}

#[test]
fn svg_painter_ids_do_not_use_process_global_counter() {
    let source = include_str!("../image.rs");
    let svg_counter = ["static ", "NEXT_SVG_PAINTER_ID"].concat();

    assert!(
        !source.contains(&svg_counter),
        "SVG painter ids must come from retained painter identity"
    );
}

#[cfg(feature = "svg")]
fn cache_test_bitmap(width: u32) -> ImageBitmap {
    ImageBitmap::from_rgba8(width, 1, vec![255; width as usize * 4]).expect("bitmap")
}

#[cfg(feature = "svg")]
fn pixel_at(bitmap: &ImageBitmap, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * bitmap.width() + x) * 4) as usize;
    let pixels = bitmap.pixels();
    [
        pixels[offset],
        pixels[offset + 1],
        pixels[offset + 2],
        pixels[offset + 3],
    ]
}

#[test]
fn painter_reports_intrinsic_size_and_bitmap() {
    let bitmap = sample_bitmap();
    let painter = BitmapPainter(bitmap.clone());
    assert_eq!(painter.intrinsic_size(), Size::new(4.0, 2.0));
    assert_eq!(painter.bitmap(), Some(&bitmap));
}

#[test]
fn bitmap_region_painter_uses_region_as_intrinsic_size() {
    let bitmap = sample_bitmap();
    let source = Rect {
        x: 1.0,
        y: 0.0,
        width: 2.0,
        height: 1.0,
    };
    let painter = BitmapRegionPainter(bitmap.clone(), source, ImageSampling::Nearest);
    assert_eq!(painter.intrinsic_size(), Size::new(2.0, 1.0));
    assert_eq!(painter.bitmap(), Some(&bitmap));
    assert_eq!(
        painter,
        Painter::from_bitmap_region(bitmap, source, ImageSampling::Nearest)
    );
}

fn atlas_bitmap() -> ImageBitmap {
    ImageBitmap::from_rgba8(64, 64, vec![255; 64 * 64 * 4]).expect("bitmap")
}

fn source_rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn drawn_images(
    size: Size,
    draw: impl FnOnce(&mut cranpose_ui_graphics::DrawScopeDefault),
) -> Vec<(Rect, Rect)> {
    let mut scope = cranpose_ui_graphics::DrawScopeDefault::new(size);
    draw(&mut scope);
    scope
        .into_primitives()
        .into_iter()
        .filter_map(|primitive| match primitive {
            cranpose_ui_graphics::DrawPrimitive::Image { src_rect, rect, .. } => {
                Some((src_rect?, rect))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_tiled_painter_keeps_the_region_as_its_intrinsic_size() {
    let bitmap = atlas_bitmap();
    let source = source_rect(8.0, 8.0, 16.0, 4.0);
    let painter = TiledPainter(bitmap.clone(), source, ImageSampling::Nearest);
    assert_eq!(painter.intrinsic_size(), Size::new(16.0, 4.0));
    assert_eq!(painter.bitmap(), Some(&bitmap));
    assert_eq!(
        painter,
        Painter::from_bitmap_tiled(bitmap, source, ImageSampling::Nearest)
    );
}

#[test]
fn a_tiled_painter_repeats_its_region_at_its_own_size() {
    let bitmap = atlas_bitmap();
    let source = source_rect(8.0, 8.0, 16.0, 4.0);
    let drawn = drawn_images(Size::new(32.0, 8.0), |scope| {
        draw_bitmap_tiled_painter(
            scope,
            bitmap.clone(),
            source,
            1.0,
            None,
            ImageSampling::Nearest,
        );
    });

    assert_eq!(drawn.len(), 4, "two columns by two rows");
    assert!(
        drawn
            .iter()
            .all(|(src, dst)| src.width == dst.width && src.height == dst.height),
        "a tile is drawn at 1:1, never scaled"
    );
    assert!(
        drawn.iter().all(|(src, _)| src.x >= 8.0
            && src.y >= 8.0
            && src.x + src.width <= 24.0
            && src.y + src.height <= 12.0),
        "a tile never reads outside the region it was given"
    );
    assert_eq!(drawn[0].1, source_rect(0.0, 0.0, 16.0, 4.0));
    assert_eq!(drawn[3].1, source_rect(16.0, 4.0, 16.0, 4.0));
}

#[test]
fn a_tiled_painter_clips_the_last_row_and_column() {
    let bitmap = atlas_bitmap();
    let source = source_rect(0.0, 0.0, 10.0, 10.0);
    let drawn = drawn_images(Size::new(25.0, 10.0), |scope| {
        draw_bitmap_tiled_painter(
            scope,
            bitmap.clone(),
            source,
            1.0,
            None,
            ImageSampling::Nearest,
        );
    });
    assert_eq!(drawn.len(), 3);
    let (src, dst) = drawn[2];
    assert_eq!(dst, source_rect(20.0, 0.0, 5.0, 10.0));
    assert_eq!(src, source_rect(0.0, 0.0, 5.0, 10.0));
}

#[test]
fn a_nine_patch_painter_keeps_the_region_as_its_intrinsic_size() {
    let bitmap = atlas_bitmap();
    let source = source_rect(0.0, 0.0, 30.0, 30.0);
    let painter = NinePatchPainter(
        bitmap.clone(),
        source,
        NinePatchInsets::uniform(10.0),
        PatchFill::Stretch,
        PatchFill::Stretch,
        ImageSampling::Nearest,
    );
    assert_eq!(painter.intrinsic_size(), Size::new(30.0, 30.0));
    assert_eq!(painter.bitmap(), Some(&bitmap));
    assert_eq!(
        painter,
        Painter::from_nine_patch(
            bitmap,
            source,
            NinePatchInsets::uniform(10.0),
            PatchFill::Stretch,
            PatchFill::Stretch,
            ImageSampling::Nearest,
        ),
        "two painters built from the same numbers are the same painter"
    );
}

#[test]
fn a_nine_patch_painter_draws_its_corners_at_their_own_size() {
    let bitmap = atlas_bitmap();
    let drawn = drawn_images(Size::new(100.0, 60.0), |scope| {
        draw_nine_patch_painter(
            scope,
            bitmap.clone(),
            source_rect(0.0, 0.0, 30.0, 30.0),
            NinePatchInsets::uniform(10.0),
            PatchFill::Stretch,
            PatchFill::Stretch,
            1.0,
            None,
            ImageSampling::Nearest,
        );
    });

    assert_eq!(drawn.len(), 9);
    assert_eq!(
        drawn[0],
        (
            source_rect(0.0, 0.0, 10.0, 10.0),
            source_rect(0.0, 0.0, 10.0, 10.0),
        )
    );
    assert_eq!(
        drawn[8],
        (
            source_rect(20.0, 20.0, 10.0, 10.0),
            source_rect(90.0, 50.0, 10.0, 10.0),
        )
    );
    assert_eq!(
        drawn[4],
        (
            source_rect(10.0, 10.0, 10.0, 10.0),
            source_rect(10.0, 10.0, 80.0, 40.0),
        ),
        "the middle grows on both axes"
    );
}

#[test]
fn a_nine_patch_painter_reads_only_its_region_of_an_atlas() {
    let bitmap = atlas_bitmap();
    let drawn = drawn_images(Size::new(80.0, 40.0), |scope| {
        draw_nine_patch_painter(
            scope,
            bitmap.clone(),
            source_rect(32.0, 16.0, 24.0, 24.0),
            NinePatchInsets::uniform(8.0),
            PatchFill::Tile,
            PatchFill::Tile,
            1.0,
            None,
            ImageSampling::Nearest,
        );
    });

    assert!(!drawn.is_empty());
    assert!(
        drawn.iter().all(|(src, _)| src.x >= 32.0
            && src.y >= 16.0
            && src.x + src.width <= 56.0
            && src.y + src.height <= 40.0),
        "no patch may read a neighbouring sprite out of the atlas"
    );
}

#[test]
#[cfg(feature = "svg")]
fn svg_painter_reports_intrinsic_size() {
    let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
    let clone = painter.clone();
    assert_eq!(painter.intrinsic_size(), Size::new(10.0, 20.0));
    assert_eq!(painter.id(), clone.id());
    assert_eq!(Painter::from_svg(painter).bitmap(), None);
}

#[test]
#[cfg(feature = "svg")]
fn svg_painter_rasterizes_requested_dimensions() {
    let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
    let bitmap = painter
        .rasterize(Size::new(5.0, 10.0))
        .expect("rasterized svg");

    assert_eq!(bitmap.width(), 5);
    assert_eq!(bitmap.height(), 10);
    assert_eq!(pixel_at(&bitmap, 2, 5), [255, 0, 0, 255]);
}

#[test]
#[cfg(feature = "svg")]
fn svg_painter_preserves_transparency() {
    let painter = SvgPainter::from_bytes(TRANSPARENT_CENTER_SVG).expect("svg painter");
    let bitmap = painter
        .rasterize(Size::new(4.0, 4.0))
        .expect("rasterized svg");

    assert_eq!(pixel_at(&bitmap, 0, 0), [0, 0, 0, 0]);
    assert_eq!(pixel_at(&bitmap, 2, 2), [0, 255, 0, 255]);
}

#[test]
#[cfg(feature = "svg")]
fn svg_painter_reuses_cached_raster_for_same_size() {
    let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
    let first = painter
        .rasterize(Size::new(8.0, 8.0))
        .expect("first raster");
    let second = painter
        .rasterize(Size::new(8.0, 8.0))
        .expect("second raster");

    assert_eq!(first.id(), second.id());
}

#[test]
#[cfg(feature = "svg")]
fn svg_raster_cache_evicts_least_recently_used_entry() {
    let mut cache = SvgRasterCache::default();
    let keys: Vec<SvgRasterKey> = (0..SVG_RASTER_CACHE_LIMIT)
        .map(|index| SvgRasterKey {
            width: index as u32 + 1,
            height: 1,
        })
        .collect();

    for key in &keys {
        cache.insert(*key, cache_test_bitmap(key.width));
    }

    let recent = cache.get(keys[0]).expect("cached raster");
    let new_key = SvgRasterKey {
        width: SVG_RASTER_CACHE_LIMIT as u32 + 1,
        height: 1,
    };
    cache.insert(new_key, cache_test_bitmap(new_key.width));

    assert!(cache.get(keys[1]).is_none());
    assert_eq!(
        cache.get(keys[0]).expect("retained raster").id(),
        recent.id()
    );
    assert!(cache.get(new_key).is_some());
}

#[test]
#[cfg(feature = "svg")]
fn svg_painter_rasterizes_distinct_sizes_separately() {
    let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
    let small = painter
        .rasterize(Size::new(8.0, 8.0))
        .expect("small raster");
    let large = painter
        .rasterize(Size::new(16.0, 16.0))
        .expect("large raster");

    assert_ne!(small.id(), large.id());
    assert_eq!(large.width(), 16);
    assert_eq!(large.height(), 16);
}

#[test]
#[cfg(feature = "svg")]
fn svg_painter_cache_recovers_after_poison() {
    let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
    let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = painter
            .inner
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        panic!("poison svg raster cache for recovery test");
    }));

    assert!(poison_result.is_err());

    let bitmap = painter
        .rasterize(Size::new(8.0, 8.0))
        .expect("svg raster cache should recover after poisoning");
    assert_eq!(bitmap.width(), 8);
    assert_eq!(bitmap.height(), 8);
}

#[test]
fn svg_draw_density_has_no_outside_context_fallback() {
    let source = include_str!("../image.rs");
    let fallback_call = ["try_current", "_density()", ".unwrap_or"].concat();
    assert!(
        !source.contains(&fallback_call),
        "SVG image drawing must use the active AppContext density"
    );
}

#[test]
#[cfg(feature = "svg")]
fn svg_painter_rejects_invalid_bytes() {
    let err = SvgPainter::from_bytes(b"not svg").expect_err("invalid svg");
    assert!(matches!(err, SvgPainterError::Parse(_)));
}

#[test]
#[cfg(not(feature = "svg"))]
fn svg_painter_reports_disabled_feature_without_resvg() {
    let err = SvgPainter::from_bytes(b"not svg").expect_err("svg feature disabled");
    assert!(matches!(err, SvgPainterError::SvgFeatureDisabled));
}

#[test]
fn fit_keeps_aspect_ratio() {
    let src = Size::new(200.0, 100.0);
    let dst = Size::new(300.0, 300.0);
    let result = ContentScale::Fit.scaled_size(src, dst);
    assert_eq!(result, Size::new(300.0, 150.0));
}

#[test]
fn crop_fills_bounds() {
    let src = Size::new(200.0, 100.0);
    let dst = Size::new(300.0, 300.0);
    let result = ContentScale::Crop.scaled_size(src, dst);
    assert_eq!(result, Size::new(600.0, 300.0));
}

#[test]
fn destination_rect_aligns_center() {
    let src = Size::new(200.0, 100.0);
    let dst = Size::new(300.0, 300.0);
    let rect = destination_rect(src, dst, Alignment::CENTER, ContentScale::Fit);
    assert_eq!(
        rect,
        Rect {
            x: 0.0,
            y: 75.0,
            width: 300.0,
            height: 150.0,
        }
    );
}

#[test]
fn crop_destination_clip_maps_centered_wide_source() {
    let src = Size::new(200.0, 100.0);
    let dst = Size::new(100.0, 100.0);
    let (dst_rect, clipped_dst_rect) =
        image_destination_clip(src, dst, Alignment::CENTER, ContentScale::Crop)
            .expect("destination clip");
    let rect = map_destination_clip_to_source(Rect::from_size(src), dst_rect, clipped_dst_rect)
        .expect("source clip");
    assert_eq!(
        rect,
        Rect {
            x: 50.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }
    );
}

#[test]
fn crop_destination_clip_honors_start_alignment() {
    let src = Size::new(200.0, 100.0);
    let dst = Size::new(100.0, 100.0);
    let (dst_rect, clipped_dst_rect) =
        image_destination_clip(src, dst, Alignment::TOP_START, ContentScale::Crop)
            .expect("destination clip");
    let rect = map_destination_clip_to_source(Rect::from_size(src), dst_rect, clipped_dst_rect)
        .expect("source clip");
    assert_eq!(
        rect,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }
    );
}

fn approx_eq(left: f32, right: f32) {
    assert!((left - right).abs() < 1e-4, "left={left}, right={right}");
}

#[test]
fn map_destination_clip_to_source_scales_proportionally() {
    let src = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    let dst = Rect {
        x: -50.0,
        y: 0.0,
        width: 200.0,
        height: 100.0,
    };
    let clipped_dst = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    let mapped = map_destination_clip_to_source(src, dst, clipped_dst).expect("mapped");
    approx_eq(mapped.x, 25.0);
    approx_eq(mapped.y, 0.0);
    approx_eq(mapped.width, 50.0);
    approx_eq(mapped.height, 100.0);
}

#[test]
fn map_destination_clip_to_source_returns_full_source_without_clipping() {
    let src = Rect {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 80.0,
    };
    let dst = Rect {
        x: 10.0,
        y: 5.0,
        width: 60.0,
        height: 40.0,
    };
    let mapped = map_destination_clip_to_source(src, dst, dst).expect("mapped");
    approx_eq(mapped.x, src.x);
    approx_eq(mapped.y, src.y);
    approx_eq(mapped.width, src.width);
    approx_eq(mapped.height, src.height);
}

fn measure_image(intrinsic: Size, constraints: Constraints) -> Size {
    let policy = ImageMeasurePolicy {
        intrinsic_size: intrinsic,
    };
    let scope = crate::density::DensityMeasureScope::new(crate::density::Density::default());
    policy.measure(&scope, &[], constraints).size
}

#[test]
fn image_measure_unconstrained() {
    let size = measure_image(
        Size::new(800.0, 600.0),
        Constraints::loose(f32::INFINITY, f32::INFINITY),
    );
    assert_eq!(size, Size::new(800.0, 600.0));
}

#[test]
fn image_measure_width_constrained_preserves_aspect_ratio() {
    let size = measure_image(
        Size::new(800.0, 600.0),
        Constraints::loose(400.0, f32::INFINITY),
    );
    assert_eq!(size, Size::new(400.0, 300.0));
}

#[test]
fn image_measure_height_constrained_preserves_aspect_ratio() {
    let size = measure_image(
        Size::new(800.0, 600.0),
        Constraints::loose(f32::INFINITY, 300.0),
    );
    assert_eq!(size, Size::new(400.0, 300.0));
}

#[test]
fn image_measure_both_constrained_uses_smaller_factor() {
    let size = measure_image(Size::new(800.0, 600.0), Constraints::loose(200.0, 400.0));
    assert_eq!(size, Size::new(200.0, 150.0));
}

#[test]
fn image_measure_fits_within_constraints() {
    let size = measure_image(Size::new(200.0, 100.0), Constraints::loose(400.0, 400.0));
    assert_eq!(size, Size::new(200.0, 100.0));
}

#[test]
fn image_measure_zero_intrinsic() {
    let size = measure_image(Size::ZERO, Constraints::loose(400.0, 400.0));
    assert_eq!(size, Size::new(0.0, 0.0));
}
