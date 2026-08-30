use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{Composition, MemoryApplier, MutableState, NodeId, location_key};
use cranpose_macros::composable;

use crate::{
    Box, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec, ScrollState, Size, Text, TextStyle,
    layout::{LayoutEngine, MeasureLayoutOptions},
    measure_layout_with_options,
};

fn expected_layout_counts(depth: usize, horizontal: bool) -> (usize, usize) {
    if depth <= 1 {
        return (1, 0);
    }
    let (left_total, left_two) = expected_layout_counts(depth - 1, !horizontal);
    let (right_total, right_two) = expected_layout_counts(depth - 1, !horizontal);
    if horizontal {
        let total = 1 + 1 + left_total + right_total;
        let two = 1 + 1 + left_two + right_two;
        (total, two)
    } else {
        let total = 1 + left_total + right_total;
        let two = 1 + 1 + left_two + right_two;
        (total, two)
    }
}

fn composition_layout_texts(composition: &mut Composition<MemoryApplier>) -> Vec<String> {
    fn collect(node: &crate::LayoutBox, out: &mut Vec<String>) {
        if let Some(text) = node.node_data.modifier_slices().text_content() {
            out.push(text.to_string());
        }
        for child in &node.children {
            collect(child, out);
        }
    }

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
    .expect("measure layout for tab switching test");
    applier.clear_runtime_handle();
    let mut texts = Vec::new();
    let layout_tree = measurements
        .layout_tree()
        .expect("tab switching test requested a layout tree");
    collect(layout_tree.root(), &mut texts);
    texts
}

fn drain_all(composition: &mut Composition<MemoryApplier>) {
    while composition
        .process_invalid_scopes()
        .expect("drain invalid scopes")
    {}
}

fn set_recursive_depth(
    composition: &mut Composition<MemoryApplier>,
    depth_state: MutableState<usize>,
    depth: usize,
) {
    depth_state.set_value(depth);
    drain_all(composition);
}

fn run_recursive_depth_cycle(
    composition: &mut Composition<MemoryApplier>,
    depth_state: MutableState<usize>,
    min_depth: usize,
    max_depth: usize,
) {
    for depth in (min_depth + 1)..=max_depth {
        set_recursive_depth(composition, depth_state, depth);
    }
    for depth in (min_depth..max_depth).rev() {
        set_recursive_depth(composition, depth_state, depth);
    }
}

#[composable]
fn progress_tab(
    progress: MutableState<f32>,
    renders: Rc<Cell<usize>>,
    branch_calls: Rc<Cell<usize>>,
) {
    renders.set(renders.get() + 1);
    let progress_value = progress.value();

    Column(
        Modifier::empty().padding(8.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Progress {:.2}", progress_value),
                Modifier::empty().padding(2.0),
                TextStyle::default(),
            );

            Row(
                Modifier::empty()
                    .padding(2.0)
                    .then(Modifier::empty().height(12.0)),
                RowSpec::default(),
                {
                    let branch_calls = Rc::clone(&branch_calls);
                    move || {
                        if progress_value > 0.0 {
                            branch_calls.set(branch_calls.get() + 1);
                            Row(
                                Modifier::empty()
                                    .width(200.0 * progress_value)
                                    .then(Modifier::empty().height(12.0)),
                                RowSpec::default(),
                                || {},
                            );
                        }
                    }
                },
            );
        },
    );
}

#[composable]
fn summary_tab() {
    Column(
        Modifier::empty().padding(8.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Summary Tab",
                Modifier::empty().padding(2.0),
                TextStyle::default(),
            );
        },
    );
}

fn make_tab_renderer(
    active_tab: MutableState<i32>,
    progress: MutableState<f32>,
    renders: Rc<Cell<usize>>,
    branch_calls: Rc<Cell<usize>>,
) -> impl FnMut() {
    move || match active_tab.value() {
        0 => progress_tab(progress, Rc::clone(&renders), Rc::clone(&branch_calls)),
        _ => summary_tab(),
    }
}

