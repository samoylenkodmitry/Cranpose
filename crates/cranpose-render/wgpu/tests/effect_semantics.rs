mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{
    CachePolicy, DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
    ProjectiveTransform, RenderGraph, RenderNode, TextPrimitiveNode,
};
use cranpose_render_common::image_compare::{
    image_difference_stats, normalize_rgba_region, sample_pixel,
};
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::text::{
    AnnotatedString, FontStyle, FontWeight, SpanStyle, TextDecoration, TextDrawStyle, TextStyle,
    TextUnit,
};
use cranpose_ui::TextLayoutOptions;
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, CompositingStrategy, DrawPrimitive, GraphicsLayer, ImageBitmap,
    ImageSampling, Point, Rect, RenderEffect, ShadowPrimitive,
};

const FRAME_WIDTH: u32 = 128;
const FRAME_HEIGHT: u32 = 96;
const ROOT_SCALE_TEST_SCALE: f32 = 2.0;
const FRACTIONAL_ROOT_SCALE: f32 = 130.0 / 96.0;
const DEVICE_SNAP_SUBPIXEL_STEPS: f64 = 16.0;
const ALPHA_LAYER_SIZE: (u32, u32) = (48, 24);
const BLUR_LAYER_SIZE: (u32, u32) = (28, 28);
const BACKDROP_LAYER_SIZE: (u32, u32) = (24, 20);
const TRANSLATED_BACKDROP_SUBTREE_SIZE: (u32, u32) = (56, 32);
const TRANSLATED_BACKDROP_COMPARE_INSET: u32 = 2;
const TRANSLATED_BACKDROP_PIXEL_TOLERANCE: u32 = 24;
const TRANSLATED_BACKDROP_MAX_DIFFERING_PIXELS: u32 = 120;
const TRANSLATED_BACKDROP_MAX_PIXEL_DIFFERENCE: u32 = 360;
const SHOWCASE_CARD_PIXEL_TOLERANCE: u32 = 1;
const SHOWCASE_CARD_MAX_DIFFERING_PIXELS: u32 = 4;
const SHOWCASE_CARD_MAX_PIXEL_DIFFERENCE: u32 = 3;
const TRANSLATED_TEXT_LOCAL_SIZE: (u32, u32) = (48, 24);
const MULTISPAN_FRAME_WIDTH: u32 = 420;
const MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE: (u32, u32) = (340, 40);
const TRANSLATED_TEXT_PIXEL_TOLERANCE: u32 = 24;
const TRANSLATED_TEXT_MAX_DIFFERING_PIXELS: u32 = 240;
const TRANSLATED_TEXT_MAX_PIXEL_DIFFERENCE: u32 = 420;
const TRANSLATED_THIN_SHAPE_LOCAL_SIZE: (u32, u32) = (40, 18);

fn stats_used_bounded_layer_surface(stats: &RenderStatsSnapshot) -> bool {
    stats.isolated_layer_renders > 0 || stats.layer_cache_hits > 0
}

#[test]
fn subtree_alpha_capture_preserves_group_opacity_and_uses_bounded_surface() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping subtree alpha capture assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(alpha_fixture());
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("alpha capture should succeed");
    let stats = renderer.last_frame_stats().expect("alpha frame stats");

    let left = rgba(&frame, 34, 38);
    let overlap = rgba(&frame, 48, 38);
    let right = rgba(&frame, 62, 38);
    let background = rgba(&frame, 8, 8);

    assert_dark(background, "background");
    assert!(
        left[0] > 80 && overlap[0] > 80 && right[0] > 80,
        "alpha-isolated subtree should remain visibly bright inside the layer: left={left:?} overlap={overlap:?} right={right:?}"
    );
    assert_channel_close(left[0], overlap[0], 8, "left vs overlap red");
    assert_channel_close(overlap[0], right[0], 8, "overlap vs right red");
    assert_eq!(
        stats.blur_passes, 0,
        "alpha-only layer should not blur: {stats:?}"
    );
    assert_eq!(
        stats.effect_applies, 0,
        "alpha-only layer should not run render effects: {stats:?}"
    );
    assert_eq!(
        stats.isolated_layer_renders, 1,
        "alpha capture should isolate only the alpha subtree now that the root renders direct: {stats:?}"
    );
    assert_local_surface_stats(&frame, stats, ALPHA_LAYER_SIZE, 1, "alpha");
}

#[test]
fn offscreen_alpha_layer_preserves_dstout_cutout_over_underlay() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping DstOut alpha isolation assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(dstout_alpha_fixture());
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("DstOut alpha capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("DstOut alpha frame stats");

    let cutout = rgba(&frame, 44, 32);
    let faded_content = rgba(&frame, 44, 58);

    assert!(
        cutout[0] > cutout[1].saturating_add(50) && cutout[1] > cutout[2] && cutout[2] < 130,
        "DstOut cutout should reveal the red underlay, got {cutout:?}"
    );
    assert!(
        faded_content[1] > 80 && faded_content[2] > 80,
        "uncut offscreen content should survive group alpha composite, got {faded_content:?}"
    );
    assert!(
        stats.isolated_layer_renders > 0,
        "DstOut + Offscreen should use an isolated layer surface: {stats:?}"
    );
}

#[test]
fn repeated_gradient_dstout_alpha_layer_reveals_underlay() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping gradient DstOut alpha isolation assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(gradient_dstout_alpha_fixture());
    for _ in 0..3 {
        let frame = renderer
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("gradient DstOut alpha capture should succeed");
        let underlay_cutout = rgba(&frame, 36, 30);
        let overlay_content = rgba(&frame, 36, 66);
        assert!(
            underlay_cutout[0] > 180
                && underlay_cutout[1] > 140
                && underlay_cutout[2] < 150
                && underlay_cutout[0] > overlay_content[0].saturating_add(20),
            "gradient DstOut cutout should reveal the warm underlay, cutout={underlay_cutout:?} overlay={overlay_content:?}"
        );
    }

    let stats = renderer
        .last_frame_stats()
        .expect("gradient DstOut alpha frame stats");
    assert!(
        stats.isolated_layer_renders > 0 || stats.layer_cache_hits > 0,
        "gradient DstOut + Offscreen should use an isolated layer surface: {stats:?}"
    );
}

#[test]
fn root_composite_respects_root_scale_on_presented_surface() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping root-scale capture assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(root_scale_fixture());
    let frame = renderer
        .capture_frame_with_scale(FRAME_WIDTH, FRAME_HEIGHT, ROOT_SCALE_TEST_SCALE)
        .expect("root-scale capture should succeed");

    assert_red(
        rgba(&frame, FRAME_WIDTH / 4, FRAME_HEIGHT / 4),
        "root-scale test should color the upper-left quadrant",
    );
    assert_red(
        rgba(&frame, FRAME_WIDTH - 8, FRAME_HEIGHT - 8),
        "root-scale test should scale the root surface to the full physical frame",
    );
}

#[test]
fn full_logical_root_capture_respects_explicit_root_scale() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping full logical root-scale assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let logical_width = 96;
    let logical_height = 72;
    let root_scale = 2.0;
    renderer.scene_mut().graph = Some(full_logical_root_scale_fixture(
        logical_width as f32,
        logical_height as f32,
    ));
    let frame =
        capture_logical_frame_with_scale(&mut renderer, logical_width, logical_height, root_scale);

    assert_red(
        rgba(&frame, 36, 32),
        "scaled full logical root should render its upper-left content",
    );
    assert_blue(
        rgba(&frame, frame.width - 10, frame.height - 10),
        "scaled full logical root should render its lower-right content into the physical target",
    );
}

#[test]
fn translation_only_layers_render_without_nested_offscreens() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping translation-only isolation assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(translation_only_fixture());
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translation-only capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("translation-only frame stats");

    assert_red(
        rgba(&frame, 26, 26),
        "translation-only nested layers should still draw at the composed position",
    );
    assert_eq!(
        stats.isolated_layer_renders, 0,
        "translation-only nested layers should render directly into the root target without isolated surfaces: {stats:?}"
    );
    assert_eq!(
        stats.offscreen_acquires, 0,
        "translation-only nested layers should not acquire offscreen targets: {stats:?}"
    );
    assert_eq!(
        stats.composite_passes, 0,
        "translation-only nested layers should not run composite passes: {stats:?}"
    );
}

#[test]
fn translation_only_wrapper_with_text_collapses_into_root_target() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping wrapper-collapse isolation assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(translation_only_wrapper_with_text_fixture(Point::new(
        12.0, 14.0,
    )));
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("wrapper text capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("wrapper text frame stats");

    let rect_pixel = rgba(&frame, 29, 31);
    assert!(
        rect_pixel[0] >= 40 || rect_pixel[1] >= 40 || rect_pixel[2] >= 40,
        "text leaf fixture should draw visible content at the composed position, got {rect_pixel:?}"
    );
    assert_eq!(
        stats.isolated_layer_renders, 0,
        "translation-only wrapper and plain text leaf should both render directly into the root target: {stats:?}"
    );
    assert_eq!(
        stats.offscreen_acquires, 0,
        "translation-only wrapper and plain text leaf should not acquire offscreen targets: {stats:?}"
    );
    assert_eq!(
        stats.composite_passes, 0,
        "translation-only wrapper and plain text leaf should not run composite passes: {stats:?}"
    );
}

#[test]
fn translated_text_wrapper_with_text_stays_on_direct_path_under_fractional_motion() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping translated text-wrapper assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let base_translation = Point::new(12.0, 14.0);
    renderer.scene_mut().graph = Some(translation_only_wrapper_with_text_fixture(base_translation));
    let base_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated text-wrapper base capture should succeed");
    let base_stats = renderer
        .last_frame_stats()
        .expect("translated text-wrapper base frame stats");

    let moved_translation = Point::new(12.35, 14.65);
    renderer.scene_mut().graph = Some(translation_only_wrapper_with_text_fixture(
        moved_translation,
    ));
    let moved_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated text-wrapper moved capture should succeed");
    let moved_stats = renderer
        .last_frame_stats()
        .expect("translated text-wrapper moved frame stats");

    assert_eq!(
        base_stats.isolated_layer_renders, 0,
        "base frame should render the wrapper and text leaf directly into the root target without isolated surfaces: {base_stats:?}"
    );
    assert_eq!(
        moved_stats.isolated_layer_renders, 0,
        "moved frame should keep the translation-only text subtree on the direct path without isolated surfaces: {moved_stats:?}"
    );
    assert_eq!(
        base_stats.offscreen_acquires, 0,
        "base frame should not acquire offscreen targets: {base_stats:?}"
    );
    assert_eq!(
        moved_stats.offscreen_acquires, 0,
        "moved frame should not acquire offscreen targets: {moved_stats:?}"
    );

    let base_pixel = rgba(
        &base_frame,
        (base_translation.x + 16.0) as u32,
        (base_translation.y + 17.0) as u32,
    );
    let moved_pixel = rgba(
        &moved_frame,
        (moved_translation.x + 16.0) as u32,
        (moved_translation.y + 17.0) as u32,
    );
    assert!(
        base_pixel[0] >= 40 || base_pixel[1] >= 40 || base_pixel[2] >= 40,
        "base frame should draw visible wrapper/text content at the composed position, got {base_pixel:?}"
    );
    assert!(
        moved_pixel[0] >= 40 || moved_pixel[1] >= 40 || moved_pixel[2] >= 40,
        "moved frame should draw visible wrapper/text content at the composed position, got {moved_pixel:?}"
    );
}

