pub mod tab_switch_regression_support;

use cranpose_core::{location_key, Composition, MemoryApplier, NodeError};
use cranpose_testing::robot::{create_headless_robot_test, RobotTestRule, TestRenderer};
use cranpose_ui::{
    measure_layout_with_options, Brush, Color, HeadlessRenderer, LayoutBox, MeasureLayoutOptions,
    RecordedRenderScene, RenderOp, SemanticsNode, SemanticsRole, Size,
};
use cranpose_ui_graphics::DrawPrimitive;
use desktop_app::app::{
    combined_app, DemoTab, TEST_ACTIVE_TAB_STATE, TEST_COMPOSITION_LOCAL_COUNTER,
};
use tab_switch_regression_support::{
    pump_robot_until_stable, set_active_tab, wait_for_active_tab_registration_robot,
};

fn drain_all(composition: &mut Composition<MemoryApplier>) -> Result<(), NodeError> {
    loop {
        if !composition.process_invalid_scopes()? {
            break;
        }
    }
    Ok(())
}

fn semantics_node_text(node: &SemanticsNode) -> Option<&str> {
    match &node.role {
        SemanticsRole::Text { value } => Some(value.as_str()),
        _ => node.description.as_deref(),
    }
}

fn find_semantic_node<'a>(node: &'a SemanticsNode, text: &str) -> Option<&'a SemanticsNode> {
    if semantics_node_text(node) == Some(text) {
        return Some(node);
    }

    node.children
        .iter()
        .find_map(|child| find_semantic_node(child, text))
}

fn find_semantic_node_containing<'a>(
    node: &'a SemanticsNode,
    fragment: &str,
) -> Option<&'a SemanticsNode> {
    if semantics_node_text(node)
        .map(|text| text.contains(fragment))
        .unwrap_or(false)
    {
        return Some(node);
    }

    node.children
        .iter()
        .find_map(|child| find_semantic_node_containing(child, fragment))
}

fn layout_texts(robot: &mut RobotTestRule<TestRenderer>) -> Vec<String> {
    robot.get_all_text()
}

fn collect_layout_text(node: &LayoutBox, out: &mut Vec<String>) {
    if let Some(text) = node.node_data.modifier_slices().text_content() {
        out.push(text.to_string());
    }
    for child in &node.children {
        collect_layout_text(child, out);
    }
}

fn composition_layout_texts(composition: &mut Composition<MemoryApplier>) -> Vec<String> {
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let measurements = measure_layout_with_options(
        &mut applier,
        root,
        Size::new(1200.0, 800.0),
        MeasureLayoutOptions {
            collect_semantics: false,
            build_layout_tree: true,
        },
    )
    .expect("measure layout for test rule");
    applier.clear_runtime_handle();
    let layout_tree = measurements.layout_tree();
    let mut texts = Vec::new();
    collect_layout_text(layout_tree.root(), &mut texts);
    texts
}

fn assert_composition_contains_text(
    composition: &mut Composition<MemoryApplier>,
    needle: &str,
    context: &str,
) {
    let texts = composition_layout_texts(composition);
    assert!(
        texts.iter().any(|text| text.contains(needle)),
        "expected text marker {needle:?} {context}; visible_texts={texts:?}",
    );
}

fn live_slot_count(slots: &[cranpose_core::SlotDebugEntry]) -> usize {
    slots.len()
}

fn async_progress_percent(texts: &[String]) -> Option<i32> {
    texts.iter().find_map(|text| {
        let suffix = text.strip_prefix("Progress: ")?;
        let digits = suffix.strip_suffix('%')?.trim();
        digits.parse::<i32>().ok()
    })
}

fn async_direction(texts: &[String]) -> Option<&str> {
    texts.iter().find_map(|text| {
        if !text.starts_with("Frames advanced: ") {
            return None;
        }
        text.rsplit_once("direction: ")
            .map(|(_, direction)| direction.trim_end_matches(')'))
    })
}

fn animation_alpha_value(texts: &[String]) -> Option<f32> {
    texts.iter().find_map(|text| {
        text.strip_prefix("Alpha ")
            .and_then(|value| value.parse::<f32>().ok())
    })
}

