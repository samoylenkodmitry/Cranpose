//! `Modifier::pointer_icon` from the composable that declares it to the hit
//! region the shell reads the cursor off.

use cranpose_core::NodeId;
use cranpose_macros::composable;
use cranpose_render_common::{
    RenderScene, graph::ProjectiveTransform, graph_scene::Scene,
    hit_graph::collect_hits_from_graph, scene_builder::build_graph_from_applier,
};
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

const VIEWPORT: Size = Size {
    width: 200.0,
    height: 200.0,
};

fn scene_for(rule: &mut ComposeTestRule) -> Scene {
    let root = rule.root_id().expect("composition has a root");
    let mut scene = Scene::new();
    let mut applier = rule.applier_mut();
    applier
        .compute_layout(root, VIEWPORT)
        .expect("layout succeeds");
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("scene graph");
    collect_hits_from_graph(
        &graph.root,
        ProjectiveTransform::identity(),
        &mut scene,
        None,
    );
    scene
}

fn topmost_pointer_icon(scene: &Scene, x: f32, y: f32) -> Option<PointerIcon> {
    scene
        .hit_test(x, y)
        .iter()
        .find_map(cranpose_render_common::HitTestTarget::pointer_icon)
}

fn hit_node_ids(scene: &Scene, x: f32, y: f32) -> Vec<NodeId> {
    scene.hit_test_nodes(x, y)
}

#[composable]
fn icon_only_app() {
    Box(
        Modifier::empty()
            .size(VIEWPORT)
            .pointer_icon(PointerIcon::POINTER),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn nested_icons_app() {
    Box(
        Modifier::empty()
            .size(VIEWPORT)
            .cursor(CursorIcon::Crosshair),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .size(Size {
                        width: 40.0,
                        height: 40.0,
                    })
                    .cursor(CursorIcon::EwResize),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn no_icon_app() {
    Box(Modifier::empty().size(VIEWPORT), BoxSpec::default(), || {});
}

#[test]
fn a_pointer_icon_alone_makes_a_node_a_hit_target() {
    let context = AppContext::new();
    let _scope = context.enter_scope();
    context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    rule.set_content(icon_only_app).expect("render");
    let scene = scene_for(&mut rule);

    assert_eq!(hit_node_ids(&scene, 10.0, 10.0).len(), 1);
    assert_eq!(
        topmost_pointer_icon(&scene, 10.0, 10.0),
        Some(PointerIcon::POINTER)
    );
}

#[test]
fn a_node_without_a_pointer_icon_stays_out_of_the_hit_graph() {
    let context = AppContext::new();
    let _scope = context.enter_scope();
    context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    rule.set_content(no_icon_app).expect("render");
    let scene = scene_for(&mut rule);

    assert!(hit_node_ids(&scene, 10.0, 10.0).is_empty());
}

#[test]
fn the_innermost_pointer_icon_wins_over_its_container() {
    let context = AppContext::new();
    let _scope = context.enter_scope();
    context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    rule.set_content(nested_icons_app).expect("render");
    let scene = scene_for(&mut rule);

    assert_eq!(
        topmost_pointer_icon(&scene, 10.0, 10.0),
        Some(PointerIcon::System(CursorIcon::EwResize)),
        "the pointer is over the inner box"
    );
    assert_eq!(
        topmost_pointer_icon(&scene, 120.0, 120.0),
        Some(PointerIcon::System(CursorIcon::Crosshair)),
        "the pointer is over the container only"
    );
}
