use crate::{layout::LayoutEngine, Column, ColumnSpec, Modifier, Row, RowSpec, Text};
use compose_core::{location_key, Composition, MemoryApplier, MutableState, NodeId};
use compose_macros::composable;
use std::cell::Cell;

thread_local! {
    static PROGRESS_TAB_RENDERS: Cell<usize> = Cell::new(0);
    static PROGRESS_BAR_BRANCH_CALLS: Cell<usize> = Cell::new(0);
}

fn expected_layout_counts(depth: usize, horizontal: bool) -> (usize, usize) {
    if depth <= 1 {
        return (1, 0);
    }
    let (left_total, left_two) = expected_layout_counts(depth - 1, !horizontal);
    let (right_total, right_two) = expected_layout_counts(depth - 1, !horizontal);
    if horizontal {
        let total = 1 + 1 + left_total + right_total; // column + row + children
        let two = 1 + left_two + right_two; // row has two layout children
        (total, two)
    } else {
        let total = 1 + left_total + right_total; // column only
        let two = 1 + left_two + right_two; // column has two layout children
        (total, two)
    }
}

fn reset_progress_counters() {
    PROGRESS_TAB_RENDERS.with(|c| c.set(0));
    PROGRESS_BAR_BRANCH_CALLS.with(|c| c.set(0));
}

#[composable]
fn progress_tab(progress: MutableState<f32>) {
    PROGRESS_TAB_RENDERS.with(|c| c.set(c.get() + 1));
    let progress_value = progress.value();

    Column(Modifier::empty().padding(8.0), ColumnSpec::default(), move || {
        Text(
            format!("Progress {:.2}", progress_value),
            Modifier::empty().padding(2.0),
        );

        Row(
            Modifier::empty().padding(2.0).then(Modifier::empty().height(12.0)),
            RowSpec::default(),
            {
                move || {
                    if progress_value > 0.0 {
                        PROGRESS_BAR_BRANCH_CALLS.with(|c| c.set(c.get() + 1));
                        Row(
                            Modifier::empty().width(200.0 * progress_value).then(Modifier::empty().height(12.0)),
                            RowSpec::default(),
                            || {},
                        );
                    }
                }
            },
        );
    });
}

#[composable]
fn summary_tab() {
    Column(Modifier::empty().padding(8.0), ColumnSpec::default(), move || {
        Text("Summary Tab", Modifier::empty().padding(2.0));
    });
}

fn make_tab_renderer(active_tab: MutableState<i32>, progress: MutableState<f32>) -> impl FnMut() {
    move || match active_tab.value() {
        0 => progress_tab(progress.clone()),
        _ => summary_tab(),
    }
}

#[test]
fn tab_switching_restores_conditional_layout_nodes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let progress = MutableState::with_runtime(0.75f32, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    let mut render = make_tab_renderer(active_tab.clone(), progress.clone());

    reset_progress_counters();
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert!(
        PROGRESS_TAB_RENDERS.with(|c| c.get()) > 0,
        "progress tab should render on initial composition"
    );
    assert!(
        PROGRESS_BAR_BRANCH_CALLS.with(|c| c.get()) > 0,
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
    reset_progress_counters();
    composition
        .render(key, &mut render)
        .expect("render primary tab after switch");
    assert!(
        PROGRESS_TAB_RENDERS.with(|c| c.get()) > 0,
        "progress tab should render after switching back"
    );
    assert!(
        PROGRESS_BAR_BRANCH_CALLS.with(|c| c.get()) > 0,
        "conditional progress bar should rebuild after switching back"
    );
}

#[test]
fn tab_switching_multiple_toggle_cycles_stays_responsive() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let progress = MutableState::with_runtime(0.4f32, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    let mut render = make_tab_renderer(active_tab.clone(), progress.clone());

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
        reset_progress_counters();
        composition
            .render(key, &mut render)
            .expect("render progress tab after switch");
        assert!(
            PROGRESS_TAB_RENDERS.with(|c| c.get()) > 0,
            "cycle {cycle}: progress tab should render after returning"
        );
    }
}

#[test]
fn tab_switching_layout_pass_handles_conditional_nodes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let progress = MutableState::with_runtime(0.8f32, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    let mut render = make_tab_renderer(active_tab.clone(), progress.clone());

    composition
        .render(key, &mut render)
        .expect("initial render");

    // Collapse the conditional branch, then switch tabs.
    progress.set_value(0.0);
    composition
        .process_invalid_scopes()
        .expect("collapse progress to zero");

    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("render inactive summary tab");

    // Update progress while the tab is hidden, simulating async state changes.
    progress.set_value(0.55);
    composition
        .process_invalid_scopes()
        .expect("update progress while hidden");

    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("render progress tab again");

    // Layout should succeed without missing node errors.
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
    Column(Modifier::empty().padding(6.0), ColumnSpec::default(), move || {
        Text(label.clone(), Modifier::empty().padding(2.0));
        if depth > 1 {
            if horizontal {
                Row(Modifier::empty().fill_max_width(), RowSpec::default(), move || {
                    for child_idx in 0..2 {
                        let child_index = index * 2 + child_idx + 1;
                        compose_core::with_key(&(depth, index, child_idx), || {
                            alternating_recursive_node(depth - 1, false, child_index);
                        });
                    }
                });
            } else {
                Column(
                    Modifier::empty().fill_max_width(),
                    ColumnSpec::default(),
                    move || {
                        for child_idx in 0..2 {
                            let child_index = index * 2 + child_idx + 1;
                            compose_core::with_key(&(depth, index, child_idx), || {
                                alternating_recursive_node(depth - 1, true, child_index);
                            });
                        }
                    },
                );
            }
        }
    });
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
    Column(Modifier::empty().padding(4.0), ColumnSpec::default(), move || {
        Text("Counter placeholder", Modifier::empty().padding(2.0));
    });
}

#[composable]
fn keyed_tab_switcher(active: MutableState<RecursiveDemoTab>, depth_state: MutableState<usize>) {
    let active_value = active.get();
    let depth_state_for_layout = depth_state.clone();
    Column(Modifier::empty().padding(8.0), ColumnSpec::default(), move || {
        compose_core::with_key(&active_value, || match active_value {
            RecursiveDemoTab::Counter => simple_counter_placeholder(),
            RecursiveDemoTab::Layout => {
                recursive_layout_root(depth_state_for_layout.clone());
            }
        });
    });
}

#[test]
fn recursive_layout_nodes_preserve_extent() {
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
            LayoutNodeKind::Text { .. }
            | LayoutNodeKind::Spacer
            | LayoutNodeKind::Button { .. }
            | LayoutNodeKind::Unknown => {}
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
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(2usize, runtime.clone());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, &mut || {
            recursive_layout_root(depth_state.clone());
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
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(RecursiveDemoTab::Counter, runtime.clone());
    let depth_state = MutableState::with_runtime(3usize, runtime.clone());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, &mut || {
            keyed_tab_switcher(active_tab.clone(), depth_state.clone())
        })
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
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(3usize, runtime.clone());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, &mut || {
            recursive_layout_root(depth_state.clone());
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
