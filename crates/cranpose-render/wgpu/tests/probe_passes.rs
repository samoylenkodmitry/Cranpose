mod support;

use support::{SIZE, record_mixed_scene};

fn probed_against_plain(
    toggle: &'static str,
    count: &str,
    extra_passes: u32,
    extra_draws: u32,
    claim: &str,
) {
    let Ok(mut renderer) = support::headless_renderer() else {
        return;
    };
    let mut scope = cranpose_ui_graphics::DrawScopeDefault::new(cranpose_ui_graphics::Size::new(
        SIZE as f32,
        SIZE as f32,
    ));
    record_mixed_scene(&mut scope);
    let graph = support::draw_run_graph(SIZE, scope);
    cranpose_render_wgpu::set_debug_toggle(toggle, None);
    let plain = support::settled_capture(&mut renderer, &graph);
    let plain_stats = renderer.last_frame_stats().expect("stats");
    cranpose_render_wgpu::set_debug_toggle(toggle, Some(count));
    let probed = support::settled_capture(&mut renderer, &graph);
    let probed_stats = renderer.last_frame_stats().expect("stats");
    cranpose_render_wgpu::set_debug_toggle(toggle, None);
    assert_eq!(
        probed_stats.pass_count,
        plain_stats.pass_count + extra_passes,
        "{claim}"
    );
    assert_eq!(
        probed_stats.draw_calls,
        plain_stats.draw_calls + extra_draws,
        "{toggle}={count}: the probe's draws are recorded, so a pass that draws is proven to draw"
    );
    assert_eq!(
        probed, plain,
        "{toggle}={count} changes no byte of the frame"
    );
}

#[test]
fn probe_passes_add_timed_empty_passes_and_change_no_byte() {
    probed_against_plain(
        "CRANPOSE_PROBE_PASSES",
        "3",
        3,
        0,
        "three probe passes load and store the page and draw nothing",
    );
}

#[test]
fn probe_draw_passes_blit_a_transparent_texel_over_the_page_and_change_no_byte() {
    probed_against_plain(
        "CRANPOSE_PROBE_DRAW_PASSES",
        "4",
        5,
        4,
        "one clear of the blank texel, then four passes each blitting it over the page",
    );
}