#[test]
fn translated_text_wrapper_preserves_local_picture_under_fractional_motion_on_each_axis() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping translated text local-picture assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let base_translation = Point::new(12.0, 14.0);
    let mut base_graph = translation_only_wrapper_with_plain_text_only_fixture(base_translation);
    mark_translated_text_wrapper(&mut base_graph);
    renderer.scene_mut().graph = Some(base_graph);
    let base_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated text base capture should succeed");

    let horizontal_translation = Point::new(12.35, 14.0);
    let mut horizontal_graph =
        translation_only_wrapper_with_plain_text_only_fixture(horizontal_translation);
    mark_translated_text_wrapper(&mut horizontal_graph);
    renderer.scene_mut().graph = Some(horizontal_graph);
    let horizontal_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated text horizontal capture should succeed");

    let vertical_translation = Point::new(12.0, 14.65);
    let mut vertical_graph =
        translation_only_wrapper_with_plain_text_only_fixture(vertical_translation);
    mark_translated_text_wrapper(&mut vertical_graph);
    renderer.scene_mut().graph = Some(vertical_graph);
    let vertical_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated text vertical capture should succeed");

    let base_normalized = normalize_translated_text_region(&base_frame, base_translation);
    let horizontal_normalized =
        normalize_translated_text_region(&horizontal_frame, horizontal_translation);
    let vertical_normalized =
        normalize_translated_text_region(&vertical_frame, vertical_translation);

    assert_translated_text_local_picture_stable(
        &base_normalized,
        &horizontal_normalized,
        "horizontal",
    );
    assert_translated_text_local_picture_stable(&base_normalized, &vertical_normalized, "vertical");
}

#[test]
fn translated_thin_shape_wrapper_uses_bounded_local_surface_under_fractional_motion() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping translated thin-shape assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let base_translation = Point::new(12.0, 14.0);
    let mut base_graph = translation_only_wrapper_with_thin_shapes_fixture(base_translation);
    mark_translated_shape_wrapper(&mut base_graph);
    renderer.scene_mut().graph = Some(base_graph);
    let base_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated thin-shape base capture should succeed");
    let base_stats = renderer
        .last_frame_stats()
        .expect("translated thin-shape base frame stats");

    let horizontal_translation = Point::new(12.35, 14.0);
    let mut horizontal_graph =
        translation_only_wrapper_with_thin_shapes_fixture(horizontal_translation);
    mark_translated_shape_wrapper(&mut horizontal_graph);
    renderer.scene_mut().graph = Some(horizontal_graph);
    let horizontal_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated thin-shape horizontal capture should succeed");
    let horizontal_stats = renderer
        .last_frame_stats()
        .expect("translated thin-shape horizontal frame stats");

    let vertical_translation = Point::new(12.0, 14.65);
    let mut vertical_graph =
        translation_only_wrapper_with_thin_shapes_fixture(vertical_translation);
    mark_translated_shape_wrapper(&mut vertical_graph);
    renderer.scene_mut().graph = Some(vertical_graph);
    let vertical_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated thin-shape vertical capture should succeed");
    let vertical_stats = renderer
        .last_frame_stats()
        .expect("translated thin-shape vertical frame stats");

    let bright_region = |frame: &CapturedFrame, translation: Point| {
        bright_pixel_count(
            frame,
            Rect {
                x: translation.x + 12.0,
                y: translation.y + 8.0,
                width: 24.0,
                height: 10.0,
            },
            120,
        )
    };
    assert!(
        stats_used_bounded_layer_surface(&base_stats)
            && stats_used_bounded_layer_surface(&horizontal_stats)
            && stats_used_bounded_layer_surface(&vertical_stats),
        "translated thin-shape fixture should render through bounded local surfaces: base={base_stats:?} horizontal={horizontal_stats:?} vertical={vertical_stats:?}"
    );
    assert!(
        bright_region(&base_frame, base_translation) >= 60,
        "translated thin-shape base frame should keep visible thin bars"
    );
    assert!(
        bright_region(&horizontal_frame, horizontal_translation) >= 60,
        "translated thin-shape horizontal frame should keep visible thin bars"
    );
    assert!(
        bright_region(&vertical_frame, vertical_translation) >= 60,
        "translated thin-shape vertical frame should keep visible thin bars"
    );
}

#[test]
fn translation_only_wrapper_with_underlined_text_stays_on_direct_path() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping underlined text-wrapper assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let translation = Point::new(12.0, 14.0);
    renderer.scene_mut().graph = Some(translation_only_wrapper_with_underlined_text_fixture(
        translation,
    ));
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("underlined text wrapper capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("underlined text wrapper frame stats");

    let text_region_bright = bright_pixel_count(
        &frame,
        Rect {
            x: translation.x + 12.0,
            y: translation.y + 8.0,
            width: 32.0,
            height: 16.0,
        },
        120,
    );
    let underline_region_bright = bright_pixel_count(
        &frame,
        Rect {
            x: translation.x + 12.0,
            y: translation.y + 20.0,
            width: 34.0,
            height: 6.0,
        },
        160,
    );
    assert!(
        text_region_bright >= 20,
        "underlined text fixture should draw visible text content at the composed position, bright_pixels={text_region_bright}"
    );
    assert!(
        underline_region_bright >= 6,
        "underlined text fixture should draw a visible underline on the direct path, bright_pixels={underline_region_bright}"
    );
    assert_eq!(
        stats.isolated_layer_renders, 0,
        "translation-only wrapper and underlined text leaf should render directly into the root target without isolated surfaces: {stats:?}"
    );
    assert_eq!(
        stats.offscreen_acquires, 0,
        "translation-only wrapper and underlined text leaf should not acquire offscreen targets: {stats:?}"
    );
    assert_eq!(
        stats.composite_passes, 0,
        "translation-only wrapper and underlined text leaf should not run composite passes: {stats:?}"
    );
}

#[test]
fn translated_content_wrapper_with_decorated_shadow_text_uses_bounded_local_surface() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping decorated text-wrapper assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let translation = Point::new(12.0, 14.0);
    let mut graph = translation_only_wrapper_with_decorated_shadow_text_fixture(translation);
    let Some(RenderNode::Layer(wrapper)) = graph.root.children.get_mut(1) else {
        panic!("expected decorated text wrapper layer");
    };
    wrapper.translated_content_context = true;
    let Some(RenderNode::Layer(text_leaf)) = wrapper.children.get_mut(0) else {
        panic!("expected decorated text leaf layer");
    };
    text_leaf.translated_content_context = true;
    renderer.scene_mut().graph = Some(graph);
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("decorated text wrapper capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("decorated text wrapper frame stats");

    let text_region_bright = bright_pixel_count(
        &frame,
        Rect {
            x: translation.x + 12.0,
            y: translation.y + 8.0,
            width: 32.0,
            height: 16.0,
        },
        120,
    );
    let decoration_region_bright = bright_pixel_count(
        &frame,
        Rect {
            x: translation.x + 12.0,
            y: translation.y + 18.0,
            width: 34.0,
            height: 8.0,
        },
        120,
    );
    let background_region_bright = bright_pixel_count(
        &frame,
        Rect {
            x: translation.x + 11.0,
            y: translation.y + 7.0,
            width: 36.0,
            height: 18.0,
        },
        40,
    );
    assert!(
        text_region_bright >= 20,
        "decorated text fixture should draw visible text content at the composed position, bright_pixels={text_region_bright}"
    );
    assert!(
        decoration_region_bright >= 6,
        "decorated text fixture should draw visible decoration content on the direct path, bright_pixels={decoration_region_bright}"
    );
    assert!(
        background_region_bright >= 40,
        "decorated text fixture should draw a visible background region on the direct path, bright_pixels={background_region_bright}"
    );
    assert!(
        stats.isolated_layer_renders >= 1,
        "translated-content decorated-shadow text should render through a bounded local surface: {stats:?}"
    );
    assert!(
        stats.blur_passes >= 1,
        "decorated text shadow should still use the bounded blur helper path: {stats:?}"
    );
    let isolated = stats.top_isolated_layers().collect::<Vec<_>>();
    assert!(
        !isolated.is_empty(),
        "translated-content decorated text should report an isolated text surface in stats: {stats:?}"
    );
}

#[test]
fn repeated_translated_drop_shadow_layers_stay_on_direct_path() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping repeated drop-shadow batching assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    const CARD_COUNT: usize = 8;
    renderer.scene_mut().graph = Some(repeated_drop_shadow_layers_fixture(CARD_COUNT));
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("repeated drop-shadow capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("repeated drop-shadow frame stats");

    assert!(
        bright_pixel_count(
            &frame,
            Rect {
                x: 8.0,
                y: 12.0,
                width: 112.0,
                height: 68.0,
            },
            80,
        ) > 600,
        "shadowed cards should remain visible after direct-path flattening"
    );
    assert_eq!(
        stats.isolated_layer_renders, 0,
        "drop-shadow-only translated layers should flatten into the parent target instead of isolating every card: {stats:?}"
    );
    assert!(
        stats.offscreen_acquires <= (CARD_COUNT as u32) * 2,
        "one blurred drop shadow should need only source and destination shadow targets: {stats:?}"
    );
    assert!(
        stats.composite_passes <= CARD_COUNT as u32,
        "drop shadows should composite once per card and avoid a second layer-surface composite: {stats:?}"
    );
    assert!(
        stats.submits <= (CARD_COUNT as u32) * 2 + 1,
        "single-shape blurred drop shadows should submit once per shadow plus surrounding direct draw chunks: {stats:?}"
    );
}

#[test]
fn shadow_blur_composite_render_submits_one_frame_command_buffer() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping shadow frame-submit assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(repeated_drop_shadow_layers_fixture(4));
    let stats = renderer
        .render_current_scene_to_texture(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("shadow render should succeed");

    assert!(
        stats.blur_passes >= 1,
        "drop-shadow frame should encode blur passes: {stats:?}"
    );
    assert!(
        stats.composite_passes >= 1,
        "drop-shadow frame should encode composite passes: {stats:?}"
    );
    assert_eq!(
        stats.encoder_count, 1,
        "drop-shadow frame should use one frame command encoder: {stats:?}"
    );
    assert_eq!(
        stats.submit_count, 1,
        "drop-shadow frame should submit exactly once outside explicit readback paths: {stats:?}"
    );
}