fn animation_offset_value(texts: &[String]) -> Option<f32> {
    texts.iter().find_map(|text| {
        text.strip_prefix("Offset ")
            .and_then(|value| value.parse::<f32>().ok())
    })
}

fn render_scene(composition: &mut Composition<MemoryApplier>) -> RecordedRenderScene {
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let measurements = measure_layout_with_options(
        &mut applier,
        root,
        Size::new(1200.0, 800.0),
        MeasureLayoutOptions {
            collect_semantics: false,
            build_layout_tree: true,
        },
    )
    .expect("measure layout for render scene");
    applier.clear_runtime_handle();
    HeadlessRenderer::new().render(&measurements.layout_tree())
}

fn advance_async_runtime_to_second_forward_half(
    composition: &mut Composition<MemoryApplier>,
    frame_time: &mut u64,
) {
    let mut reached_upper_peak = false;
    let mut returned_to_zero = false;
    let mut last_progress = None;
    let mut last_direction = None;
    let mut last_texts = Vec::new();

    for _ in 0..2400 {
        *frame_time += 16_666_667;
        composition
            .runtime_handle()
            .drain_frame_callbacks(*frame_time);
        drain_all(composition).expect("advance async runtime frame");

        let texts = composition_layout_texts(composition);
        let progress = async_progress_percent(&texts).unwrap_or_else(|| {
            panic!("async progress text missing while advancing: visible_texts={texts:?}")
        });
        let direction = async_direction(&texts).unwrap_or_else(|| {
            panic!("async direction text missing while advancing: visible_texts={texts:?}")
        });
        let direction_owned = direction.to_string();
        last_progress = Some(progress);
        last_direction = Some(direction_owned.clone());
        last_texts = texts;

        if progress >= 95 {
            reached_upper_peak = true;
        }
        if reached_upper_peak && progress <= 2 {
            returned_to_zero = true;
        }
        if returned_to_zero && (45..=60).contains(&progress) {
            return;
        }
    }

    panic!(
        "did not reach async runtime second forward pass near half progress; reached_upper_peak={reached_upper_peak} returned_to_zero={returned_to_zero} last_progress={last_progress:?} last_direction={last_direction:?} visible_texts={last_texts:?}"
    );
}

fn assert_layout_contains_text(
    robot: &mut RobotTestRule<TestRenderer>,
    needle: &str,
    context: &str,
) {
    let texts = layout_texts(robot);
    assert!(
        texts.iter().any(|text| text.contains(needle)),
        "expected text marker {needle:?} {context}; visible_texts={texts:?}",
    );
}

fn assert_lazy_list_viewport(robot: &mut RobotTestRule<TestRenderer>, context: &str) {
    let semantics_root = robot
        .shell_mut()
        .semantics_tree()
        .map(|tree| tree.root().clone())
        .expect("lazy list semantics tree");
    let viewport = find_semantic_node(&semantics_root, "LazyListViewport")
        .unwrap_or_else(|| panic!("LazyListViewport semantics {context}"));
    let row = find_semantic_node(&semantics_root, "ItemRow #0")
        .unwrap_or_else(|| panic!("ItemRow #0 semantics {context}"));
    let hello = find_semantic_node(&semantics_root, "Hello #0")
        .unwrap_or_else(|| panic!("Hello #0 semantics {context}"));

    let root_size = robot
        .shell_mut()
        .root_layout_size()
        .unwrap_or_else(|| panic!("root layout size {context}"));
    let viewport_bounds = robot
        .shell_mut()
        .node_layout_bounds(viewport.node_id)
        .unwrap_or_else(|| panic!("LazyListViewport bounds {context}"));
    let row_bounds = robot
        .shell_mut()
        .node_layout_bounds(row.node_id)
        .unwrap_or_else(|| panic!("ItemRow #0 bounds {context}"));
    let hello_bounds = robot
        .shell_mut()
        .node_layout_bounds(hello.node_id)
        .unwrap_or_else(|| panic!("Hello #0 bounds {context}"));

    assert!(
        viewport_bounds.3 >= (root_size.1 * 0.35).max(220.0),
        "lazy viewport height regressed {context}: root={root_size:?} viewport={viewport_bounds:?}",
    );
    assert!(
        (viewport_bounds.2 - row_bounds.2).abs() <= 2.0,
        "first row width should match lazy viewport {context}: viewport={viewport_bounds:?} row={row_bounds:?}",
    );
    assert!(
        hello_bounds.1 >= row_bounds.1 + row_bounds.3 - 1.0,
        "secondary lazy item content should remain below the row {context}: row={row_bounds:?} hello={hello_bounds:?}",
    );
}