#[composable]
fn scrollable_test_tab(label: &'static str) {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll_state, false),
        ColumnSpec::default(),
        move || {
            Text(label, Modifier::empty().padding(8.0), TextStyle::default());
            Text(
                "Scrollable content",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn wrapped_stateful_counter_tab(
    restored_counter: Rc<RefCell<Option<MutableState<i32>>>>,
    restored_pointer: Rc<RefCell<Option<MutableState<i32>>>>,
) {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    let counter = cranpose_core::rememberMutableStateOf(|| 0i32);
    let pointer = cranpose_core::rememberMutableStateOf(|| 0i32);
    let is_even = counter.value() % 2 == 0;
    *restored_counter.borrow_mut() = Some(counter);
    *restored_pointer.borrow_mut() = Some(pointer);

    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll_state, false),
        ColumnSpec::default(),
        move || {
            cranpose_core::with_key(&is_even, || {
                Text(
                    if is_even { "even" } else { "odd" },
                    Modifier::empty().padding(8.0),
                    TextStyle::default(),
                );
            });
            Text(
                format!("Pointer value {}", pointer.value()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
            Text(
                format!("Counter value {}", counter.value()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn counter_test_tab(counter: MutableState<i32>) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            "Counter Tab",
            Modifier::empty().padding(8.0),
            TextStyle::default(),
        );
        Text(
            format!("Counter value {}", counter.value()),
            Modifier::empty().padding(8.0),
            TextStyle::default(),
        );
    });
}

#[composable]
fn mixed_scrollable_tab_host(active_tab: MutableState<i32>, counter: MutableState<i32>) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Row(Modifier::empty(), RowSpec::default(), || {
                Text("Tab A", Modifier::empty(), TextStyle::default());
                Text("Tab B", Modifier::empty(), TextStyle::default());
                Text("Tab C", Modifier::empty(), TextStyle::default());
            });

            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    let active = active_tab.value();
                    cranpose_core::with_key(&active, || match active {
                        0 => counter_test_tab(counter),
                        1 => scrollable_test_tab("Composition Local Marker"),
                        2 => scrollable_test_tab("Web Fetch Marker"),
                        _ => scrollable_test_tab("Async Marker"),
                    });
                },
            );
        },
    );
}

#[composable]
fn wrapped_tab_switching_host(
    active_tab: MutableState<i32>,
    restored_counter: Rc<RefCell<Option<MutableState<i32>>>>,
    restored_pointer: Rc<RefCell<Option<MutableState<i32>>>>,
) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Row(Modifier::empty(), RowSpec::default(), || {
                Text("Tab A", Modifier::empty(), TextStyle::default());
                Text("Tab B", Modifier::empty(), TextStyle::default());
                Text("Tab C", Modifier::empty(), TextStyle::default());
            });

            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                {
                    let restored_counter = Rc::clone(&restored_counter);
                    let restored_pointer = Rc::clone(&restored_pointer);
                    move || {
                        let active = active_tab.value();
                        cranpose_core::with_key(&active, || match active {
                            0 => wrapped_stateful_counter_tab(
                                Rc::clone(&restored_counter),
                                Rc::clone(&restored_pointer),
                            ),
                            1 => scrollable_test_tab("Composition Local Marker"),
                            _ => scrollable_test_tab("Web Fetch Marker"),
                        });
                    }
                },
            );
        },
    );
}

