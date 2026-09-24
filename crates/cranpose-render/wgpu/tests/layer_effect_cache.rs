use cranpose_render_common::{
    graph::{CachePolicy, ProjectiveTransform, RenderGraph, RenderNode},
    raster_cache::LAYER_RASTER_CACHE_KIND_LABELS,
};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, RenderEffect};

use crate::{
    shared_test_support, support,
    support::bar_scene::{BAR, HEIGHT, WIDTH},
};

const INK: Rect = Rect {
    x: 40.0,
    y: 36.0,
    width: 60.0,
    height: 20.0,
};

fn effect_kind() -> usize {
    LAYER_RASTER_CACHE_KIND_LABELS
        .iter()
        .position(|label| *label == "effect")
        .expect("the layer cache accounts for layer effects")
}

/// The striped page with a blurred layer over `BAR` whose content is one
/// still rectangle.
fn blurred_layer_page() -> RenderGraph {
    let mut page = support::striped_page(WIDTH, HEIGHT);
    let mut layer = shared_test_support::layer_node(
        BAR,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            render_effect: Some(RenderEffect::blur(3.0)),
            ..Default::default()
        },
        vec![support::solid_rect(INK, Color::WHITE)],
    );
    layer.node_id = Some(7);
    layer.cache_policy = CachePolicy::Auto;
    page.push(RenderNode::Layer(Box::new(layer)));
    support::page_graph(WIDTH, HEIGHT, page)
}

/// A render effect over a retained surface is drawn once the same output
/// is wanted two frames running, then read back from the layer cache: the
/// third frame of a still blurred layer runs no blur and lands on the bytes
/// the first frame drew.
#[test]
fn a_still_layers_effect_is_drawn_once_and_read_back() {
    let mut renderer = support::headless_renderer().expect("GPU required for the cache probe");
    let kind = effect_kind();
    let first = support::capture_graph(&mut renderer, blurred_layer_page(), WIDTH, HEIGHT);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(stats.blur_passes > 0, "the first frame draws the blur");
    assert_eq!(stats.layer_cache_misses_by_kind[kind], 1);
    assert_eq!(stats.layer_cache_hits_by_kind[kind], 0);

    let second = support::capture_graph(&mut renderer, blurred_layer_page(), WIDTH, HEIGHT);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        stats.blur_passes > 0,
        "the second frame draws the blur into the cache"
    );
    assert_eq!(stats.layer_cache_misses_by_kind[kind], 1);
    assert_eq!(stats.layer_cache_hits_by_kind[kind], 0);

    let third = support::capture_graph(&mut renderer, blurred_layer_page(), WIDTH, HEIGHT);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert_eq!(stats.blur_passes, 0, "the third frame reads the blur back");
    assert_eq!(stats.effect_applies, 0);
    assert_eq!(stats.layer_cache_hits_by_kind[kind], 1);
    assert_eq!(stats.layer_cache_misses_by_kind[kind], 0);

    for (label, frame) in [("second", &second), ("third", &third)] {
        let differing = support::differing_pixels(WIDTH, &first.pixels, &frame.pixels);
        assert!(
            differing.is_empty(),
            "the {label} frame differs from the first: {}",
            support::describe_differing(&differing)
        );
    }
    assert_eq!(renderer.device_error_count_for_tests(), 0);
}