fn assert_animations_tab_visible(robot: &mut RobotTestRule<TestRenderer>, context: &str) {
    let semantics_root = robot
        .shell_mut()
        .semantics_tree()
        .map(|tree| tree.root().clone())
        .expect("animations semantics tree");
    let visible_texts = semantics_root
        .children
        .iter()
        .filter_map(semantics_node_text)
        .collect::<Vec<_>>();
    assert!(
        find_semantic_node(&semantics_root, "Animations Showcase").is_some(),
        "animations header should remain visible {context}: visible_texts={visible_texts:?}",
    );
    assert!(
        find_semantic_node_containing(&semantics_root, "Alpha ").is_some(),
        "animations alpha content should remain visible {context}: visible_texts={visible_texts:?}",
    );
    assert!(
        find_semantic_node_containing(&semantics_root, "Lazy Pulse:").is_some(),
        "animations lazy pulse content should remain visible {context}: visible_texts={visible_texts:?}",
    );
}

#[test]
fn lazy_list_initial_keeps_full_lazy_viewport() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut robot = create_headless_robot_test(1200, 800, || {
        combined_app();
    });
    robot.shell_mut().set_semantics_enabled(true);
    wait_for_active_tab_registration_robot(&mut robot);
    set_active_tab(DemoTab::LazyList);
    robot.wait_for_idle();

    assert_lazy_list_viewport(&mut robot, "when started directly on LazyList");
}

#[test]
fn mineswapper_to_lazy_list_keeps_full_lazy_viewport() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut robot = create_headless_robot_test(1200, 800, || {
        combined_app();
    });
    robot.shell_mut().set_semantics_enabled(true);
    wait_for_active_tab_registration_robot(&mut robot);

    let registered = TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow().is_some());
    assert!(registered, "active tab state was not registered");

    set_active_tab(DemoTab::Mineswapper2);
    robot.wait_for_idle();
    set_active_tab(DemoTab::LazyList);
    robot.wait_for_idle();

    assert_lazy_list_viewport(&mut robot, "after switching from Mineswapper2");
}

#[test]
fn animations_to_lazy_list_keeps_full_lazy_viewport() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut robot = create_headless_robot_test(1200, 800, || {
        combined_app();
    });
    robot.shell_mut().set_semantics_enabled(true);
    wait_for_active_tab_registration_robot(&mut robot);

    let registered = TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow().is_some());
    assert!(registered, "active tab state was not registered");

    set_active_tab(DemoTab::Animations);
    robot.wait_for_idle();
    set_active_tab(DemoTab::LazyList);
    robot.wait_for_idle();

    assert_lazy_list_viewport(&mut robot, "after switching from Animations");
}

#[test]
fn animations_tab_renders_showcase_content() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut robot = create_headless_robot_test(1200, 800, || {
        combined_app();
    });
    robot.shell_mut().set_semantics_enabled(true);
    wait_for_active_tab_registration_robot(&mut robot);

    let registered = TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow().is_some());
    assert!(registered, "active tab state was not registered");

    set_active_tab(DemoTab::Animations);
    for _ in 0..10 {
        robot.wait_for_idle();
    }

    assert_animations_tab_visible(&mut robot, "after switching to Animations");
}