#[test]
fn translated_multispan_showcase_text_stays_exact_at_fractional_root_scale() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping fractional root-scale multispan text assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let base_translation = Point::new(12.4, 14.4);
    let mut base_graph =
        translation_only_wrapper_with_multispan_showcase_text_fixture(base_translation);
    mark_translated_text_wrapper(&mut base_graph);
    renderer.scene_mut().graph = Some(base_graph);
    let base_frame = capture_logical_frame_with_scale(
        &mut renderer,
        MULTISPAN_FRAME_WIDTH,
        FRAME_HEIGHT,
        FRACTIONAL_ROOT_SCALE,
    );

    let moved_translation = Point::new(12.4, 13.4);
    let mut moved_graph =
        translation_only_wrapper_with_multispan_showcase_text_fixture(moved_translation);
    mark_translated_text_wrapper(&mut moved_graph);
    renderer.scene_mut().graph = Some(moved_graph);
    let moved_frame = capture_logical_frame_with_scale(
        &mut renderer,
        MULTISPAN_FRAME_WIDTH,
        FRAME_HEIGHT,
        FRACTIONAL_ROOT_SCALE,
    );

    let width = scaled_dimension(MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.0, FRACTIONAL_ROOT_SCALE);
    let height = scaled_dimension(MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.1, FRACTIONAL_ROOT_SCALE);
    let base_normalized = normalize_translated_region_at_scale(
        &base_frame,
        base_translation,
        MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE,
        FRACTIONAL_ROOT_SCALE,
    );
    let moved_normalized = normalize_translated_region_at_scale(
        &moved_frame,
        moved_translation,
        MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE,
        FRACTIONAL_ROOT_SCALE,
    );

    assert_exact_normalized_match(
        "fractional root-scale multispan showcase text",
        &base_normalized,
        &moved_normalized,
        width,
        height,
    );
}

#[test]
fn translated_showcase_card_surface_stays_exact_at_fractional_root_scale() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping fractional root-scale showcase card assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let base_translation = Point::new(12.4, 14.4);
    let mut base_graph = translation_only_wrapper_with_showcase_card_fixture(base_translation);
    mark_translated_text_wrapper(&mut base_graph);
    renderer.scene_mut().graph = Some(base_graph);
    let base_frame = capture_logical_frame_with_scale(
        &mut renderer,
        MULTISPAN_FRAME_WIDTH,
        FRAME_HEIGHT,
        FRACTIONAL_ROOT_SCALE,
    );

    let moved_translation = Point::new(12.4, 13.4);
    let mut moved_graph = translation_only_wrapper_with_showcase_card_fixture(moved_translation);
    mark_translated_text_wrapper(&mut moved_graph);
    renderer.scene_mut().graph = Some(moved_graph);
    let moved_frame = capture_logical_frame_with_scale(
        &mut renderer,
        MULTISPAN_FRAME_WIDTH,
        FRAME_HEIGHT,
        FRACTIONAL_ROOT_SCALE,
    );

    let width = scaled_dimension(MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.0, FRACTIONAL_ROOT_SCALE);
    let height = scaled_dimension(MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.1, FRACTIONAL_ROOT_SCALE);
    let base_normalized = normalize_translated_region_at_scale(
        &base_frame,
        base_translation,
        MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE,
        FRACTIONAL_ROOT_SCALE,
    );
    let moved_normalized = normalize_translated_region_at_scale(
        &moved_frame,
        moved_translation,
        MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE,
        FRACTIONAL_ROOT_SCALE,
    );

    assert_nearly_exact_normalized_match(
        "fractional root-scale showcase card surface",
        &base_normalized,
        &moved_normalized,
        (width, height),
        NormalizedMatchTolerance {
            pixel_tolerance: SHOWCASE_CARD_PIXEL_TOLERANCE,
            max_differing_pixels: SHOWCASE_CARD_MAX_DIFFERING_PIXELS,
            max_pixel_difference: SHOWCASE_CARD_MAX_PIXEL_DIFFERENCE,
        },
    );
}

#[test]
fn static_alpha_surface_keeps_nested_text_and_icon_crisp_at_fractional_offset() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping static alpha surface crispness assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let base_translation = Point::new(20.0, 18.0);
    renderer.scene_mut().graph = Some(alpha_icon_text_surface_fixture(base_translation));
    let base_frame = renderer
        .capture_frame(150, 86)
        .expect("base alpha surface capture should succeed");
    let base_stats = renderer.last_frame_stats().expect("base frame stats");
    assert_eq!(
        base_stats.isolated_layer_renders, 1,
        "alpha card should isolate exactly its local surface: {base_stats:?}"
    );

    let fractional_translation = Point::new(20.4, 18.4);
    renderer.scene_mut().graph = Some(alpha_icon_text_surface_fixture(fractional_translation));
    let fractional_frame = renderer
        .capture_frame(150, 86)
        .expect("fractional alpha surface capture should succeed");

    let size = (94, 46);
    let base = normalize_translated_region_at_scale(&base_frame, base_translation, size, 1.0);
    let fractional =
        normalize_translated_region_at_scale(&fractional_frame, fractional_translation, size, 1.0);

    assert_exact_normalized_match(
        "static translucent nested icon/text surface",
        &base,
        &fractional,
        size.0,
        size.1,
    );
}

#[test]
fn translated_multispan_showcase_text_with_padding_stays_exact_at_fractional_root_scale() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping fractional root-scale padded multispan text assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let base_translation = Point::new(12.4, 14.4);
    let mut base_graph =
        translation_only_wrapper_with_padded_multispan_showcase_text_fixture(base_translation);
    mark_translated_text_wrapper(&mut base_graph);
    renderer.scene_mut().graph = Some(base_graph);
    let base_frame = capture_logical_frame_with_scale(
        &mut renderer,
        MULTISPAN_FRAME_WIDTH,
        FRAME_HEIGHT,
        FRACTIONAL_ROOT_SCALE,
    );
    let base_stats = renderer.last_frame_stats().expect("base frame stats");

    let moved_translation = Point::new(12.4, 13.4);
    let mut moved_graph =
        translation_only_wrapper_with_padded_multispan_showcase_text_fixture(moved_translation);
    mark_translated_text_wrapper(&mut moved_graph);
    renderer.scene_mut().graph = Some(moved_graph);
    let moved_frame = capture_logical_frame_with_scale(
        &mut renderer,
        MULTISPAN_FRAME_WIDTH,
        FRAME_HEIGHT,
        FRACTIONAL_ROOT_SCALE,
    );
    let moved_stats = renderer.last_frame_stats().expect("moved frame stats");
    assert!(
        stats_used_bounded_layer_surface(&base_stats)
            && stats_used_bounded_layer_surface(&moved_stats),
        "padded multispan translated text should use bounded layer surfaces: base={base_stats:?} moved={moved_stats:?}"
    );

    let width = scaled_dimension(MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.0, FRACTIONAL_ROOT_SCALE);
    let height = scaled_dimension(MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.1, FRACTIONAL_ROOT_SCALE);
    let base_normalized = normalize_translated_region_at_scale(
        &base_frame,
        base_translation,
        MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE,
        FRACTIONAL_ROOT_SCALE,
    );
    let moved_normalized = normalize_translated_region_at_scale(
        &moved_frame,
        moved_translation,
        MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE,
        FRACTIONAL_ROOT_SCALE,
    );

    assert_exact_normalized_match(
        "fractional root-scale padded multispan showcase text",
        &base_normalized,
        &moved_normalized,
        width,
        height,
    );
}

#[test]
fn bounded_blur_capture_stays_inside_layer_bounds() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping bounded blur capture assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(unblurred_fixture());
    let baseline = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("unblurred baseline capture should succeed");

    renderer.scene_mut().graph = Some(blur_fixture());
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("blur capture should succeed");
    let stats = renderer.last_frame_stats().expect("blur frame stats");

    let center = rgba(&frame, 58, 40);
    let blurred_edge = rgba(&frame, 54, 40);
    let baseline_edge = rgba(&baseline, 54, 40);
    let outside = rgba(&frame, 43, 40);
    let corner = rgba(&frame, 8, 8);

    assert!(
        center[0] > 150,
        "blurred center should stay bright: center={center:?}"
    );
    assert!(
        (blurred_edge[0] as u16) + 30 < (baseline_edge[0] as u16),
        "blurred edge should soften relative to the unblurred baseline: blurred={blurred_edge:?} baseline={baseline_edge:?}"
    );
    assert_dark(
        outside,
        "pixel outside the bounded blur layer should stay untouched",
    );
    assert_dark(corner, "far background");
    assert!(
        stats.blur_passes >= 1,
        "bounded blur should execute at least one blur pass: {stats:?}"
    );
    assert_eq!(
        stats.isolated_layer_renders, 1,
        "bounded blur capture should isolate only the blur child now that the root renders direct: {stats:?}"
    );
    assert_local_surface_stats(&frame, stats, BLUR_LAYER_SIZE, 3, "blur");
}

#[test]
fn bounded_backdrop_capture_only_filters_local_snapshot() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping bounded backdrop capture assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(backdrop_fixture());
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("backdrop capture should succeed");
    let stats = renderer.last_frame_stats().expect("backdrop frame stats");

    let outside_left = rgba(&frame, 58, 20);
    let inside_mixed = rgba(&frame, 58, 40);
    let outside_right = rgba(&frame, 88, 40);

    assert_red(
        outside_left,
        "outside backdrop region should keep the original red background",
    );
    assert_blue(
        outside_right,
        "outside backdrop region on the blue side should stay unchanged",
    );
    assert!(
        inside_mixed[2] >= outside_left[2].saturating_add(40),
        "backdrop blur inside the layer should pick up blue from the neighboring backdrop: outside={outside_left:?} inside={inside_mixed:?}"
    );
    assert!(
        stats.blur_passes >= 1,
        "bounded backdrop blur should execute blur passes: {stats:?}"
    );
    assert_eq!(
        stats.isolated_layer_renders, 2,
        "capture should render the root surface and one isolated backdrop child: {stats:?}"
    );
    assert_local_surface_stats(&frame, stats, BACKDROP_LAYER_SIZE, 4, "backdrop");
}