#[test]
fn tab_switching_restores_conditional_layout_nodes() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let progress = MutableState::with_runtime(0.75f32, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    let renders = Rc::new(Cell::new(0));
    let branch_calls = Rc::new(Cell::new(0));
    let mut render = make_tab_renderer(
        active_tab,
        progress,
        Rc::clone(&renders),
        Rc::clone(&branch_calls),
    );

    composition
        .render(key, &mut render)
        .expect("initial render");
    assert!(
        renders.get() > 0,
        "progress tab should render on initial composition"
    );
    assert!(
        branch_calls.get() > 0,
        "conditional progress bar should be built on initial render"
    );

    progress.set_value(0.0);
    composition
        .process_invalid_scopes()
        .expect("recompose after collapsing progress");

    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("render secondary tab");

    progress.set_value(0.65);
    composition
        .process_invalid_scopes()
        .expect("update progress while hidden");

    active_tab.set_value(0);
    renders.set(0);
    branch_calls.set(0);
    composition
        .render(key, &mut render)
        .expect("render primary tab after switch");
    assert!(
        renders.get() > 0,
        "progress tab should render after switching back"
    );
    assert!(
        branch_calls.get() > 0,
        "conditional progress bar should rebuild after switching back"
    );
}

#[test]
fn scrollable_tab_host_preserves_content_across_mixed_switches() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(3i32, runtime.clone());
    let counter = MutableState::with_runtime(0i32, runtime);
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, || mixed_scrollable_tab_host(active_tab, counter))
        .expect("initial render");
    drain_all(&mut composition);

    active_tab.set_value(0);
    drain_all(&mut composition);
    let counter_texts = composition_layout_texts(&mut composition);
    assert!(
        counter_texts
            .iter()
            .any(|text| text.contains("Counter value 0")),
        "counter tab content missing after first switch: {counter_texts:?}",
    );

    active_tab.set_value(1);
    drain_all(&mut composition);
    let first_scrollable = composition_layout_texts(&mut composition);
    assert!(
        first_scrollable
            .iter()
            .any(|text| text.contains("Composition Local Marker")),
        "first scrollable tab content missing: {first_scrollable:?}",
    );

    active_tab.set_value(2);
    drain_all(&mut composition);
    let second_scrollable = composition_layout_texts(&mut composition);
    assert!(
        second_scrollable
            .iter()
            .any(|text| text.contains("Web Fetch Marker")),
        "second scrollable tab content missing after mixed switches: {second_scrollable:?}",
    );
}

#[test]
fn restored_wrapped_counter_tab_updates_after_mixed_tab_walk() {
    let _app_context = crate::render_state::app_context_test_scope();
    let restored_counter = Rc::new(RefCell::new(None));
    let restored_pointer = Rc::new(RefCell::new(None));

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime);
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, {
            let restored_counter = Rc::clone(&restored_counter);
            let restored_pointer = Rc::clone(&restored_pointer);
            move || {
                wrapped_tab_switching_host(
                    active_tab,
                    Rc::clone(&restored_counter),
                    Rc::clone(&restored_pointer),
                )
            }
        })
        .expect("initial render");
    drain_all(&mut composition);

    let initial = composition_layout_texts(&mut composition);
    assert!(
        initial.iter().any(|text| text.contains("Counter value 0")),
        "wrapped counter tab missing initial content: {initial:?}",
    );
    assert!(
        initial.iter().any(|text| text == "even"),
        "wrapped counter tab missing initial even branch: {initial:?}",
    );

    for (tab, marker) in [
        (1, "Composition Local Marker"),
        (2, "Web Fetch Marker"),
        (1, "Composition Local Marker"),
        (2, "Web Fetch Marker"),
    ] {
        active_tab.set_value(tab);
        drain_all(&mut composition);
        let texts = composition_layout_texts(&mut composition);
        assert!(
            texts.iter().any(|text| text.contains(marker)),
            "tab {tab} content missing after mixed walk: {texts:?}",
        );
    }

    active_tab.set_value(0);
    drain_all(&mut composition);
    let restored_counter = restored_counter
        .borrow()
        .as_ref()
        .copied()
        .expect("restored counter state registered");
    let restored_pointer = restored_pointer
        .borrow()
        .as_ref()
        .copied()
        .expect("restored pointer state registered");

    restored_pointer.set_value(1);
    drain_all(&mut composition);
    restored_counter.set_value(1);
    drain_all(&mut composition);

    let updated = composition_layout_texts(&mut composition);
    assert!(
        updated.iter().any(|text| text.contains("Pointer value 1")),
        "wrapped counter pointer state did not update after restore: {updated:?}",
    );
    assert!(
        updated.iter().any(|text| text == "odd"),
        "wrapped counter branch did not update after mixed tab walk: {updated:?}",
    );
    assert!(
        updated.iter().any(|text| text.contains("Counter value 1")),
        "wrapped counter state did not update after mixed tab walk: {updated:?}",
    );
}