#[test]
fn animations_to_composition_local_survives_local_recompose() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());
    TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| cell.borrow_mut().take());

    let mut robot = create_headless_robot_test(1200, 800, || {
        combined_app();
    });
    robot.shell_mut().set_semantics_enabled(true);
    wait_for_active_tab_registration_robot(&mut robot);

    let registered = TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow().is_some());
    assert!(registered, "active tab state was not registered");

    set_active_tab(DemoTab::Animations);
    robot.wait_for_idle();
    set_active_tab(DemoTab::CompositionLocal);
    robot.wait_for_idle();

    let counter_state = TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| {
        *cell
            .borrow()
            .as_ref()
            .expect("composition local counter registered")
    });

    let semantics_root = robot
        .shell_mut()
        .semantics_tree()
        .map(|tree| tree.root().clone())
        .expect("composition local semantics tree before increment");
    assert!(
        find_semantic_node(&semantics_root, "Increment").is_some(),
        "increment button should remain visible after switching from Animations",
    );
    assert!(
        find_semantic_node_containing(&semantics_root, "READING local").is_some(),
        "reading local content should remain visible after switching from Animations",
    );

    counter_state.update(|value| *value += 1);
    robot.wait_for_idle();

    let semantics_root = robot
        .shell_mut()
        .semantics_tree()
        .map(|tree| tree.root().clone())
        .expect("composition local semantics tree after increment");
    assert!(
        find_semantic_node(&semantics_root, "Increment").is_some(),
        "increment button should survive composition local recompose after Animations",
    );
    assert!(
        find_semantic_node_containing(&semantics_root, "READING local").is_some(),
        "reading local content should survive composition local recompose after Animations",
    );
}

#[test]
fn animations_to_other_tabs_preserve_tab_content_markers() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());
    TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| cell.borrow_mut().take());

    let mut robot = create_headless_robot_test(1200, 800, || {
        combined_app();
    });
    robot.shell_mut().set_semantics_enabled(true);
    wait_for_active_tab_registration_robot(&mut robot);

    set_active_tab(DemoTab::Animations);
    pump_robot_until_stable(&mut robot, 40);
    assert_animations_tab_visible(&mut robot, "before leaving Animations");

    let tab_markers = [
        (DemoTab::Counter, "Cranpose Playground"),
        (DemoTab::CompositionLocal, "READING local:"),
        (DemoTab::Async, "Async Runtime Demo"),
        (DemoTab::WebFetch, "Fetch data from the web"),
        (DemoTab::TextInput, "Text Input Demo"),
        (DemoTab::Layout, "Recursive Layout Playground"),
        (DemoTab::ModifierShowcase, "Select Showcase"),
        (DemoTab::LazyList, "Lazy List Demo"),
        (DemoTab::Mineswapper2, "Mineswapper 2"),
        (DemoTab::HackerNews, "Hacker News"),
        (DemoTab::Images, "Chessboard Image Demo"),
        (DemoTab::Text, "Text Rendering Feature Showcase"),
        (DemoTab::Winamp, "Stopped | pos"),
        (DemoTab::Xkcd, "XKCD Random Comic"),
        (DemoTab::Shaders, "Shaders & Effects"),
        (DemoTab::ShaderRect, "Fire Shader"),
        (
            DemoTab::MarkdownViewer,
            "Enter a URL pointing to a raw Markdown file and press Fetch.",
        ),
    ];

    for (tab, marker) in tab_markers {
        set_active_tab(tab);
        pump_robot_until_stable(&mut robot, 60);
        assert_layout_contains_text(
            &mut robot,
            marker,
            &format!("after switching from Animations to {:?}", tab),
        );
    }
}

#[test]
fn animations_to_other_tabs_preserve_tab_content_markers_in_composition() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());
    TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| cell.borrow_mut().take());

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, combined_app)
        .expect("install combined app content");
    drain_all(&mut composition).expect("initial drain");

    set_active_tab(DemoTab::Animations);
    drain_all(&mut composition).expect("switch to animations tab");
    assert_composition_contains_text(&mut composition, "Animations Showcase", "before tab walk");

    let tab_markers = [
        (DemoTab::Counter, "Cranpose Playground"),
        (DemoTab::CompositionLocal, "READING local:"),
        (DemoTab::Async, "Async Runtime Demo"),
        (DemoTab::WebFetch, "Fetch data from the web"),
    ];

    for (tab, marker) in tab_markers {
        set_active_tab(tab);
        drain_all(&mut composition)
            .unwrap_or_else(|_| panic!("switch to {:?} in composition", tab));
        assert_composition_contains_text(
            &mut composition,
            marker,
            &format!(
                "after switching from Animations to {:?} in direct composition",
                tab
            ),
        );
    }
}