#[test]
fn a_magnifying_layers_backdrop_stays_registered_with_the_world_behind_it() {
    // A glass surface is a window, not a picture: scaling the layer it lives in
    // must move the glass, never the world seen through it. That only holds if
    // the underlay the nested backdrop samples is built at the scale that
    // surface renders at -- and a magnifying layer's surface scale is quantized
    // UP from the parent's (`magnifying_layer_scale`), so "the parent's scale"
    // is a whole quarter-step wrong. Built at the parent's scale, the backdrop
    // showed the world shrunk by parent/child about the surface origin: the
    // bottom bar's 1.03x press lift pulled the content behind it 20% toward the
    // bar's top-left corner, most visible as background lines sliding off the
    // real ones the moment a finger went down.
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping magnifying backdrop registration assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let root_scale = 2.0;
    let logical_width = FRAME_WIDTH as f32 / root_scale;
    let logical_height = FRAME_HEIGHT as f32 / root_scale;
    let boundary_x = 32.0;
    // Big enough that the misregistration is unmistakable: the error grows with
    // the distance from the surface origin, and the boundary sits 20 logical px
    // into a 40px-wide surface.
    let glass = Rect {
        x: 0.0,
        y: 0.0,
        width: 40.0,
        height: 24.0,
    };
    // The press lift's shape: a uniform scale about the layer's own center,
    // built exactly the way the scene builder builds one, so the transform and
    // the graphics layer agree the way they do in a real frame.
    let lift = GraphicsLayer {
        scale: 1.25,
        ..GraphicsLayer::default()
    };
    let magnified = layer(
        glass,
        cranpose_render_common::layer_transform::layer_transform_to_parent(
            glass,
            Point { x: 12.0, y: 12.0 },
            &lift,
        ),
        lift.clone(),
        vec![RenderNode::Layer(Box::new(layer(
            glass,
            ProjectiveTransform::identity(),
            GraphicsLayer {
                // A plain, barely-there blur: this test is about *where* the
                // backdrop lands, so the effect stays a near-pass-through and
                // leaves the red/blue boundary sharp enough to locate.
                backdrop_effect: Some(RenderEffect::blur(0.6)),
                ..GraphicsLayer::default()
            },
            vec![],
        )))],
    );

    renderer.scene_mut().graph = Some(RenderGraph::new(layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: logical_width,
            height: logical_height,
        },
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: boundary_x,
                    height: logical_height,
                },
                Color::RED,
            ),
            solid_rect(
                Rect {
                    x: boundary_x,
                    y: 0.0,
                    width: logical_width - boundary_x,
                    height: logical_height,
                },
                Color::BLUE,
            ),
            RenderNode::Layer(Box::new(magnified)),
        ],
    )));

    let frame = renderer
        .capture_frame_with_scale(FRAME_WIDTH, FRAME_HEIGHT, root_scale)
        .expect("magnifying backdrop capture should succeed");

    // Where the red/blue boundary reads through the glass, scanned along the
    // glass's middle row rather than asserted pixel by pixel, so the blur's
    // softness does not decide the verdict.
    let row = (logical_height * 0.5 * root_scale) as u32;
    let scan_from = ((boundary_x - 14.0) * root_scale) as u32;
    let scan_to = ((boundary_x + 14.0) * root_scale) as u32;
    let seen_boundary = (scan_from..scan_to)
        .find(|x| {
            let pixel = rgba(&frame, *x, row);
            pixel[2] > pixel[0]
        })
        .unwrap_or_else(|| panic!("no red/blue boundary visible through the glass along row {row}"))
        as f32;
    let true_boundary = boundary_x * root_scale;

    // The bug displaced this by 10 device px (5 logical); the blur's own
    // softness moves it by well under 2.
    assert!(
        (seen_boundary - true_boundary).abs() <= 4.0,
        "the world seen through a magnifying layer's glass must stay where it is: \
         boundary reads at device x {seen_boundary} but really sits at {true_boundary}"
    );
}

#[test]
fn scaled_sibling_backdrop_sees_prior_backdrop_output() {
    // Regression: two sibling backdrop layers over a sharp red/blue boundary at
    // root_scale = 2. The lower-z backdrop (strong blur) sits to the right of the
    // higher-z backdrop's center, exactly like dragging the Blur rect right of the
    // Glass rect in the Shaders demo. The higher-z backdrop must snapshot the
    // already-composited output of the lower-z one. A logical-vs-physical unit
    // mismatch in the pending-composite flush check made the snapshot skip it.
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping scaled sibling backdrop assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let root_scale = 2.0;
    let logical_width = FRAME_WIDTH as f32 / root_scale;
    let logical_height = FRAME_HEIGHT as f32 / root_scale;
    let logical_root = Rect {
        x: 0.0,
        y: 0.0,
        width: logical_width,
        height: logical_height,
    };
    let boundary_x = 36.0;

    // Lower-z backdrop: strong blur over the red/blue boundary.
    let strong_blur = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 24.0,
            height: 20.0,
        },
        ProjectiveTransform::translation(32.0, 10.0),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(8.0)),
            ..GraphicsLayer::default()
        },
        vec![],
    );
    // Higher-z backdrop: barely-blurring layer whose logical capture rect overlaps
    // the strong blur layer, but does not reach the strong blur layer's
    // physical-pixel rect (capture starts at logical 24, physical 48; this layer's
    // capture ends at logical 41).
    let weak_blur = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 24.0,
            height: 20.0,
        },
        ProjectiveTransform::translation(16.0, 12.0),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(1.0)),
            ..GraphicsLayer::default()
        },
        vec![],
    );

    renderer.scene_mut().graph = Some(RenderGraph::new(layer(
        logical_root,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: boundary_x,
                    height: logical_height,
                },
                Color::RED,
            ),
            solid_rect(
                Rect {
                    x: boundary_x,
                    y: 0.0,
                    width: logical_width - boundary_x,
                    height: logical_height,
                },
                Color::BLUE,
            ),
            RenderNode::Layer(Box::new(strong_blur)),
            RenderNode::Layer(Box::new(weak_blur)),
        ],
    )));

    let frame = renderer
        .capture_frame_with_scale(FRAME_WIDTH, FRAME_HEIGHT, root_scale)
        .expect("scaled sibling backdrop capture should succeed");

    assert_red(
        rgba(&frame, 20, 40),
        "background outside both backdrops should stay red",
    );

    // Logical (34, 20): inside both backdrop rects, 2px left of the boundary.
    // The strong blur mixes blue well past this point; the weak blur cannot.
    // The pixel is only mixed if the weak backdrop sampled the strong one's output.
    let overlap = rgba(&frame, 68, 40);
    assert!(
        overlap[2] >= 60,
        "higher-z backdrop must sample the lower-z backdrop's blurred output in the overlap: {overlap:?}"
    );
}

#[test]
fn nested_backdrop_blur_radius_changes_rendered_pixels() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping nested backdrop radius assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(nested_backdrop_effect_fixture(0.0));
    let base_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("nested backdrop base capture should succeed");

    renderer.scene_mut().graph = Some(nested_backdrop_effect_fixture(18.0));
    let blurred_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("nested backdrop blurred capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("nested backdrop blur frame stats");

    assert!(
        stats.blur_passes >= 1,
        "nested backdrop blur radius should execute blur passes: {stats:?}"
    );
    assert!(
        stats.submit_count <= 2,
        "capturing a nested backdrop frame should submit once for rendering plus the explicit readback copy: {stats:?}"
    );
    assert!(
        stats.encoder_count <= 2,
        "capturing a nested backdrop frame should use one render encoder plus the explicit readback encoder: {stats:?}"
    );

    let compare_rect = Rect {
        x: 62.0,
        y: 34.0,
        width: 36.0,
        height: 28.0,
    };
    let width = compare_rect.width as u32;
    let height = compare_rect.height as u32;
    let base = normalize_rgba_region(
        &base_frame.pixels,
        base_frame.width,
        base_frame.height,
        compare_rect,
        width,
        height,
    );
    let blurred = normalize_rgba_region(
        &blurred_frame.pixels,
        blurred_frame.width,
        blurred_frame.height,
        compare_rect,
        width,
        height,
    );
    let diff = image_difference_stats(&base, &blurred, width, height, 10);

    assert!(
        diff.differing_pixels > 90,
        "changing nested backdrop blur radius should change the rendered layer; differing_pixels={} max_diff={} stats={stats:?}",
        diff.differing_pixels,
        diff.max_difference
    );
}

#[test]
fn cached_nested_backdrop_blur_radius_changes_rendered_pixels() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping cached nested backdrop radius assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(cached_nested_backdrop_effect_fixture(0.0));
    let base_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("cached nested backdrop base capture should succeed");

    renderer.scene_mut().graph = Some(cached_nested_backdrop_effect_fixture(18.0));
    let blurred_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("cached nested backdrop blurred capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("cached nested backdrop blur frame stats");

    assert!(
        stats.layer_cache_hits > 0,
        "cached nested backdrop fixture should exercise retained layer cache: {stats:?}"
    );
    assert!(
        stats.blur_passes >= 1,
        "cached nested backdrop blur radius should execute blur passes: {stats:?}"
    );

    let compare_rect = Rect {
        x: 62.0,
        y: 34.0,
        width: 36.0,
        height: 28.0,
    };
    let width = compare_rect.width as u32;
    let height = compare_rect.height as u32;
    let base = normalize_rgba_region(
        &base_frame.pixels,
        base_frame.width,
        base_frame.height,
        compare_rect,
        width,
        height,
    );
    let blurred = normalize_rgba_region(
        &blurred_frame.pixels,
        blurred_frame.width,
        blurred_frame.height,
        compare_rect,
        width,
        height,
    );
    let diff = image_difference_stats(&base, &blurred, width, height, 10);

    assert!(
        diff.differing_pixels > 90,
        "cached nested backdrop blur radius must invalidate retained layer output; differing_pixels={} max_diff={} stats={stats:?}",
        diff.differing_pixels,
        diff.max_difference
    );
}

#[test]
fn static_backdrop_reuses_cache_when_non_overlapping_content_animates() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping spatial backdrop cache assertion because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(spatial_backdrop_cache_fixture(Color::RED));
    renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("backdrop cache warmup should succeed");
    let warmup_stats = renderer
        .last_frame_stats()
        .expect("backdrop warmup frame stats");

    renderer.scene_mut().graph = Some(spatial_backdrop_cache_fixture(Color::BLUE));
    let animated_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("non-overlapping animation frame should succeed");
    let animated_stats = renderer
        .last_frame_stats()
        .expect("non-overlapping animation frame stats");

    assert!(
        warmup_stats.blur_passes > 0,
        "the warmup frame must materialize the backdrop effect: {warmup_stats:?}"
    );
    assert!(
        animated_stats.layer_cache_hits > 0,
        "static glass should hit its retained backdrop surface: {animated_stats:?}"
    );
    assert_eq!(
        animated_stats.blur_passes, 0,
        "content outside the sampled backdrop region must not rerun the glass blur: {animated_stats:?}"
    );
    let animated_pixel = rgba(&animated_frame, 112, 80);
    assert!(
        animated_pixel[2] > animated_pixel[0],
        "the non-overlapping animated content must still update: {animated_pixel:?}"
    );
}