#[test]
fn tab_switching_multiple_toggle_cycles_stays_responsive() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let progress = MutableState::with_runtime(0.4f32, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    let renders = Rc::new(Cell::new(0));
    let branch_calls = Rc::new(Cell::new(0));
    let mut render = make_tab_renderer(
        active_tab,
        progress,
        Rc::clone(&renders),
        Rc::clone(&branch_calls),
    );

    composition
        .render(key, &mut render)
        .expect("initial render");

    for cycle in 0..6 {
        active_tab.set_value(1);
        composition
            .render(key, &mut render)
            .expect("render summary tab");

        progress.set_value(if cycle % 2 == 0 { 0.0 } else { 0.9 });
        composition
            .process_invalid_scopes()
            .expect("process progress change while hidden");

        active_tab.set_value(0);
        renders.set(0);
        composition
            .render(key, &mut render)
            .expect("render progress tab after switch");
        assert!(
            renders.get() > 0,
            "cycle {cycle}: progress tab should render after returning"
        );
    }
}

#[test]
fn tab_switching_layout_pass_handles_conditional_nodes() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let progress = MutableState::with_runtime(0.8f32, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    let mut render = make_tab_renderer(
        active_tab,
        progress,
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
    );

    composition
        .render(key, &mut render)
        .expect("initial render");

    progress.set_value(0.0);
    composition
        .process_invalid_scopes()
        .expect("collapse progress to zero");

    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("render inactive summary tab");

    progress.set_value(0.55);
    composition
        .process_invalid_scopes()
        .expect("update progress while hidden");

    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("render progress tab again");

    let root = composition
        .root()
        .expect("composition should have a root node");
    let mut applier = composition.applier_mut();
    let viewport = crate::modifier::Size {
        width: 800.0,
        height: 600.0,
    };
    applier
        .compute_layout(root, viewport)
        .expect("layout computation should succeed after tab switch");
}

#[composable]
fn alternating_recursive_node(depth: usize, horizontal: bool, index: usize) {
    let label = format!("Node {index} depth {depth}");
    Column(
        Modifier::empty().padding(6.0),
        ColumnSpec::default(),
        move || {
            Text(
                label.clone(),
                Modifier::empty().padding(2.0),
                TextStyle::default(),
            );
            if depth > 1 {
                if horizontal {
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::default(),
                        move || {
                            for child_idx in 0..2 {
                                let child_index = index * 2 + child_idx + 1;
                                cranpose_core::with_key(&(depth, index, child_idx), || {
                                    alternating_recursive_node(depth - 1, false, child_index);
                                });
                            }
                        },
                    );
                } else {
                    Column(
                        Modifier::empty().fill_max_width(),
                        ColumnSpec::default(),
                        move || {
                            for child_idx in 0..2 {
                                let child_index = index * 2 + child_idx + 1;
                                cranpose_core::with_key(&(depth, index, child_idx), || {
                                    alternating_recursive_node(depth - 1, true, child_index);
                                });
                            }
                        },
                    );
                }
            }
        },
    );
}

#[composable]
fn recursive_layout_root(depth_state: MutableState<usize>) {
    let depth = depth_state.get();
    alternating_recursive_node(depth, true, 0);
}

