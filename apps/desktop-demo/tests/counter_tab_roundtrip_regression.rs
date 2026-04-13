use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, MutableState};
use cranpose_foundation::PointerEvent;
use cranpose_render_common::graph::ProjectiveTransform;
use cranpose_render_common::graph_scene::{ClickAction, HitGeometry, Scene};
use cranpose_render_common::hit_graph::{collect_hits_from_graph, HitGraphSink};
use cranpose_render_common::{RenderScene, Renderer};
use cranpose_ui::{LayoutTree, SemanticsAction, SemanticsNode, SemanticsRole, Size};
use cranpose_ui_graphics::{Point, Rect, RoundedCornerShape};
use desktop_app::app::{
    combined_app, DemoTab, TEST_ACTIVE_TAB_STATE, TEST_COUNTER_APP_COUNTER_STATE,
};
use std::rc::Rc;

#[derive(Default)]
struct HitGraphRenderer {
    scene: Scene,
}

struct SceneHitSink<'a> {
    scene: &'a mut Scene,
}

impl HitGraphSink for SceneHitSink<'_> {
    fn push_hit(
        &mut self,
        node_id: cranpose_core::NodeId,
        capture_path: &[cranpose_core::NodeId],
        geometry: HitGeometry,
        shape: Option<RoundedCornerShape>,
        click_actions: &[Rc<dyn Fn(Point)>],
        pointer_inputs: &[Rc<dyn Fn(PointerEvent)>],
    ) {
        self.scene.push_hit(
            node_id,
            capture_path.to_vec(),
            geometry,
            shape,
            click_actions
                .iter()
                .cloned()
                .map(ClickAction::WithPoint)
                .collect(),
            pointer_inputs.to_vec(),
        );
    }
}

fn collect_graph_hits(
    layer: &cranpose_render_common::graph::LayerNode,
    scene: &mut Scene,
    parent_hit_clip: Option<Rect>,
) {
    let mut sink = SceneHitSink { scene };
    collect_hits_from_graph(
        layer,
        ProjectiveTransform::identity(),
        &mut sink,
        parent_hit_clip,
    );
}

impl Renderer for HitGraphRenderer {
    type Scene = Scene;
    type Error = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        let graph = cranpose_render_common::scene_builder::build_graph_from_layout_tree(
            layout_tree.root(),
            1.0,
        );
        collect_graph_hits(&graph.root, &mut self.scene, None);
        self.scene.replace_graph(graph);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        if let Some(graph) =
            cranpose_render_common::scene_builder::build_graph_from_applier(applier, root, 1.0)
        {
            collect_graph_hits(&graph.root, &mut self.scene, None);
            self.scene.replace_graph(graph);
        }
        Ok(())
    }
}

fn semantics_text(node: &SemanticsNode) -> Option<&str> {
    match &node.role {
        SemanticsRole::Text { value } => Some(value.as_str()),
        _ => node.description.as_deref(),
    }
}

fn is_clickable(node: &SemanticsNode) -> bool {
    node.actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }))
}

fn subtree_contains_text(node: &SemanticsNode, text: &str) -> bool {
    semantics_text(node)
        .map(|value| value.contains(text))
        .unwrap_or(false)
        || node
            .children
            .iter()
            .any(|child| subtree_contains_text(child, text))
}

fn collect_matching_buttons<'a>(
    node: &'a SemanticsNode,
    text: &str,
    out: &mut Vec<&'a SemanticsNode>,
) {
    if is_clickable(node) && subtree_contains_text(node, text) {
        out.push(node);
    }
    for child in &node.children {
        collect_matching_buttons(child, text, out);
    }
}

fn button_bounds(
    shell: &mut AppShell<HitGraphRenderer>,
    text: &str,
) -> Vec<(u64, f32, f32, f32, f32)> {
    let Some(root) = shell.semantics_tree().map(|tree| tree.root().clone()) else {
        return Vec::new();
    };

    let mut matches = Vec::new();
    collect_matching_buttons(&root, text, &mut matches);
    matches
        .into_iter()
        .filter_map(|node| {
            shell
                .node_layout_bounds(node.node_id)
                .map(|(x, y, w, h)| (node.node_id as u64, x, y, w, h))
        })
        .collect()
}

