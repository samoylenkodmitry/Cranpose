pub mod tab_switch_regression_support;

use cranpose_animation::{animateFloatAsState, tween, Easing};
use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, MutableState};
use cranpose_foundation::PointerEvent;
use cranpose_macros::composable;
use cranpose_render_common::graph::ProjectiveTransform;
use cranpose_render_common::graph::{LayerNode, RenderNode};
use cranpose_render_common::graph_scene::{ClickAction, HitGeometry, Scene};
use cranpose_render_common::hit_graph::{collect_hits_from_graph, HitGraphSink};
use cranpose_render_common::{RenderScene, Renderer};
use cranpose_ui::{
    Button, ButtonSpec, LayoutTree, Modifier, SemanticsAction, SemanticsNode, SemanticsRole, Size,
    Text, TextStyle,
};
use cranpose_ui_graphics::{Point, Rect, RoundedCornerShape};
use desktop_app::app::{
    combined_app, DemoTab, TEST_ACTIVE_TAB_STATE, TEST_COUNTER_APP_COUNTER_STATE,
    TEST_LAZY_LIST_STATE, TEST_RECURSIVE_LAYOUT_DEPTH_STATE,
};
use std::rc::Rc;
use std::time::Duration;
use tab_switch_regression_support::{active_tab_state, pump_shell_until_stable};

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

fn max_layer_translation_y(layer: &LayerNode) -> f32 {
    layer.children.iter().fold(
        layer.graphics_layer.translation_y.abs(),
        |max_value, child| {
            let child_value = match child {
                RenderNode::Layer(child_layer) => max_layer_translation_y(child_layer),
                RenderNode::Primitive(_) => 0.0,
            };
            max_value.max(child_value)
        },
    )
}

#[composable]
fn returned_press_lift(press_tick: MutableState<u64>) -> f32 {
    let press_target = cranpose_core::useState(|| 0.0_f32);
    cranpose_core::LaunchedEffect!(press_tick.value(), {
        let target_state = press_target;
        let tick_state = press_tick;
        move |scope| {
            if tick_state.value() == 0 {
                return;
            }
            target_state.set(1.0);
            let reset_target_state = target_state;
            scope.launch_background(
                move |_| async move {
                    std::thread::sleep(Duration::from_millis(120));
                },
                move |_| reset_target_state.set(0.0),
            );
        }
    });

    animateFloatAsState(
        press_target.value(),
        tween(240, Easing::LinearEasing),
        "returned_button_graphics_layer_probe",
    )
    .value()
}

#[composable]
fn animate_float_returned_button_graphics_layer_probe() {
    let press_tick = cranpose_core::useState(|| 0_u64);
    let lift = returned_press_lift(press_tick);

    Button(
        Modifier::empty()
            .graphics_layer_block(move |layer| {
                layer.translation_y = lift * 32.0;
                layer.scale_x = 1.0 - lift * 0.05;
                layer.scale_y = 1.0 - lift * 0.05;
            })
            .height(48.0)
            .padding_symmetric(12.0, 8.0),
        ButtonSpec::default(),
        move || {
            press_tick.set(press_tick.value().wrapping_add(1));
        },
        move || {
            Text("Press probe", Modifier::empty(), TextStyle::default());
        },
    );
}

#[test]
fn animate_float_as_state_returned_value_updates_parent_graphics_layer_after_click() {
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        animate_float_returned_button_graphics_layer_probe,
    );
    shell.set_buffer_size(320, 180);
    shell.set_viewport(320.0, 180.0);
    pump_shell_until_stable(&mut shell);

    shell.set_cursor(32.0, 24.0);
    shell.pointer_pressed();
    shell.pointer_released();

    let mut saw_intermediate_layer = false;
    let mut last_translation = 0.0;
    for _ in 0..24 {
        std::thread::sleep(Duration::from_millis(16));
        shell.update();
        let translation = shell
            .scene()
            .graph
            .as_ref()
            .map(|graph| max_layer_translation_y(&graph.root))
            .expect("graph available after returned animation frame");
        last_translation = translation;
        if translation > 1.0 && translation < 31.0 {
            saw_intermediate_layer = true;
            break;
        }
    }

    assert!(
        saw_intermediate_layer,
        "animateFloatAsState returned from a child composable did not update the parent graphics_layer_block; last translation_y={last_translation}"
    );
}

fn semantics_text(node: &SemanticsNode) -> Option<&str> {
    match &node.role {
        SemanticsRole::Text { value } => Some(value.as_str()),
        _ => node.description.as_deref(),
    }
}

fn collect_layout_texts(node: &cranpose_ui::LayoutBox, out: &mut Vec<String>) {
    if let Some(text) = node.node_data.modifier_slices().text_content() {
        out.push(text.to_string());
    }
    for child in &node.children {
        collect_layout_texts(child, out);
    }
}