fn layout_two_child_stats(composition: &mut Composition<MemoryApplier>) -> (usize, usize) {
    let root = composition.root().expect("composition has root");
    let mut applier = composition.applier_mut();
    let layout = applier
        .compute_layout(
            root,
            crate::modifier::Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");
    fn count(node: &crate::layout::LayoutBox, stats: &mut Vec<(NodeId, usize)>) -> (usize, usize) {
        use crate::layout::LayoutNodeKind;
        let layout_children: Vec<_> = node
            .children
            .iter()
            .filter(|child| {
                matches!(
                    child.node_data.kind,
                    LayoutNodeKind::Layout | LayoutNodeKind::Subcompose
                )
            })
            .collect();
        let mut two_child = if layout_children.len() == 2 { 1 } else { 0 };
        let mut unknown = if matches!(node.node_data.kind, LayoutNodeKind::Unknown) {
            1
        } else {
            0
        };
        for child in &layout_children {
            let (child_two, child_unknown) = count(child, stats);
            two_child += child_two;
            unknown += child_unknown;
        }
        stats.push((node.node_id, layout_children.len()));
        (two_child, unknown)
    }
    let mut stats = Vec::new();
    let result = count(layout.root(), &mut stats);
    if cfg!(debug_assertions) {
        eprintln!("layout stats: {:?}", stats);
    }
    result
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum RecursiveDemoTab {
    Counter,
    Layout,
}

#[composable]
fn simple_counter_placeholder() {
    Column(
        Modifier::empty().padding(4.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Counter placeholder",
                Modifier::empty().padding(2.0),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn keyed_tab_switcher(active: MutableState<RecursiveDemoTab>, depth_state: MutableState<usize>) {
    let active_value = active.get();
    let depth_state_for_layout = depth_state;
    Column(
        Modifier::empty().padding(8.0),
        ColumnSpec::default(),
        move || {
            cranpose_core::with_key(&active_value, || match active_value {
                RecursiveDemoTab::Counter => simple_counter_placeholder(),
                RecursiveDemoTab::Layout => {
                    recursive_layout_root(depth_state_for_layout);
                }
            });
        },
    );
}

#[test]
fn recursive_layout_nodes_preserve_extent() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut || alternating_recursive_node(4, true, 0))
        .expect("initial render");

    let root = composition
        .root()
        .expect("composition should retain a root node");
    let mut applier = composition.applier_mut();
    let layout = applier
        .compute_layout(
            root,
            crate::modifier::Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout should succeed");

    fn assert_positive_extents(box_node: &crate::layout::LayoutBox) {
        use crate::layout::LayoutNodeKind;
        match box_node.node_data.kind {
            LayoutNodeKind::Layout | LayoutNodeKind::Subcompose => {
                assert!(
                    box_node.rect.width > 0.0 && box_node.rect.height > 0.0,
                    "layout node {} has zero extent",
                    box_node.node_id
                );
            }
            LayoutNodeKind::Spacer | LayoutNodeKind::Button { .. } | LayoutNodeKind::Unknown => {}
        }
        for child in &box_node.children {
            assert_positive_extents(child);
        }
    }

    assert_positive_extents(layout.root());
    fn count_layout_nodes(node: &crate::layout::LayoutBox) -> (usize, usize) {
        use crate::layout::LayoutNodeKind;
        let layout_children: Vec<_> = node
            .children
            .iter()
            .filter(|child| {
                matches!(
                    child.node_data.kind,
                    LayoutNodeKind::Layout | LayoutNodeKind::Subcompose
                )
            })
            .collect();
        let mut total = 1usize;
        let mut two_child = if layout_children.len() == 2 { 1 } else { 0 };
        for child in layout_children {
            let (child_total, child_two) = count_layout_nodes(child);
            total += child_total;
            two_child += child_two;
        }
        (total, two_child)
    }

    let (_actual_total, actual_two) = count_layout_nodes(layout.root());
    let (_expected_total, expected_two) = expected_layout_counts(4, true);
    assert_eq!(
        actual_two, expected_two,
        "branching layout nodes lost children"
    );
}

#[test]
fn recursive_layout_updates_keep_all_branches() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(2usize, runtime.clone());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, &mut || {
            recursive_layout_root(depth_state);
        })
        .expect("initial render");

    let (baseline_two, baseline_unknown) = layout_two_child_stats(&mut composition);
    assert_eq!(
        baseline_unknown, 0,
        "unexpected unknown layout nodes in baseline"
    );

    depth_state.set_value(4);
    while composition
        .process_invalid_scopes()
        .expect("recompose after depth increase")
    {}

    let (updated_two, updated_unknown) = layout_two_child_stats(&mut composition);
    assert_eq!(
        updated_unknown, 0,
        "unexpected unknown layout nodes after update"
    );
    let (_, expected_two) = expected_layout_counts(4, true);
    assert_eq!(
        baseline_two,
        expected_layout_counts(2, true).1,
        "baseline tree mismatch"
    );
    assert_eq!(
        updated_two, expected_two,
        "branch nodes missing after depth increase"
    );
}

#[test]
fn tab_switching_recursive_layout_preserves_branches() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(RecursiveDemoTab::Counter, runtime.clone());
    let depth_state = MutableState::with_runtime(3usize, runtime.clone());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, &mut || keyed_tab_switcher(active_tab, depth_state))
        .expect("initial render");

    active_tab.set(RecursiveDemoTab::Layout);
    while composition
        .process_invalid_scopes()
        .expect("process switch to recursive tab")
    {}

    let (initial_two, initial_unknown) = layout_two_child_stats(&mut composition);
    assert_eq!(
        initial_unknown, 0,
        "unexpected unknown nodes after first switch to recursive tab"
    );
    let (_, expected_two_initial) = expected_layout_counts(3, true);
    assert_eq!(
        initial_two, expected_two_initial,
        "recursive layout lost branches on first switch"
    );
    depth_state.set(4);
    while composition
        .process_invalid_scopes()
        .expect("process depth increase while layout active")
    {}

    active_tab.set(RecursiveDemoTab::Counter);
    while composition
        .process_invalid_scopes()
        .expect("process switch back to counter tab")
    {}

    active_tab.set(RecursiveDemoTab::Layout);
    while composition
        .process_invalid_scopes()
        .expect("process second switch to recursive tab")
    {}

    let (two_count, unknown) = layout_two_child_stats(&mut composition);
    assert_eq!(
        unknown, 0,
        "unexpected unknown nodes after second switch to recursive tab"
    );
    let (_, expected_two) = expected_layout_counts(4, true);
    assert_eq!(
        two_count, expected_two,
        "recursive layout lost branches after second switch"
    );
}