#[test]
fn animations_tab_keeps_tree_after_frame_recomposition() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, combined_app)
        .expect("install combined app content");

    let registered = TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow().is_some());
    assert!(registered, "active tab state was not registered");

    set_active_tab(DemoTab::Animations);
    let first_recompose = composition
        .process_invalid_scopes()
        .expect("first recomposition after switching to animations");
    assert!(
        first_recompose,
        "switch to animations should invalidate a scope"
    );

    drain_all(&mut composition).expect("switch to animations tab and settle");

    let before_groups = composition.debug_dump_slot_table_groups();

    let mut frame_time = 0u64;
    for _ in 0..12 {
        frame_time += 16_666_667;
        composition
            .runtime_handle()
            .drain_frame_callbacks(frame_time);
        drain_all(&mut composition).expect("advance animation frame and settle");
    }

    let after_slots = composition.debug_dump_slot_entries();
    let after_groups = composition.debug_dump_slot_table_groups();
    assert!(
        after_groups.len() >= before_groups.len().saturating_sub(8),
        "animations slots collapsed after frame recomposition\n--- before groups ---\n{before_groups:#?}\n--- after groups ---\n{after_groups:#?}\n--- after slots ---\n{after_slots:#?}"
    );
}

#[test]
fn animations_tab_advances_display_values_after_switch() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, combined_app)
        .expect("install combined app content");
    drain_all(&mut composition).expect("initial drain");

    set_active_tab(DemoTab::Animations);
    drain_all(&mut composition).expect("switch to animations tab");

    let initial_texts = composition_layout_texts(&mut composition);
    let initial_alpha =
        animation_alpha_value(&initial_texts).expect("initial animations alpha text");
    let initial_offset =
        animation_offset_value(&initial_texts).expect("initial animations offset text");

    let runtime = composition.runtime_handle();
    let mut time = 0u64;
    let mut saw_alpha_change = false;
    let mut saw_offset_change = false;
    let mut last_texts = initial_texts;

    for _ in 0..240 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("advance animations frame");

        let texts = composition_layout_texts(&mut composition);
        let alpha = animation_alpha_value(&texts).expect("animations alpha text");
        let offset = animation_offset_value(&texts).expect("animations offset text");
        saw_alpha_change |= (alpha - initial_alpha).abs() > 0.01;
        saw_offset_change |= (offset - initial_offset).abs() > 0.5;
        last_texts = texts;

        if saw_alpha_change && saw_offset_change {
            break;
        }
    }

    assert!(
        saw_alpha_change,
        "animations alpha value never changed after switching tabs: initial={initial_alpha} texts={last_texts:?}",
    );
    assert!(
        saw_offset_change,
        "animations offset value never changed after switching tabs: initial={initial_offset} texts={last_texts:?}",
    );
}

#[test]
fn async_runtime_progress_advances_after_switching_from_animations() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, combined_app)
        .expect("install combined app content");
    drain_all(&mut composition).expect("initial drain");

    set_active_tab(DemoTab::Animations);
    drain_all(&mut composition).expect("switch to animations tab");
    assert_composition_contains_text(
        &mut composition,
        "Animations Showcase",
        "before switching to async runtime",
    );

    set_active_tab(DemoTab::Async);
    drain_all(&mut composition).expect("switch to async runtime tab");

    let runtime = composition.runtime_handle();
    let initial_texts = composition_layout_texts(&mut composition);
    let initial_progress = async_progress_percent(&initial_texts)
        .expect("initial async progress text should be present after tab switch");
    let mut last_progress = initial_progress;
    let mut last_texts = initial_texts;
    let mut time = 0u64;

    for _ in 0..180 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("advance async runtime after animations switch");

        let texts = composition_layout_texts(&mut composition);
        let progress = async_progress_percent(&texts)
            .expect("async progress text should remain present while advancing");
        last_progress = progress;
        last_texts = texts;

        if progress != initial_progress {
            return;
        }
    }

    panic!(
        "async runtime progress stayed frozen after Animations -> Async switch: initial={initial_progress}% last={last_progress}% visible_texts={last_texts:?}"
    );
}