fn layout_tree_texts(tree: &LayoutTree) -> Vec<String> {
    let mut texts = Vec::new();
    collect_layout_texts(tree.root(), &mut texts);
    texts
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

fn semantics_texts(shell: &mut AppShell<HitGraphRenderer>) -> Vec<String> {
    let Some(root) = shell.semantics_tree().map(|tree| tree.root().clone()) else {
        return Vec::new();
    };

    let mut texts = Vec::new();
    collect_semantics_texts(&root, &mut texts);
    texts
}

fn async_progress_percent(texts: &[String]) -> Option<i32> {
    texts.iter().find_map(|text| {
        let suffix = text.strip_prefix("Progress: ")?;
        let digits = suffix.trim().trim_end_matches('%').trim();
        digits.parse::<i32>().ok()
    })
}

fn animation_alpha_value(texts: &[String]) -> Option<f32> {
    texts.iter().find_map(|text| {
        text.strip_prefix("Alpha ")
            .and_then(|value| value.parse::<f32>().ok())
    })
}

fn counter_state() -> MutableState<i32> {
    TEST_COUNTER_APP_COUNTER_STATE.with(|cell| {
        *cell
            .borrow()
            .as_ref()
            .expect("counter state should be registered")
    })
}

fn robot_move(shell: &mut AppShell<HitGraphRenderer>, x: f32, y: f32) {
    let _ = shell.set_cursor(x, y);
    pump_shell_until_stable(shell);
}

fn robot_click(shell: &mut AppShell<HitGraphRenderer>, x: f32, y: f32) {
    let _ = shell.set_cursor(x, y);
    assert!(
        shell.pointer_pressed(),
        "pointer down should hit a target at ({x}, {y})"
    );
    pump_shell_until_stable(shell);
    assert!(
        shell.pointer_released(),
        "pointer up should hit a target at ({x}, {y})"
    );
    pump_shell_until_stable(shell);
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

fn first_index_from_texts(texts: &[String]) -> Option<usize> {
    texts.iter().find_map(|text| {
        let value = text.strip_prefix("FirstIndex: ")?;
        value.trim().parse::<usize>().ok()
    })
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
    pump_shell_until_stable(&mut shell);

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
    let texts = semantics_texts(&mut shell);
    assert!(
        texts.iter().any(|text| text.contains("Counter: 1")),
        "counter label did not update after increment: {texts:?}",
    );
}

#[test]
fn lazy_list_jump_updates_first_index_in_combined_app_shell_robot_path() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());
    TEST_COUNTER_APP_COUNTER_STATE.with(|cell| cell.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, combined_app);
    shell.set_buffer_size(1200, 800);
    shell.set_viewport(1200.0, 800.0);
    shell.set_semantics_enabled(true);
    pump_shell_until_stable(&mut shell);

    active_tab_state().set(DemoTab::LazyList);
    pump_shell_until_stable(&mut shell);

    let initial_layout = layout_tree_texts(shell.layout_tree().expect("lazy list layout"));
    let initial_semantics = semantics_texts(&mut shell);
    assert_eq!(
        first_index_from_texts(&initial_layout),
        Some(0),
        "expected initial layout indicator to start at 0; texts={initial_layout:?}"
    );
    assert_eq!(
        first_index_from_texts(&initial_semantics),
        Some(0),
        "expected initial semantics indicator to start at 0; texts={initial_semantics:?}"
    );

    click_button_by_text(&mut shell, "Jump to Middle");
    pump_shell_until_stable(&mut shell);

    let layout_texts = layout_tree_texts(shell.layout_tree().expect("updated lazy list layout"));
    let semantics = semantics_texts(&mut shell);
    let layout_index = first_index_from_texts(&layout_texts);
    let semantics_index = first_index_from_texts(&semantics);

    assert!(
        layout_index.is_some_and(|index| (45..=55).contains(&index)),
        "expected layout indicator to jump near 50; layout_index={layout_index:?} texts={layout_texts:?}"
    );
    assert!(
        semantics_index.is_some_and(|index| (45..=55).contains(&index)),
        "expected semantics indicator to jump near 50; semantics_index={semantics_index:?} texts={semantics:?}"
    );
}

#[test]
fn lazy_list_direct_scroll_updates_first_index_in_combined_app_shell() {
    TEST_LAZY_LIST_STATE.with(|cell| cell.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, || {
        combined_app();
    });

    pump_shell_until_stable(&mut shell);
    active_tab_state().set(DemoTab::LazyList);
    pump_shell_until_stable(&mut shell);

    let list_state = TEST_LAZY_LIST_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .expect("lazy list state should be registered");
    list_state.scroll_to_item(50, 0.0);
    pump_shell_until_stable(&mut shell);

    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    let layout_index = first_index_from_texts(&layout_texts);
    assert!(
        layout_index.is_some_and(|index| index >= 45),
        "expected direct scroll to update combined-app layout indicator near 50; layout_index={layout_index:?} texts={layout_texts:?}"
    );
}