#[test]
fn recursive_layout_depth_decrease_then_increase_restores_branches() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(3usize, runtime.clone());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, &mut || {
            recursive_layout_root(depth_state);
        })
        .expect("initial render");

    let (baseline_two, baseline_unknown) = layout_two_child_stats(&mut composition);
    assert_eq!(
        baseline_unknown, 0,
        "unexpected unknown nodes at baseline depth"
    );
    let (_, expected_two_depth3) = expected_layout_counts(3, true);
    assert_eq!(
        baseline_two, expected_two_depth3,
        "baseline layout tree mismatch at depth 3"
    );

    depth_state.set(2);
    while composition
        .process_invalid_scopes()
        .expect("recompose after depth decrease")
    {}

    let (decreased_two, decreased_unknown) = layout_two_child_stats(&mut composition);
    assert_eq!(
        decreased_unknown, 0,
        "unexpected unknown nodes after depth decrease"
    );
    let (_, expected_two_depth2) = expected_layout_counts(2, true);
    assert_eq!(
        decreased_two, expected_two_depth2,
        "layout tree mismatch after decreasing depth"
    );

    depth_state.set(3);
    while composition
        .process_invalid_scopes()
        .expect("recompose after depth increase")
    {}

    let (restored_two, restored_unknown) = layout_two_child_stats(&mut composition);
    assert_eq!(
        restored_unknown, 0,
        "unexpected unknown nodes after increasing depth again"
    );
    assert_eq!(
        restored_two, expected_two_depth3,
        "layout tree mismatch after re-increasing depth"
    );
}

