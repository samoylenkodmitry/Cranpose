mod support;

use cranpose_render_common::graph::RenderGraph;
use cranpose_ui_graphics::{DrawScopeDefault, Size};
use support::{LockedRenderer, SIZE, record_mixed_scene};

const TOGGLE: &str = "CRANPOSE_ABLATE";

fn graph_of(record: fn(&mut DrawScopeDefault)) -> RenderGraph {
    let mut scope = DrawScopeDefault::new(Size::new(SIZE as f32, SIZE as f32));
    record(&mut scope);
    support::draw_run_graph(SIZE, scope)
}

fn capture_under(
    renderer: &mut LockedRenderer,
    graph: &RenderGraph,
    switch: Option<&str>,
) -> Vec<u8> {
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, switch);
    let pixels = support::settled_capture(renderer, graph);
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);
    pixels
}

#[test]
fn the_shape_switches_remove_the_fragment_program_and_then_the_fill() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping shape ablation: headless WGPU init failed: {err}");
            return;
        }
    };
    let mixed = graph_of(record_mixed_scene);
    let base = capture_under(&mut renderer, &mixed, None);
    let flat = capture_under(&mut renderer, &mixed, Some("shape"));
    let discarded = capture_under(&mut renderer, &mixed, Some("shape_fill"));
    let cleared = capture_under(&mut renderer, &graph_of(|_| {}), None);

    assert!(
        support::distinct_colors(&base) > 8,
        "the scene must draw something"
    );
    assert_ne!(base, flat, "`shape` must drop coverage and brush");
    assert!(
        support::distinct_colors(&flat) > 1,
        "`shape` must keep the fill: flat fragments still write"
    );
    assert_ne!(
        discarded, flat,
        "`shape_fill` must remove the fill `shape` keeps"
    );
    assert_eq!(discarded, cleared, "`shape_fill` must leave only the clear");
}