#[test]
fn translated_backdrop_capture_preserves_local_picture_under_rigid_motion() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping translated backdrop capture assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    let base_translation = Point::new(14.3, 18.6);
    renderer.scene_mut().graph = Some(translated_backdrop_fixture(base_translation));
    let base_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated backdrop base capture should succeed");
    let base_stats = renderer
        .last_frame_stats()
        .expect("translated backdrop base frame stats");

    let moved_translation = Point::new(36.4, 30.1);
    renderer.scene_mut().graph = Some(translated_backdrop_fixture(moved_translation));
    let moved_frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("translated backdrop moved capture should succeed");
    let moved_stats = renderer
        .last_frame_stats()
        .expect("translated backdrop moved frame stats");

    assert!(
        base_stats.blur_passes >= 1 && moved_stats.blur_passes >= 1,
        "translated backdrop frames should execute blur passes: base={base_stats:?} moved={moved_stats:?}"
    );
    assert_eq!(
        base_stats.isolated_layer_renders, 2,
        "translated backdrop base frame should keep only the content-bearing wrapper and backdrop child isolated: {base_stats:?}"
    );
    assert!(
        (1..=2).contains(&moved_stats.isolated_layer_renders),
        "translated backdrop moved frame should reuse cached rigid content without adding extra isolated layers: {moved_stats:?}"
    );

    let base_normalized = normalize_translated_backdrop_region(&base_frame, base_translation);
    let moved_normalized = normalize_translated_backdrop_region(&moved_frame, moved_translation);

    assert_translated_backdrop_local_picture(
        &base_normalized,
        translated_backdrop_compare_dimensions().0,
        translated_backdrop_compare_dimensions().1,
        "base",
    );
    assert_translated_backdrop_local_picture(
        &moved_normalized,
        translated_backdrop_compare_dimensions().0,
        translated_backdrop_compare_dimensions().1,
        "moved",
    );

    let stats = image_difference_stats(
        &base_normalized,
        &moved_normalized,
        translated_backdrop_compare_dimensions().0,
        translated_backdrop_compare_dimensions().1,
        TRANSLATED_BACKDROP_PIXEL_TOLERANCE,
    );
    if stats.differing_pixels > TRANSLATED_BACKDROP_MAX_DIFFERING_PIXELS
        || stats.max_difference > TRANSLATED_BACKDROP_MAX_PIXEL_DIFFERENCE
    {
        let diff = stats
            .first_difference
            .as_ref()
            .expect("failing translated backdrop comparison should report first difference");
        panic!(
            "translated backdrop local picture changed under rigid motion; differing_pixels={} max_diff={} first differing normalized pixel at ({}, {}) base={:?} moved={:?} diff={}",
            stats.differing_pixels,
            stats.max_difference,
            diff.x,
            diff.y,
            diff.lhs,
            diff.rhs,
            diff.difference
        );
    }
}

fn alpha_fixture() -> RenderGraph {
    let alpha_layer = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: ALPHA_LAYER_SIZE.0 as f32,
            height: ALPHA_LAYER_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(24.0, 26.0),
        GraphicsLayer {
            alpha: 0.5,
            ..GraphicsLayer::default()
        },
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 28.0,
                    height: 24.0,
                },
                Color::WHITE,
            ),
            solid_rect(
                Rect {
                    x: 20.0,
                    y: 0.0,
                    width: 28.0,
                    height: 24.0,
                },
                Color::WHITE,
            ),
        ],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(alpha_layer)),
    ])
}

fn dstout_alpha_fixture() -> RenderGraph {
    let top_layer = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 48.0,
        },
        ProjectiveTransform::translation(20.0, 20.0),
        GraphicsLayer {
            alpha: 0.35,
            compositing_strategy: CompositingStrategy::Offscreen,
            ..GraphicsLayer::default()
        },
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 80.0,
                    height: 48.0,
                },
                Color(0.1, 0.8, 1.0, 1.0),
            ),
            dstout_rect(Rect {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 24.0,
            }),
        ],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        solid_rect(
            Rect {
                x: 20.0,
                y: 20.0,
                width: 80.0,
                height: 48.0,
            },
            Color(0.70, 0.20, 0.10, 1.0),
        ),
        RenderNode::Layer(Box::new(top_layer)),
    ])
}

fn gradient_dstout_alpha_fixture() -> RenderGraph {
    let top_layer = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 88.0,
            height: 64.0,
        },
        ProjectiveTransform::translation(20.0, 18.0),
        GraphicsLayer {
            alpha: 0.35,
            compositing_strategy: CompositingStrategy::Offscreen,
            ..GraphicsLayer::default()
        },
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 88.0,
                    height: 64.0,
                },
                Color(0.10, 0.55, 0.95, 1.0),
            ),
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id: 201,
                    rect: Rect {
                        x: 20.0,
                        y: 36.0,
                        width: 48.0,
                        height: 18.0,
                    },
                    text: std::rc::Rc::new(AnnotatedString::from("TOP")),
                    text_style: TextStyle::from_span_style(SpanStyle {
                        color: Some(Color::WHITE),
                        font_size: TextUnit::Sp(14.0),
                        ..Default::default()
                    }),
                    font_size: 14.0,
                    layout_options: TextLayoutOptions::default(),
                    clip: None,
                })),
            }),
            gradient_dstout_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 88.0,
                    height: 64.0,
                },
                12.0,
                42.0,
            ),
        ],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        solid_rect(
            Rect {
                x: 20.0,
                y: 18.0,
                width: 88.0,
                height: 64.0,
            },
            Color(0.18, 0.22, 0.32, 1.0),
        ),
        solid_rect(
            Rect {
                x: 28.0,
                y: 26.0,
                width: 64.0,
                height: 18.0,
            },
            Color(1.0, 0.78, 0.22, 1.0),
        ),
        RenderNode::Layer(Box::new(top_layer)),
    ])
}

fn blur_fixture() -> RenderGraph {
    let blur_layer = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: BLUR_LAYER_SIZE.0 as f32,
            height: BLUR_LAYER_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(44.0, 26.0),
        GraphicsLayer {
            render_effect: Some(RenderEffect::blur(12.0)),
            ..GraphicsLayer::default()
        },
        vec![solid_rect(
            Rect {
                x: 10.0,
                y: 10.0,
                width: 10.0,
                height: 10.0,
            },
            Color::WHITE,
        )],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(blur_layer)),
    ])
}

fn unblurred_fixture() -> RenderGraph {
    let plain_layer = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: BLUR_LAYER_SIZE.0 as f32,
            height: BLUR_LAYER_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(44.0, 26.0),
        GraphicsLayer::default(),
        vec![solid_rect(
            Rect {
                x: 10.0,
                y: 10.0,
                width: 10.0,
                height: 10.0,
            },
            Color::WHITE,
        )],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(plain_layer)),
    ])
}

fn backdrop_fixture() -> RenderGraph {
    let backdrop_layer = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: BACKDROP_LAYER_SIZE.0 as f32,
            height: BACKDROP_LAYER_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(48.0, 30.0),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(8.0)),
            ..GraphicsLayer::default()
        },
        vec![],
    );

    graph(vec![
        solid_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: FRAME_HEIGHT as f32,
            },
            Color::RED,
        ),
        solid_rect(
            Rect {
                x: 64.0,
                y: 0.0,
                width: 64.0,
                height: FRAME_HEIGHT as f32,
            },
            Color::BLUE,
        ),
        RenderNode::Layer(Box::new(backdrop_layer)),
    ])
}

fn spatial_backdrop_cache_fixture(animated_color: Color) -> RenderGraph {
    let backdrop_layer = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 30.0,
        },
        ProjectiveTransform::translation(10.0, 10.0),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(8.0)),
            ..GraphicsLayer::default()
        },
        vec![],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        solid_rect(
            Rect {
                x: 100.0,
                y: 68.0,
                width: 24.0,
                height: 24.0,
            },
            animated_color,
        ),
        RenderNode::Layer(Box::new(backdrop_layer)),
    ])
}

fn nested_backdrop_effect_fixture(child_blur_radius: f32) -> RenderGraph {
    let child = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 32.0,
        },
        ProjectiveTransform::translation(54.0, 28.0),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(child_blur_radius)),
            ..GraphicsLayer::default()
        },
        vec![solid_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 48.0,
                height: 32.0,
            },
            Color::from_rgba_u8(40, 220, 140, 70),
        )],
    );
    let parent = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 112.0,
            height: 64.0,
        },
        ProjectiveTransform::translation(8.0, 16.0),
        GraphicsLayer {
            render_effect: Some(RenderEffect::blur(0.0)),
            ..GraphicsLayer::default()
        },
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 64.0,
                    height: 64.0,
                },
                Color::RED,
            ),
            solid_rect(
                Rect {
                    x: 64.0,
                    y: 0.0,
                    width: 48.0,
                    height: 64.0,
                },
                Color::BLUE,
            ),
            RenderNode::Layer(Box::new(child)),
        ],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(parent)),
    ])
}

fn cached_nested_backdrop_effect_fixture(child_blur_radius: f32) -> RenderGraph {
    let mut graph = nested_backdrop_effect_fixture(child_blur_radius);
    let Some(RenderNode::Layer(parent)) = graph.root.children.get_mut(1) else {
        panic!("expected nested backdrop parent layer");
    };
    parent.cache_policy = CachePolicy::Auto;
    let Some(RenderNode::Layer(child)) = parent.children.get_mut(2) else {
        panic!("expected nested backdrop child layer");
    };
    child.cache_policy = CachePolicy::Auto;
    graph.root.recompute_raster_cache_hashes();
    graph
}

fn translated_backdrop_fixture(translation: Point) -> RenderGraph {
    let backdrop_layer = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 16.0,
        },
        ProjectiveTransform::translation(18.0, 8.0),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(8.0)),
            ..GraphicsLayer::default()
        },
        vec![],
    );
    let translated_subtree = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: TRANSLATED_BACKDROP_SUBTREE_SIZE.0 as f32,
            height: TRANSLATED_BACKDROP_SUBTREE_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(translation.x, translation.y),
        GraphicsLayer::default(),
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 28.0,
                    height: TRANSLATED_BACKDROP_SUBTREE_SIZE.1 as f32,
                },
                Color::RED,
            ),
            solid_rect(
                Rect {
                    x: 28.0,
                    y: 0.0,
                    width: 28.0,
                    height: TRANSLATED_BACKDROP_SUBTREE_SIZE.1 as f32,
                },
                Color::BLUE,
            ),
            RenderNode::Layer(Box::new(backdrop_layer)),
        ],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(translated_subtree)),
    ])
}

fn opaque_row_with_glass_fixture(animated_color: Color) -> RenderGraph {
    let glass_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 24.0,
        height: 16.0,
    };
    let mut glass = layer(
        glass_bounds,
        ProjectiveTransform::translation(64.0, 8.0),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(6.0)),
            ..GraphicsLayer::default()
        },
        vec![solid_rect(
            glass_bounds,
            Color::from_rgba_u8(255, 255, 255, 60),
        )],
    );
    glass.isolation.shape_clip = true;
    let row_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 112.0,
        height: 32.0,
    };
    let mut row = layer(
        row_bounds,
        ProjectiveTransform::translation(8.0, 20.0),
        GraphicsLayer::default(),
        vec![
            solid_rect(row_bounds, Color::from_rgb_u8(30, 90, 200)),
            RenderNode::Layer(Box::new(glass)),
        ],
    );
    row.isolation.shape_clip = true;
    row.cache_policy = CachePolicy::Auto;

    let mut graph = graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(row)),
        solid_rect(
            Rect {
                x: 8.0,
                y: 72.0,
                width: 24.0,
                height: 16.0,
            },
            animated_color,
        ),
    ]);
    graph.root.recompute_raster_cache_hashes();
    graph
}