#[test]
fn tab_switching_node_vec_does_not_grow_unboundedly() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let progress = MutableState::with_runtime(0.5f32, runtime);

    let key = location_key(file!(), line!(), column!());
    let mut render = make_tab_renderer(
        active_tab,
        progress,
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
    );

    composition.render(key, &mut render).expect("initial");
    active_tab.set_value(1);
    while composition.process_invalid_scopes().expect("switch") {}
    active_tab.set_value(0);
    while composition.process_invalid_scopes().expect("switch back") {}

    let baseline_active = composition.applier_mut().len();
    let baseline_capacity = composition.applier_mut().capacity();
    let baseline_slots = composition.debug_dump_slot_entries().len();

    for _ in 0..50 {
        active_tab.set_value(1);
        while composition.process_invalid_scopes().expect("to tab 1") {}
        active_tab.set_value(0);
        while composition.process_invalid_scopes().expect("to tab 0") {}
    }

    let final_active = composition.applier_mut().len();
    let final_capacity = composition.applier_mut().capacity();
    let final_slots = composition.debug_dump_slot_entries().len();

    assert_eq!(
        baseline_active, final_active,
        "active node count should be stable across tab cycles"
    );
    assert!(
        final_capacity <= baseline_capacity * 2,
        "node vec capacity grew from {} to {} ({:.1}x) over 50 cycles - indicates unbounded growth",
        baseline_capacity,
        final_capacity,
        final_capacity as f64 / baseline_capacity as f64,
    );
    assert!(
        final_slots <= baseline_slots + 10,
        "slot count grew from {} to {} over 50 cycles - indicates unbounded slot growth",
        baseline_slots,
        final_slots,
    );
}

#[test]
fn depth_cycling_node_vec_does_not_grow_unboundedly() {
    let _app_context = crate::render_state::app_context_test_scope();
    const MIN_DEPTH: usize = 3;
    const MAX_DEPTH: usize = 5;
    const DEPTH_CYCLES: usize = 2;

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(MIN_DEPTH, runtime);
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, &mut || recursive_layout_root(depth_state))
        .expect("initial render");

    run_recursive_depth_cycle(&mut composition, depth_state, MIN_DEPTH, MAX_DEPTH);

    let baseline_active = composition.applier_mut().len();
    let baseline_capacity = composition.applier_mut().capacity();
    let baseline_tombstones = composition.applier_mut().tombstone_count();

    for cycle in 0..DEPTH_CYCLES {
        run_recursive_depth_cycle(&mut composition, depth_state, MIN_DEPTH, MAX_DEPTH);
        assert_eq!(
            composition.applier_mut().len(),
            baseline_active,
            "active node count changed during depth cycle {cycle}"
        );
    }

    let final_active = composition.applier_mut().len();
    let final_capacity = composition.applier_mut().capacity();
    let final_tombstones = composition.applier_mut().tombstone_count();

    assert_eq!(
        baseline_active, final_active,
        "active node count should be stable across depth cycles"
    );
    assert!(
        final_capacity <= baseline_capacity + 32,
        "node vec capacity grew from {} to {} over {} depth cycles ({} new tombstones) - \
         indicates MemoryApplier does not reuse freed indices",
        baseline_capacity,
        final_capacity,
        DEPTH_CYCLES,
        final_tombstones - baseline_tombstones,
    );
}