fn collect_semantics_texts(node: &SemanticsNode, out: &mut Vec<String>) {
    if let Some(text) = semantics_text(node) {
        out.push(text.to_string());
    }
    for child in &node.children {
        collect_semantics_texts(child, out);
    }
}

fn semantics_texts(shell: &AppShell<HitGraphRenderer>) -> Vec<String> {
    let Some(root) = shell.semantics_tree().map(|tree| tree.root().clone()) else {
        return Vec::new();
    };

    let mut texts = Vec::new();
    collect_semantics_texts(&root, &mut texts);
    texts
}

fn counter_state() -> MutableState<i32> {
    TEST_COUNTER_APP_COUNTER_STATE.with(|cell| {
        *cell
            .borrow()
            .as_ref()
            .expect("counter state should be registered")
    })
}

fn active_tab_state() -> MutableState<DemoTab> {
    TEST_ACTIVE_TAB_STATE.with(|cell| {
        *cell
            .borrow()
            .as_ref()
            .expect("active tab state should be registered")
    })
}

fn pump_until_stable(shell: &mut AppShell<HitGraphRenderer>) {
    for _ in 0..80 {
        if !(shell.needs_redraw() || shell.has_active_animations()) {
            break;
        }
        shell.update();
    }
}

fn robot_move(shell: &mut AppShell<HitGraphRenderer>, x: f32, y: f32) {
    let _ = shell.set_cursor(x, y);
    pump_until_stable(shell);
}

fn robot_click(shell: &mut AppShell<HitGraphRenderer>, x: f32, y: f32) {
    let _ = shell.set_cursor(x, y);
    assert!(
        shell.pointer_pressed(),
        "pointer down should hit a target at ({x}, {y})"
    );
    pump_until_stable(shell);
    assert!(
        shell.pointer_released(),
        "pointer up should hit a target at ({x}, {y})"
    );
    pump_until_stable(shell);
}

fn click_button_by_text(shell: &mut AppShell<HitGraphRenderer>, text: &str) {
    let matches = button_bounds(shell, text);
    assert!(
        !matches.is_empty(),
        "expected semantics button {text:?}; found none"
    );
    let (_, x, y, w, h) = matches[0];
    robot_click(shell, x + w * 0.5, y + h * 0.5);
}

#[test]
fn counter_increment_survives_combined_app_tab_roundtrip_robot_path() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());
    TEST_COUNTER_APP_COUNTER_STATE.with(|cell| cell.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, combined_app);
    shell.set_buffer_size(800, 600);
    shell.set_viewport(800.0, 600.0);
    shell.set_semantics_enabled(true);
    pump_until_stable(&mut shell);

    assert_eq!(active_tab_state().get(), DemoTab::Counter);
    assert_eq!(counter_state().get(), 0);

    click_button_by_text(&mut shell, "CompositionLocal Test");
    assert_eq!(active_tab_state().get(), DemoTab::CompositionLocal);

    TEST_COUNTER_APP_COUNTER_STATE.with(|cell| {
        cell.borrow_mut().take();
    });
    click_button_by_text(&mut shell, "Counter App");
    assert_eq!(active_tab_state().get(), DemoTab::Counter);
    let rerendered_counter_state = TEST_COUNTER_APP_COUNTER_STATE.with(|cell| *cell.borrow());
    assert!(
        rerendered_counter_state.is_some(),
        "counter_app should register a fresh counter state when returning to the counter tab"
    );
    assert_eq!(counter_state().get(), 0);

    for step in 0..20 {
        let progress = step as f32 / 19.0;
        let x = 78.0 + (80.0 - 78.0) * progress;
        let y = 51.8 + (230.0 - 51.8) * progress;
        robot_move(&mut shell, x, y);
    }

    let increment_buttons = button_bounds(&mut shell, "Increment");
    assert_eq!(
        increment_buttons.len(),
        1,
        "expected exactly one Increment button after tab roundtrip, got {increment_buttons:?}"
    );

    let (_, x, y, w, h) = increment_buttons[0];
    robot_move(&mut shell, x + w * 0.5, y + h * 0.5);
    robot_click(&mut shell, x + w * 0.5, y + h * 0.5);

    assert_eq!(counter_state().get(), 1);
    let texts = semantics_texts(&shell);
    assert!(
        texts.iter().any(|text| text.contains("Counter: 1")),
        "counter label did not update after increment: {texts:?}",
    );
}
