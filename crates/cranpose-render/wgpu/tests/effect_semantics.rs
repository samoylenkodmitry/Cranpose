mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{
    DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
    RenderGraph, RenderNode,
};
use cranpose_render_common::image_compare::{
    image_difference_stats, normalize_rgba_region, sample_pixel,
};
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, GraphicsLayer, Point, Rect, RenderEffect};

const FRAME_WIDTH: u32 = 128;
const FRAME_HEIGHT: u32 = 96;
const ROOT_SCALE_TEST_SCALE: f32 = 2.0;
const ALPHA_LAYER_SIZE: (u32, u32) = (48, 24);
const BLUR_LAYER_SIZE: (u32, u32) = (28, 28);
const BACKDROP_LAYER_SIZE: (u32, u32) = (24, 20);
const TRANSLATED_BACKDROP_SUBTREE_SIZE: (u32, u32) = (56, 32);
const TRANSLATED_BACKDROP_COMPARE_INSET: u32 = 2;
const TRANSLATED_BACKDROP_PIXEL_TOLERANCE: u32 = 24;
const TRANSLATED_BACKDROP_MAX_DIFFERING_PIXELS: u32 = 120;
const TRANSLATED_BACKDROP_MAX_PIXEL_DIFFERENCE: u32 = 360;

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
        stats.isolated_layer_renders, 2,
        "capture should render the root surface and one isolated alpha child: {stats:?}"
    );
    assert_local_surface_stats(&frame, stats, ALPHA_LAYER_SIZE, 1, "alpha");
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
        stats.isolated_layer_renders, 2,
        "capture should render the root surface and one isolated blur child: {stats:?}"
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
        base_stats.isolated_layer_renders, 3,
        "translated backdrop base frame should render the root surface, the translated subtree surface, and one isolated backdrop child: {base_stats:?}"
    );
    assert_eq!(
        moved_stats.isolated_layer_renders, 3,
        "translated backdrop moved frame should render the root surface, the translated subtree surface, and one isolated backdrop child: {moved_stats:?}"
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