#[test]
fn a_row_whose_glass_reads_only_its_own_draws_keeps_its_raster() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping opaque row raster assertions because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(opaque_row_with_glass_fixture(Color::RED));
    renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("row warmup should succeed");
    let warm_stats = renderer.last_frame_stats().expect("row warmup frame stats");

    renderer.scene_mut().graph = Some(opaque_row_with_glass_fixture(Color::GREEN));
    let animated = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("row animation frame should succeed");
    let animated_stats = renderer
        .last_frame_stats()
        .expect("row animation frame stats");

    assert!(
        warm_stats.blur_passes > 0,
        "the glass must blur the row draw under it: {warm_stats:?}"
    );
    assert_eq!(
        animated_stats.isolated_layer_renders, 0,
        "a draw outside the row must not re-render the row: {animated_stats:?}"
    );
    assert!(
        animated_stats.layer_cache_hit_pixels >= 112 * 32,
        "the whole row raster, glass and blur included, must be replayed: {animated_stats:?}"
    );

    let glass_pixel = rgba(&animated, 76, 30);
    let row_pixel = rgba(&animated, 20, 30);
    assert!(
        glass_pixel[2] > glass_pixel[0] && row_pixel[2] > row_pixel[0],
        "the row and the glass over it both stay blue: glass={glass_pixel:?} row={row_pixel:?}"
    );
    let moved_pixel = rgba(&animated, 20, 80);
    assert!(
        moved_pixel[1] > moved_pixel[0],
        "the animated draw outside the row must still update: {moved_pixel:?}"
    );
}

fn graph(children: Vec<RenderNode>) -> RenderGraph {
    RenderGraph::new(layer(
        frame_rect(),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    ))
}

fn root_scale_fixture() -> RenderGraph {
    let logical_root = Rect {
        x: 0.0,
        y: 0.0,
        width: FRAME_WIDTH as f32 / ROOT_SCALE_TEST_SCALE,
        height: FRAME_HEIGHT as f32 / ROOT_SCALE_TEST_SCALE,
    };
    RenderGraph::new(layer(
        logical_root,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![solid_rect(logical_root, Color::RED)],
    ))
}

fn full_logical_root_scale_fixture(logical_width: f32, logical_height: f32) -> RenderGraph {
    let logical_root = Rect {
        x: 0.0,
        y: 0.0,
        width: logical_width,
        height: logical_height,
    };
    let lower_right = Rect {
        x: logical_width * 0.5,
        y: logical_height * 0.5,
        width: logical_width * 0.5,
        height: logical_height * 0.5,
    };
    RenderGraph::new(layer(
        logical_root,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            solid_rect(logical_root, Color::RED),
            solid_rect(lower_right, Color::BLUE),
        ],
    ))
}

fn translation_only_fixture() -> RenderGraph {
    let inner = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 16.0,
            height: 16.0,
        },
        ProjectiveTransform::translation(10.0, 12.0),
        GraphicsLayer::default(),
        vec![solid_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
            },
            Color::RED,
        )],
    );
    let middle = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 32.0,
            height: 32.0,
        },
        ProjectiveTransform::translation(8.0, 6.0),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(inner))],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(middle)),
    ])
}

fn translation_only_wrapper_with_text_fixture(wrapper_translation: Point) -> RenderGraph {
    translation_only_wrapper_with_text_style_fixture(
        wrapper_translation,
        TextStyle::from_span_style(SpanStyle {
            color: Some(Color::WHITE),
            ..Default::default()
        }),
    )
}

fn translation_only_wrapper_with_plain_text_only_fixture(
    wrapper_translation: Point,
) -> RenderGraph {
    let text_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: TRANSLATED_TEXT_LOCAL_SIZE.0 as f32,
            height: TRANSLATED_TEXT_LOCAL_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(9.0, 7.0),
        GraphicsLayer::default(),
        vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 99,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: TRANSLATED_TEXT_LOCAL_SIZE.0 as f32,
                    height: 20.0,
                },
                text: std::rc::Rc::new(AnnotatedString::from("Label")),
                text_style: TextStyle::from_span_style(SpanStyle {
                    color: Some(Color::WHITE),
                    ..Default::default()
                }),
                font_size: 16.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        })],
    );
    let wrapper = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 40.0,
        },
        ProjectiveTransform::translation(wrapper_translation.x, wrapper_translation.y),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(text_leaf))],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(wrapper)),
    ])
}

fn translation_only_wrapper_with_underlined_text_fixture(
    wrapper_translation: Point,
) -> RenderGraph {
    translation_only_wrapper_with_text_style_fixture(
        wrapper_translation,
        TextStyle::from_span_style(SpanStyle {
            color: Some(Color::WHITE),
            text_decoration: Some(cranpose_ui::text::TextDecoration::UNDERLINE),
            ..Default::default()
        }),
    )
}

fn translation_only_wrapper_with_thin_shapes_fixture(wrapper_translation: Point) -> RenderGraph {
    let shape_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: TRANSLATED_THIN_SHAPE_LOCAL_SIZE.0 as f32,
            height: TRANSLATED_THIN_SHAPE_LOCAL_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(9.0, 7.0),
        GraphicsLayer::default(),
        vec![
            solid_rect(
                Rect {
                    x: 2.0,
                    y: 1.0,
                    width: 32.0,
                    height: 3.0,
                },
                Color(0.66, 0.70, 0.86, 1.0),
            ),
            solid_rect(
                Rect {
                    x: 4.0,
                    y: 7.0,
                    width: 34.0,
                    height: 1.0,
                },
                Color(0.95, 0.80, 0.84, 1.0),
            ),
            solid_rect(
                Rect {
                    x: 6.0,
                    y: 11.0,
                    width: 30.0,
                    height: 1.0,
                },
                Color(0.95, 0.80, 0.84, 1.0),
            ),
            solid_rect(
                Rect {
                    x: 8.0,
                    y: 15.0,
                    width: 28.0,
                    height: 1.0,
                },
                Color(0.95, 0.80, 0.84, 1.0),
            ),
        ],
    );
    let wrapper = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 40.0,
        },
        ProjectiveTransform::translation(wrapper_translation.x, wrapper_translation.y),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(shape_leaf))],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(wrapper)),
    ])
}

fn translation_only_wrapper_with_decorated_shadow_text_fixture(
    wrapper_translation: Point,
) -> RenderGraph {
    translation_only_wrapper_with_text_style_fixture(
        wrapper_translation,
        TextStyle::from_span_style(SpanStyle {
            color: Some(Color::WHITE),
            letter_spacing: TextUnit::Em(0.18),
            background: Some(Color(0.2, 0.3, 0.52, 0.55)),
            text_decoration: Some(
                cranpose_ui::text::TextDecoration::UNDERLINE
                    .combine(cranpose_ui::text::TextDecoration::LINE_THROUGH),
            ),
            shadow: Some(cranpose_ui::text::Shadow {
                color: Color::BLACK,
                offset: Point::new(2.0, 2.0),
                blur_radius: 3.0,
            }),
            ..Default::default()
        }),
    )
}

fn repeated_drop_shadow_layers_fixture(card_count: usize) -> RenderGraph {
    let columns = 4usize;
    let card_width = 22.0;
    let card_height = 14.0;
    let gap_x = 8.0;
    let gap_y = 10.0;

    let mut children = Vec::with_capacity(card_count + 1);
    children.push(solid_rect(frame_rect(), Color::BLACK));
    for index in 0..card_count {
        let column = index % columns;
        let row = index / columns;
        let x = 12.0 + column as f32 * (card_width + gap_x);
        let y = 14.0 + row as f32 * (card_height + gap_y);
        children.push(RenderNode::Layer(Box::new(shadowed_card_layer(
            Point::new(x, y),
            card_width,
            card_height,
            index,
        ))));
    }

    graph(children)
}

fn shadowed_card_layer(
    translation: Point,
    card_width: f32,
    card_height: f32,
    index: usize,
) -> cranpose_render_common::graph::LayerNode {
    let card_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: card_width,
        height: card_height,
    };
    let shadow_rect = Rect {
        x: 0.0,
        y: 3.0,
        width: card_width,
        height: card_height,
    };
    let shade = 0.52 + (index % 3) as f32 * 0.08;
    layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: card_width,
            height: card_height + 6.0,
        },
        ProjectiveTransform::translation(translation.x, translation.y),
        GraphicsLayer::default(),
        vec![
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Shadow(ShadowPrimitive::Drop {
                        shape: Box::new(DrawPrimitive::Rect {
                            rect: shadow_rect,
                            brush: Brush::solid(Color(0.0, 0.0, 0.0, 0.50)),
                            stroke: None,
                        }),
                        cutout: None,
                        blur_radius: 5.0,
                        blend_mode: BlendMode::SrcOver,
                    }),
                    clip: None,
                }),
            }),
            solid_rect(card_rect, Color(shade, 0.78, 0.94, 1.0)),
        ],
    )
}

fn translation_only_wrapper_with_text_style_fixture(
    wrapper_translation: Point,
    text_style: TextStyle,
) -> RenderGraph {
    let text_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 32.0,
            height: 18.0,
        },
        ProjectiveTransform::translation(9.0, 7.0),
        GraphicsLayer::default(),
        vec![
            solid_rect(
                Rect {
                    x: 4.0,
                    y: 4.0,
                    width: 16.0,
                    height: 8.0,
                },
                Color::RED,
            ),
            solid_rect(
                Rect {
                    x: 4.0,
                    y: 14.0,
                    width: 20.0,
                    height: 1.0,
                },
                Color::WHITE,
            ),
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id: 1,
                    rect: Rect {
                        x: 4.0,
                        y: 2.0,
                        width: 24.0,
                        height: 14.0,
                    },
                    text: std::rc::Rc::new(AnnotatedString::from("Text")),
                    text_style,
                    font_size: 14.0,
                    layout_options: TextLayoutOptions::default(),
                    clip: None,
                })),
            }),
        ],
    );
    let wrapper = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 40.0,
        },
        ProjectiveTransform::translation(wrapper_translation.x, wrapper_translation.y),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(text_leaf))],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(wrapper)),
    ])
}