#[test]
fn recursive_layout_tab_renders_accent_box_chrome() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, combined_app)
        .expect("install combined app content");
    drain_all(&mut composition).expect("initial drain");

    set_active_tab(DemoTab::Layout);
    drain_all(&mut composition).expect("switch to recursive layout tab");

    let scene = render_scene(&mut composition);
    let accent_colors = [
        Color(0.25, 0.32, 0.58, 0.75),
        Color(0.30, 0.20, 0.45, 0.75),
        Color(0.20, 0.40, 0.32, 0.75),
        Color(0.45, 0.28, 0.24, 0.75),
    ];

    let accent_boxes = scene
        .operations()
        .iter()
        .filter(|op| match op {
            RenderOp::Primitive { primitive, .. } => match primitive {
                DrawPrimitive::Rect {
                    brush: Brush::Solid(color),
                    ..
                }
                | DrawPrimitive::RoundRect {
                    brush: Brush::Solid(color),
                    ..
                } => accent_colors.contains(color),
                DrawPrimitive::Blend { primitive, .. } => match primitive.as_ref() {
                    DrawPrimitive::Rect {
                        brush: Brush::Solid(color),
                        ..
                    }
                    | DrawPrimitive::RoundRect {
                        brush: Brush::Solid(color),
                        ..
                    } => accent_colors.contains(color),
                    _ => false,
                },
                _ => false,
            },
            RenderOp::Text { .. } => false,
        })
        .count();

    assert!(
        accent_boxes >= 4,
        "recursive layout tab should render accent box chrome after tab switch; accent_boxes={accent_boxes} scene={scene:#?}",
    );
}

#[test]
fn async_runtime_to_other_tabs_after_second_forward_pass_preserves_content() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, combined_app)
        .expect("install combined app content");
    drain_all(&mut composition).expect("initial drain");
    let registered = TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow().is_some());
    assert!(registered, "active tab state was not registered");

    let tab_markers = [
        (DemoTab::Animations, "Animations Showcase"),
        (DemoTab::TextInput, "Text Input Demo"),
        (DemoTab::ModifierShowcase, "Select Showcase"),
    ];
    let mut frame_time = 0u64;

    for (target_tab, marker) in tab_markers {
        set_active_tab(DemoTab::Async);
        drain_all(&mut composition).expect("switch to async runtime tab before frame advancement");
        let slots_before_wait_dump = composition.debug_dump_slot_entries();
        let live_slots_before_wait = live_slot_count(&slots_before_wait_dump);
        advance_async_runtime_to_second_forward_half(&mut composition, &mut frame_time);
        let slots_after_wait_dump = composition.debug_dump_slot_entries();
        let live_slots_after_wait = live_slot_count(&slots_after_wait_dump);
        assert_composition_contains_text(
            &mut composition,
            "Async Runtime Demo",
            "before leaving async tab",
        );
        assert_composition_contains_text(
            &mut composition,
            "Pause animation",
            "async controls disappeared before tab switch",
        );
        assert_composition_contains_text(
            &mut composition,
            "Reset",
            "async reset button disappeared before tab switch",
        );
        assert!(
            live_slots_after_wait <= live_slots_before_wait + 32,
            "async runtime animation leaked live slots before tab switch: before={live_slots_before_wait} after={live_slots_after_wait}",
        );

        set_active_tab(target_tab);
        drain_all(&mut composition).expect("switch away from async runtime tab");
        assert_composition_contains_text(
            &mut composition,
            marker,
            &format!(
                "after switching away from Async Runtime during second forward pass to {:?}",
                target_tab
            ),
        );
    }
}