#[test]
fn recursive_layout_depth_six_survives_combined_app_shell_robot_path() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());
    TEST_RECURSIVE_LAYOUT_DEPTH_STATE.with(|cell| cell.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, combined_app);
    shell.set_buffer_size(1024, 768);
    shell.set_viewport(1024.0, 768.0);
    shell.set_semantics_enabled(true);
    pump_shell_until_stable(&mut shell);

    click_button_by_text(&mut shell, "Recursive Layout");
    assert_eq!(active_tab_state().get(), DemoTab::Layout);

    let mut texts = semantics_texts(&mut shell);
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Recursive Layout Playground")),
        "recursive layout tab content missing after tab click: {texts:?}",
    );

    for depth in 4..=6 {
        click_button_by_text(&mut shell, "Increase depth");
        texts = semantics_texts(&mut shell);
        assert!(
            texts
                .iter()
                .any(|text| text == &format!("Current depth: {depth}")),
            "recursive layout depth label stalled at step {depth}: {texts:?}",
        );
        assert!(
            texts.iter().any(|text| text == &format!("Depth {depth}")),
            "recursive layout root text missing at step {depth}: {texts:?}",
        );
    }
}

#[test]
fn async_runtime_progress_advances_after_animations_click_robot_path() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, combined_app);
    shell.set_buffer_size(1200, 800);
    shell.set_viewport(1200.0, 800.0);
    shell.set_semantics_enabled(true);
    pump_shell_until_stable(&mut shell);

    click_button_by_text(&mut shell, "Animations");
    let animations_texts = semantics_texts(&mut shell);
    assert!(
        animations_texts
            .iter()
            .any(|text| text.contains("Animations Showcase")),
        "animations tab should be visible after click; visible_texts={animations_texts:?}",
    );

    click_button_by_text(&mut shell, "Async Runtime");
    let initial_texts = semantics_texts(&mut shell);
    let initial_progress = async_progress_percent(&initial_texts)
        .expect("async progress text should be present after switching from Animations");

    let mut last_progress = initial_progress;
    let mut last_texts = initial_texts;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        shell.update();
        let texts = semantics_texts(&mut shell);
        let progress = async_progress_percent(&texts)
            .expect("async progress text should remain present while advancing");
        last_progress = progress;
        last_texts = texts;
        if progress != initial_progress {
            return;
        }
    }

    let debug = shell.debug_runtime_leak_stats();
    panic!(
        "async runtime progress stayed frozen on the robot click path after Animations -> Async: initial={initial_progress}% last={last_progress}% visible_texts={last_texts:?} runtime_stats={:?} slot_stats={:?} pass_stats={:?}",
        debug.runtime_stats,
        debug.slot_stats,
        debug.pass_stats,
    );
}

#[test]
fn async_runtime_progress_advances_after_animations_direct_state_switch_in_shell() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, combined_app);
    shell.set_buffer_size(1200, 800);
    shell.set_viewport(1200.0, 800.0);
    shell.set_semantics_enabled(true);
    pump_shell_until_stable(&mut shell);

    active_tab_state().set(DemoTab::Animations);
    pump_shell_until_stable(&mut shell);
    active_tab_state().set(DemoTab::Async);
    pump_shell_until_stable(&mut shell);

    let initial_texts = semantics_texts(&mut shell);
    let initial_progress = async_progress_percent(&initial_texts)
        .expect("async progress text should be present after direct state switch");

    let mut last_progress = initial_progress;
    let mut last_texts = initial_texts;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        shell.update();
        let texts = semantics_texts(&mut shell);
        let progress = async_progress_percent(&texts)
            .expect("async progress text should remain present while advancing");
        last_progress = progress;
        last_texts = texts;
        if progress != initial_progress {
            return;
        }
    }

    let debug = shell.debug_runtime_leak_stats();
    panic!(
        "async runtime progress stayed frozen on the shell direct-state path after Animations -> Async: initial={initial_progress}% last={last_progress}% visible_texts={last_texts:?} runtime_stats={:?} slot_stats={:?} pass_stats={:?}",
        debug.runtime_stats,
        debug.slot_stats,
        debug.pass_stats,
    );
}

#[test]
fn animations_tab_alpha_advances_on_shell_path() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, combined_app);
    shell.set_buffer_size(1200, 800);
    shell.set_viewport(1200.0, 800.0);
    shell.set_semantics_enabled(true);
    pump_shell_until_stable(&mut shell);

    active_tab_state().set(DemoTab::Animations);
    pump_shell_until_stable(&mut shell);

    let initial_texts = semantics_texts(&mut shell);
    let initial_alpha =
        animation_alpha_value(&initial_texts).expect("animations alpha text should be present");

    let mut last_alpha = initial_alpha;
    let mut last_texts = initial_texts;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        shell.update();
        let texts = semantics_texts(&mut shell);
        let alpha =
            animation_alpha_value(&texts).expect("animations alpha text should remain present");
        last_alpha = alpha;
        last_texts = texts;
        if (alpha - initial_alpha).abs() > 0.01 {
            return;
        }
    }

    let debug = shell.debug_runtime_leak_stats();
    panic!(
        "animations alpha stayed frozen on the shell path: initial={initial_alpha} last={last_alpha} visible_texts={last_texts:?} runtime_stats={:?} slot_stats={:?} pass_stats={:?}",
        debug.runtime_stats,
        debug.slot_stats,
        debug.pass_stats,
    );
}