fn showcase_multispan_text() -> AnnotatedString {
    AnnotatedString::builder()
        .push_style(SpanStyle {
            color: Some(Color(0.5, 0.9, 0.6, 1.0)),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        })
        .append("This is bold green ")
        .pop()
        .append("and this is normal text. ")
        .push_style(SpanStyle {
            color: Some(Color(0.9, 0.4, 0.4, 1.0)),
            font_style: Some(FontStyle::Italic),
            text_decoration: Some(TextDecoration::UNDERLINE),
            ..Default::default()
        })
        .append("This is red, italic, and underlined!")
        .pop()
        .to_annotated_string()
}

fn translation_only_wrapper_with_multispan_showcase_text_fixture(
    wrapper_translation: Point,
) -> RenderGraph {
    let text_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 24.0,
        },
        ProjectiveTransform::translation(9.0, 7.0),
        GraphicsLayer::default(),
        vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 101,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 320.0,
                    height: 24.0,
                },
                text: std::rc::Rc::new(showcase_multispan_text()),
                text_style: TextStyle::from_span_style(SpanStyle::default()),
                font_size: 16.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        })],
    );
    let wrapper = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.0 as f32,
            height: MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(wrapper_translation.x, wrapper_translation.y),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(text_leaf))],
    );
    let frame_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: MULTISPAN_FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    };

    RenderGraph::new(layer(
        frame_rect,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            solid_rect(frame_rect, Color::BLACK),
            RenderNode::Layer(Box::new(wrapper)),
        ],
    ))
}

fn showcase_card_wrapper(wrapper_translation: Point) -> cranpose_render_common::graph::LayerNode {
    let text_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 24.0,
        },
        ProjectiveTransform::translation(9.0, 7.0),
        GraphicsLayer::default(),
        vec![
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::RoundRect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 320.0,
                            height: 28.0,
                        },
                        brush: Brush::solid(Color(0.18, 0.22, 0.30, 0.94)),
                        radii: cranpose_ui_graphics::CornerRadii::uniform(8.0),
                        stroke: None,
                    },
                    clip: None,
                }),
            }),
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id: 102,
                    rect: Rect {
                        x: 8.0,
                        y: 4.0,
                        width: 304.0,
                        height: 24.0,
                    },
                    text: std::rc::Rc::new(showcase_multispan_text()),
                    text_style: TextStyle::from_span_style(SpanStyle::default()),
                    font_size: 16.0,
                    layout_options: TextLayoutOptions::default(),
                    clip: None,
                })),
            }),
        ],
    );
    layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.0 as f32,
            height: MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(wrapper_translation.x, wrapper_translation.y),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(text_leaf))],
    )
}

fn translation_only_wrapper_with_showcase_card_fixture(wrapper_translation: Point) -> RenderGraph {
    let wrapper = showcase_card_wrapper(wrapper_translation);
    let frame_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: MULTISPAN_FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    };

    RenderGraph::new(layer(
        frame_rect,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            solid_rect(frame_rect, Color::BLACK),
            RenderNode::Layer(Box::new(wrapper)),
        ],
    ))
}

fn translation_only_wrapper_with_padded_multispan_showcase_text_fixture(
    wrapper_translation: Point,
) -> RenderGraph {
    let text_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 24.0,
        },
        ProjectiveTransform::translation(9.0, 7.0),
        GraphicsLayer::default(),
        vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 103,
                rect: Rect {
                    x: 8.0,
                    y: 4.0,
                    width: 304.0,
                    height: 24.0,
                },
                text: std::rc::Rc::new(showcase_multispan_text()),
                text_style: TextStyle::from_span_style(SpanStyle::default()),
                font_size: 16.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        })],
    );
    let wrapper = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.0 as f32,
            height: MULTISPAN_TEXT_WRAPPER_LOCAL_SIZE.1 as f32,
        },
        ProjectiveTransform::translation(wrapper_translation.x, wrapper_translation.y),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(text_leaf))],
    );
    let frame_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: MULTISPAN_FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    };

    RenderGraph::new(layer(
        frame_rect,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            solid_rect(frame_rect, Color::BLACK),
            RenderNode::Layer(Box::new(wrapper)),
        ],
    ))
}

fn gradient_stroke_text_fixture() -> RenderGraph {
    let text_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 36.0,
        },
        ProjectiveTransform::translation(8.0, 10.0),
        GraphicsLayer::default(),
        vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 77,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 120.0,
                    height: 28.0,
                },
                text: std::rc::Rc::new(AnnotatedString::from("Gradient")),
                text_style: TextStyle::from_span_style(SpanStyle {
                    brush: Some(Brush::linear_gradient(vec![
                        Color(0.42, 0.94, 1.0, 1.0),
                        Color(0.75, 0.84, 1.0, 1.0),
                        Color(1.0, 0.76, 0.54, 1.0),
                    ])),
                    alpha: Some(0.92),
                    font_size: TextUnit::Sp(20.0),
                    draw_style: Some(TextDrawStyle::Stroke { width: 3.8 }),
                    ..Default::default()
                }),
                font_size: 20.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        })],
    );

    graph(vec![
        solid_rect(frame_rect(), Color::BLACK),
        RenderNode::Layer(Box::new(text_leaf)),
    ])
}

fn crisp_icon_bitmap() -> ImageBitmap {
    const SIZE: u32 = 16;
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let stroke =
                x == 1 || x == SIZE - 2 || y == 1 || y == SIZE - 2 || x == y || x + y == SIZE - 1;
            let rgba = if stroke {
                [250, 252, 255, 255]
            } else {
                [18, 26, 38, 255]
            };
            let index = ((y * SIZE + x) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&rgba);
        }
    }
    ImageBitmap::from_rgba8(SIZE, SIZE, pixels).expect("valid crisp icon")
}

fn alpha_icon_text_surface_fixture(translation: Point) -> RenderGraph {
    let icon = crisp_icon_bitmap();
    let text_style = TextStyle::from_span_style(SpanStyle {
        color: Some(Color::WHITE),
        font_size: TextUnit::Sp(15.0),
        ..Default::default()
    });
    let icon_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 32.0,
            height: 32.0,
        },
        ProjectiveTransform::translation(8.0, 7.0),
        GraphicsLayer::default(),
        vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Image {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 32.0,
                        height: 32.0,
                    },
                    image: icon,
                    alpha: 1.0,
                    color_filter: None,
                    sampling: ImageSampling::Nearest,
                    src_rect: None,
                },
                clip: None,
            }),
        })],
    );
    let text_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 44.0,
            height: 22.0,
        },
        ProjectiveTransform::translation(47.0, 12.0),
        GraphicsLayer::default(),
        vec![RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 104,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 44.0,
                    height: 22.0,
                },
                text: std::rc::Rc::new(AnnotatedString::from("Meta")),
                text_style,
                font_size: 15.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        })],
    );
    let alpha_surface = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 94.0,
            height: 46.0,
        },
        ProjectiveTransform::translation(translation.x, translation.y),
        GraphicsLayer {
            alpha: 0.82,
            ..GraphicsLayer::default()
        },
        vec![
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::RoundRect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 94.0,
                            height: 46.0,
                        },
                        brush: Brush::solid(Color(0.16, 0.18, 0.24, 1.0)),
                        radii: cranpose_ui_graphics::CornerRadii::uniform(12.0),
                        stroke: None,
                    },
                    clip: None,
                }),
            }),
            RenderNode::Layer(Box::new(icon_leaf)),
            RenderNode::Layer(Box::new(text_leaf)),
        ],
    );

    RenderGraph::new(layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 150.0,
            height: 86.0,
        },
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 150.0,
                            height: 86.0,
                        },
                        brush: Brush::solid(Color::BLACK),
                        stroke: None,
                    },
                    clip: None,
                }),
            }),
            RenderNode::Layer(Box::new(alpha_surface)),
        ],
    ))
}

#[test]
fn gradient_stroke_text_effect_renders_colored_material() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping gradient stroke effect assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(gradient_stroke_text_fixture());
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("gradient stroke capture should succeed");
    let stats = renderer
        .last_frame_stats()
        .expect("gradient stroke frame stats");

    assert!(
        stats.effect_applies > 0,
        "gradient stroke text should exercise the runtime shader effect path: {stats:?}"
    );

    let mut colored_pixels = 0usize;
    let mut min_red = 1.0f32;
    let mut max_red = 0.0f32;
    let mut min_blue = 1.0f32;
    let mut max_blue = 0.0f32;

    for y in 10..38 {
        for x in 8..128 {
            let rgba = rgba(&frame, x, y);
            let red = rgba[0] as f32 / 255.0;
            let green = rgba[1] as f32 / 255.0;
            let blue = rgba[2] as f32 / 255.0;
            let max_channel = red.max(green.max(blue));
            let min_channel = red.min(green.min(blue));
            let saturation = max_channel - min_channel;
            if max_channel > 0.30 && saturation > 0.08 {
                colored_pixels += 1;
                min_red = min_red.min(red);
                max_red = max_red.max(red);
                min_blue = min_blue.min(blue);
                max_blue = max_blue.max(blue);
            }
        }
    }

    assert!(
        colored_pixels > 20,
        "gradient stroke text should produce visible colored ink, got colored_pixels={colored_pixels}"
    );
    let red_span = max_red - min_red;
    let blue_span = max_blue - min_blue;
    assert!(
        red_span.max(blue_span) > 0.12,
        "gradient stroke text should vary across the sampled region, got red_span={red_span:.3}, blue_span={blue_span:.3}"
    );
}

fn layer(
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    graphics_layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> cranpose_render_common::graph::LayerNode {
    shared_test_support::layer_node(local_bounds, transform_to_parent, graphics_layer, children)
}

fn solid_rect(rect: Rect, color: Color) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Rect {
                rect,
                brush: Brush::solid(color),
                stroke: None,
            },
            clip: None,
        }),
    })
}

fn dstout_rect(rect: Rect) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Blend {
                primitive: Box::new(DrawPrimitive::Rect {
                    rect,
                    brush: Brush::solid(Color::BLACK),
                    stroke: None,
                }),
                blend_mode: BlendMode::DstOut,
            },
            clip: None,
        }),
    })
}

fn gradient_dstout_rect(rect: Rect, start_y: f32, end_y: f32) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Blend {
                primitive: Box::new(DrawPrimitive::Rect {
                    rect,
                    brush: Brush::vertical_gradient(
                        vec![Color::BLACK, Color::from_rgba_u8(0, 0, 0, 0)],
                        start_y,
                        end_y,
                    ),
                    stroke: None,
                }),
                blend_mode: BlendMode::DstOut,
            },
            clip: None,
        }),
    })
}

fn frame_rect() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    }
}

fn mark_translated_text_wrapper(graph: &mut RenderGraph) {
    let Some(RenderNode::Layer(wrapper)) = graph.root.children.get_mut(1) else {
        panic!("expected translated text wrapper layer");
    };
    wrapper.translated_content_context = true;
    wrapper.motion_context_animated = true;
    let Some(RenderNode::Layer(text_leaf)) = wrapper.children.get_mut(0) else {
        panic!("expected translated text leaf layer");
    };
    text_leaf.translated_content_context = true;
    text_leaf.motion_context_animated = true;
}

