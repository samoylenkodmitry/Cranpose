use cranpose_ui::{
    Modifier, RecordedRenderScene, ZoomState, command_draw_scope_reusing, format_layout_tree,
    format_modifier_chain, format_render_scene, format_screen_summary, log_layout_tree,
    log_modifier_chain, log_render_scene, log_screen_summary, measure_layout, run_test_composition,
};
use cranpose_ui_graphics::Size;

fn measured() -> cranpose_ui::LayoutTree {
    let mut composition = run_test_composition(|| {
        cranpose_ui::widgets::box_widget::Box(
            Modifier::empty().size(Size::new(40.0, 20.0)),
            cranpose_ui::widgets::box_widget::BoxSpec::default(),
            || {},
        );
    });
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let tree = measure_layout(&mut applier, root, Size::new(200.0, 200.0))
        .expect("the tree measures")
        .into_layout_tree()
        .expect("a layout tree");
    applier.clear_runtime_handle();
    tree
}

#[test]
fn the_layout_formatter_describes_the_tree_it_is_given() {
    let tree = measured();
    let text = format_layout_tree(&tree);
    assert!(
        text.contains("LAYOUT TREE"),
        "the layout dump lost its header: {text}"
    );
    assert!(
        text.contains("40"),
        "a 40-wide box was not described: {text}"
    );
    log_layout_tree(&tree);
}

#[test]
fn the_scene_and_summary_formatters_describe_an_empty_screen_without_panicking() {
    let tree = measured();
    let scene = RecordedRenderScene::default();

    let scene_text = format_render_scene(&scene);
    assert!(
        !scene_text.is_empty(),
        "an empty scene formatted to nothing"
    );

    let summary = format_screen_summary(&tree, &scene);
    assert!(
        summary.contains("SUMMARY"),
        "the summary lost its header: {summary}"
    );

    log_render_scene(&scene);
    log_screen_summary(&tree, &scene);
}

#[test]
fn the_modifier_chain_formatter_reports_how_many_nodes_it_walked() {
    let chain = cranpose_foundation::ModifierNodeChain::new();
    let text = format_modifier_chain(&chain, &[]);
    assert!(
        text.contains("MODIFIER CHAIN"),
        "the chain dump lost its header: {text}"
    );
    assert!(
        text.contains("Total nodes: 0"),
        "an empty chain was not described as empty: {text}"
    );
    log_modifier_chain(&chain, &[]);
}

#[test]
fn a_reusing_draw_scope_gives_the_callers_buffer_back() {
    let storage = Vec::with_capacity(64);
    let capacity = storage.capacity();

    let finished = command_draw_scope_reusing(Size::new(10.0, 10.0), storage).finish();
    assert!(
        finished.primitives.capacity() >= capacity,
        "the scope handed back a buffer with less capacity than it was given"
    );
    assert!(
        finished.primitives.is_empty(),
        "a scope nobody drew into produced primitives"
    );
}

#[test]
fn a_zoom_state_starts_with_limits_that_admit_the_identity_scale() {
    run_test_composition(|| {
        let state = ZoomState::new();
        assert!(
            state.min_scale() <= 1.0 && state.max_scale() >= 1.0,
            "unzoomed content ({}..{}) does not fit its own limits",
            state.min_scale(),
            state.max_scale()
        );
        assert!(
            state.min_scale() > 0.0,
            "a minimum scale of zero would let content vanish"
        );
    });
}