fn mark_translated_shape_wrapper(graph: &mut RenderGraph) {
    let Some(RenderNode::Layer(wrapper)) = graph.root.children.get_mut(1) else {
        panic!("expected translated thin-shape wrapper layer");
    };
    wrapper.translated_content_context = true;
    wrapper.motion_context_animated = true;
    let Some(RenderNode::Layer(shape_leaf)) = wrapper.children.get_mut(0) else {
        panic!("expected translated thin-shape leaf layer");
    };
    shape_leaf.translated_content_context = true;
    shape_leaf.motion_context_animated = true;
}

fn normalize_translated_backdrop_region(frame: &CapturedFrame, translation: Point) -> Vec<u8> {
    let (output_width, output_height) = translated_backdrop_compare_dimensions();
    let inset = TRANSLATED_BACKDROP_COMPARE_INSET as f32;
    normalize_rgba_region(
        &frame.pixels,
        frame.width,
        frame.height,
        Rect {
            x: translation.x + inset,
            y: translation.y + inset,
            width: output_width as f32,
            height: output_height as f32,
        },
        output_width,
        output_height,
    )
}

fn scaled_dimension(logical_size: u32, root_scale: f32) -> u32 {
    ((logical_size as f32) * root_scale).ceil().max(1.0) as u32
}

fn capture_logical_frame_with_scale(
    renderer: &mut WgpuRenderer,
    logical_width: u32,
    logical_height: u32,
    root_scale: f32,
) -> CapturedFrame {
    renderer
        .capture_frame_with_scale(
            scaled_dimension(logical_width, root_scale),
            scaled_dimension(logical_height, root_scale),
            root_scale,
        )
        .expect("scaled logical capture should succeed")
}

fn normalize_translated_region_at_scale(
    frame: &CapturedFrame,
    translation: Point,
    local_size: (u32, u32),
    root_scale: f32,
) -> Vec<u8> {
    let canonical_device_origin = |logical: f32| {
        let device = f64::from(logical) * f64::from(root_scale);
        ((device * DEVICE_SNAP_SUBPIXEL_STEPS).round() / DEVICE_SNAP_SUBPIXEL_STEPS).round() as u32
    };
    let start_x = canonical_device_origin(translation.x);
    let start_y = canonical_device_origin(translation.y);
    let width = scaled_dimension(local_size.0, root_scale);
    let height = scaled_dimension(local_size.1, root_scale);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let src_y = start_y + y;
        let row_start = ((src_y * frame.width + start_x) * 4) as usize;
        let row_end = row_start + (width * 4) as usize;
        pixels.extend_from_slice(&frame.pixels[row_start..row_end]);
    }
    pixels
}

fn assert_exact_normalized_match(label: &str, base: &[u8], moved: &[u8], width: u32, height: u32) {
    let stats = image_difference_stats(base, moved, width, height, 0);
    if stats.differing_pixels == 0 && stats.max_difference == 0 {
        return;
    }

    let diff = stats
        .first_difference
        .as_ref()
        .expect("exact comparison should report first difference");
    panic!(
        "{label} changed under a 1px logical scroll at fractional root scale; differing_pixels={} max_diff={} first differing normalized pixel at ({}, {}) base={:?} moved={:?} diff={}",
        stats.differing_pixels,
        stats.max_difference,
        diff.x,
        diff.y,
        diff.lhs,
        diff.rhs,
        diff.difference
    );
}

struct NormalizedMatchTolerance {
    pixel_tolerance: u32,
    max_differing_pixels: u32,
    max_pixel_difference: u32,
}

fn assert_nearly_exact_normalized_match(
    label: &str,
    base: &[u8],
    moved: &[u8],
    size: (u32, u32),
    tolerance: NormalizedMatchTolerance,
) {
    let stats = image_difference_stats(base, moved, size.0, size.1, tolerance.pixel_tolerance);
    if stats.differing_pixels <= tolerance.max_differing_pixels
        && stats.max_difference <= tolerance.max_pixel_difference
    {
        return;
    }

    let diff = stats
        .first_difference
        .as_ref()
        .expect("inexact comparison should report first difference");
    panic!(
        "{label} changed under a 1px logical scroll at fractional root scale; differing_pixels={} max_diff={} first differing normalized pixel at ({}, {}) base={:?} moved={:?} diff={}",
        stats.differing_pixels,
        stats.max_difference,
        diff.x,
        diff.y,
        diff.lhs,
        diff.rhs,
        diff.difference
    );
}

fn normalize_translated_text_region(frame: &CapturedFrame, translation: Point) -> Vec<u8> {
    normalize_rgba_region(
        &frame.pixels,
        frame.width,
        frame.height,
        Rect {
            x: translation.x + 9.0,
            y: translation.y + 7.0,
            width: TRANSLATED_TEXT_LOCAL_SIZE.0 as f32,
            height: TRANSLATED_TEXT_LOCAL_SIZE.1 as f32,
        },
        TRANSLATED_TEXT_LOCAL_SIZE.0,
        TRANSLATED_TEXT_LOCAL_SIZE.1,
    )
}

fn assert_translated_text_local_picture_stable(base: &[u8], moved: &[u8], axis: &str) {
    let stats = image_difference_stats(
        base,
        moved,
        TRANSLATED_TEXT_LOCAL_SIZE.0,
        TRANSLATED_TEXT_LOCAL_SIZE.1,
        TRANSLATED_TEXT_PIXEL_TOLERANCE,
    );
    if stats.differing_pixels > TRANSLATED_TEXT_MAX_DIFFERING_PIXELS
        || stats.max_difference > TRANSLATED_TEXT_MAX_PIXEL_DIFFERENCE
    {
        let diff = stats
            .first_difference
            .as_ref()
            .expect("failing translated text comparison should report first difference");
        panic!(
            "translated text local picture changed too much under {axis} fractional motion; differing_pixels={} max_diff={} first differing normalized pixel at ({}, {}) base={:?} moved={:?} diff={}",
            stats.differing_pixels,
            stats.max_difference,
            diff.x,
            diff.y,
            diff.lhs,
            diff.rhs,
            diff.difference
        );
    }
}

fn translated_backdrop_compare_dimensions() -> (u32, u32) {
    (
        TRANSLATED_BACKDROP_SUBTREE_SIZE.0 - (TRANSLATED_BACKDROP_COMPARE_INSET * 2),
        TRANSLATED_BACKDROP_SUBTREE_SIZE.1 - (TRANSLATED_BACKDROP_COMPARE_INSET * 2),
    )
}

fn rgba(frame: &CapturedFrame, x: u32, y: u32) -> [u8; 4] {
    let index = ((y as usize) * (frame.width as usize) + (x as usize)) * 4;
    [
        frame.pixels[index],
        frame.pixels[index + 1],
        frame.pixels[index + 2],
        frame.pixels[index + 3],
    ]
}

fn bright_pixel_count(frame: &CapturedFrame, rect: Rect, threshold: u8) -> usize {
    let left = rect.x.max(0.0).floor() as u32;
    let top = rect.y.max(0.0).floor() as u32;
    let right = (rect.x + rect.width)
        .min(frame.width as f32)
        .ceil()
        .max(0.0) as u32;
    let bottom = (rect.y + rect.height)
        .min(frame.height as f32)
        .ceil()
        .max(0.0) as u32;

    let mut count = 0usize;
    for y in top..bottom {
        for x in left..right {
            let pixel = rgba(frame, x, y);
            if pixel[0] >= threshold || pixel[1] >= threshold || pixel[2] >= threshold {
                count += 1;
            }
        }
    }
    count
}

fn assert_translated_backdrop_local_picture(pixels: &[u8], width: u32, height: u32, label: &str) {
    let (left_x, mixed_x, right_x) = translated_backdrop_sample_positions(width);
    let left = sample_pixel(pixels, width, left_x, height / 2);
    let mixed = sample_pixel(pixels, width, mixed_x, height / 2);
    let right = sample_pixel(pixels, width, right_x, height / 2);

    assert_red(
        left,
        &format!("{label} translated backdrop left background should stay red"),
    );
    assert_blue(
        right,
        &format!("{label} translated backdrop right background should stay blue"),
    );
    assert!(
        mixed[2] >= left[2].saturating_add(40),
        "{label} translated backdrop blur should mix blue into the center window: left={left:?} mixed={mixed:?} right={right:?}"
    );
}

fn translated_backdrop_sample_positions(width: u32) -> (u32, u32, u32) {
    let split_x = (TRANSLATED_BACKDROP_SUBTREE_SIZE.0 / 2) - TRANSLATED_BACKDROP_COMPARE_INSET;
    let left_x = split_x / 3;
    let mixed_x = split_x;
    let right_x = split_x + (split_x - left_x);
    assert!(
        right_x < width,
        "translated backdrop sample positions must stay inside the normalized region",
    );
    (left_x, mixed_x, right_x)
}

fn assert_channel_close(actual: u8, expected: u8, tolerance: u8, label: &str) {
    let difference = actual.abs_diff(expected);
    assert!(
        difference <= tolerance,
        "{label} differed by {difference}, actual={actual}, expected={expected}, tolerance={tolerance}"
    );
}

fn assert_dark(pixel: [u8; 4], label: &str) {
    assert!(
        pixel[0] <= 4 && pixel[1] <= 4 && pixel[2] <= 4,
        "{label} should stay dark, got {pixel:?}"
    );
}

fn assert_red(pixel: [u8; 4], label: &str) {
    assert!(
        pixel[0] >= 220 && pixel[1] <= 20 && pixel[2] <= 20,
        "{label}, got {pixel:?}"
    );
}

fn assert_blue(pixel: [u8; 4], label: &str) {
    assert!(
        pixel[2] >= 220 && pixel[0] <= 20 && pixel[1] <= 20,
        "{label}, got {pixel:?}"
    );
}

fn assert_local_surface_stats(
    frame: &CapturedFrame,
    stats: RenderStatsSnapshot,
    layer_size: (u32, u32),
    extra_local_targets: u64,
    label: &str,
) {
    let frame_bytes = (frame.width as u64) * (frame.height as u64) * 4;
    let frame_pixels = (frame.width as u64) * (frame.height as u64);
    let layer_pixels = (layer_size.0 as u64) * (layer_size.1 as u64);
    let expected_bytes_upper_bound = frame_bytes + layer_pixels * 4 * extra_local_targets;
    assert!(
        stats.offscreen_acquires > 0,
        "{label} should acquire effect offscreen targets: {stats:?}"
    );
    assert!(
        stats.offscreen_total_bytes <= expected_bytes_upper_bound,
        "{label} should stay within the root frame surface plus bounded local scratch targets: max_bytes={expected_bytes_upper_bound} stats={stats:?}"
    );
    assert!(
        stats.isolated_layer_pixels <= frame_pixels + layer_pixels,
        "{label} should only isolate the root frame plus one bounded child layer: {stats:?}"
    );
}
