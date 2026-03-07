use super::*;
use cranpose_core::NodeId;
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use cranpose_ui_graphics::Rect;
use cranpose_ui_graphics::Size as ViewportSize;
use std::cell::RefCell;

thread_local! {
    static LAST_LAZY_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
}

#[test]
fn lazy_column_unbounded_height_matches_effective_viewport() {
    let mut composition = run_test_composition(|| {
        let list_state = remember_lazy_list_state();
        LAST_LAZY_STATE.with(|cell| {
            *cell.borrow_mut() = Some(list_state);
        });

        LazyColumn(
            Modifier::empty(),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(
                    100,
                    None::<fn(usize) -> u64>,
                    None::<fn(usize) -> u64>,
                    |_| {
                        Spacer(Size {
                            width: 0.0,
                            height: 100.0,
                        });
                    },
                );
            },
        );
    });

    let root = composition.root().expect("lazy column root");
    let handle = composition.runtime_handle();
    let measurements = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let result = measure_layout(
            &mut applier,
            root,
            ViewportSize {
                width: 320.0,
                height: f32::INFINITY,
            },
        )
        .expect("layout measurement");
        applier.clear_runtime_handle();
        result
    };

    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let expected_height = list_state.layout_info().viewport_size;
    let actual_height = measurements.root_size().height;

    assert!(actual_height.is_finite());
    assert!(
        (actual_height - expected_height).abs() < 0.01,
        "expected lazy column height to match effective viewport {expected_height}, got {actual_height}"
    );

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

fn measure_tree(
    composition: &mut TestComposition,
    root: NodeId,
    size: ViewportSize,
) -> crate::LayoutTree {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = measure_layout(&mut applier, root, size)
        .expect("layout measurement")
        .into_layout_tree();
    applier.clear_runtime_handle();
    layout
}

fn collect_visible_item_texts(
    scene: &crate::renderer::RecordedRenderScene,
    viewport: Rect,
) -> Vec<(usize, f32)> {
    let mut items = Vec::new();
    for operation in scene.operations() {
        let crate::renderer::RenderOp::Text { rect, value, .. } = operation else {
            continue;
        };
        if !value.starts_with("Item ") {
            continue;
        }
        let intersects_vertically =
            rect.y < viewport.y + viewport.height && rect.y + rect.height > viewport.y;
        if !intersects_vertically {
            continue;
        }
        let Some(index) = value
            .strip_prefix("Item ")
            .and_then(|value| value.parse().ok())
        else {
            continue;
        };
        items.push((index, rect.y));
    }
    items.sort_by(|left, right| left.1.partial_cmp(&right.1).expect("finite y"));
    items
}

#[test]
fn lazy_column_variable_height_reverse_scroll_keeps_rendered_items_ordered() {
    let mut composition = run_test_composition(|| {
        let list_state = remember_lazy_list_state();
        LAST_LAZY_STATE.with(|cell| {
            *cell.borrow_mut() = Some(list_state);
        });

        LazyColumn(
            Modifier::empty(),
            list_state,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            |scope| {
                scope.items(
                    120,
                    None::<fn(usize) -> u64>,
                    None::<fn(usize) -> u64>,
                    |index| {
                        let height = match index % 9 {
                            0 => 32.0,
                            1 => 48.0,
                            2 => 240.0,
                            3 => 56.0,
                            4 => 72.0,
                            5 => 180.0,
                            6 => 40.0,
                            7 => 96.0,
                            _ => 56.0,
                        };
                        Column(
                            Modifier::empty().fill_max_width().height(height),
                            ColumnSpec::default(),
                            move || {
                                Text(
                                    format!("Item {}", index),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    },
                );
            },
        );
    });

    let root = composition.root().expect("lazy column root");
    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let viewport_size = ViewportSize {
        width: 320.0,
        height: 260.0,
    };
    let renderer = HeadlessRenderer::new();
    let deltas = [
        -180.0, -180.0, -220.0, -150.0, -240.0, -120.0, -160.0, 60.0, 60.0, 80.0, -96.0, -96.0,
        44.0, 44.0, 44.0, -140.0, -140.0, 72.0, 72.0, 72.0, 72.0,
    ];
    let mut last_top_item: Option<usize> = None;

    for (step, delta) in deltas.into_iter().enumerate() {
        list_state.dispatch_scroll_delta(delta);
        let layout = measure_tree(&mut composition, root, viewport_size);
        let visible = collect_visible_item_texts(
            &renderer.render(&layout),
            Rect {
                x: 0.0,
                y: 0.0,
                width: viewport_size.width,
                height: viewport_size.height,
            },
        );

        assert!(
            !visible.is_empty(),
            "step {step}: expected at least one visible item after delta {delta}"
        );

        for pair in visible.windows(2) {
            assert!(
                pair[1].0 > pair[0].0,
                "step {step}: rendered item order regressed after delta {delta}: {:?}",
                visible
            );
        }

        if let Some(previous_top_item) = last_top_item {
            let current_top_item = visible[0].0;
            if delta < 0.0 {
                assert!(
                    current_top_item >= previous_top_item,
                    "step {step}: forward scroll moved top item backward from {previous_top_item} to {current_top_item}"
                );
            } else if delta > 0.0 {
                assert!(
                    current_top_item <= previous_top_item,
                    "step {step}: reverse scroll backtracked from top item {previous_top_item} to {current_top_item}"
                );
            }
        }

        last_top_item = Some(visible[0].0);
    }

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[test]
fn lazy_column_content_type_reuse_reverse_scroll_keeps_rendered_items_ordered() {
    let mut composition = run_test_composition(|| {
        let list_state = remember_lazy_list_state();
        LAST_LAZY_STATE.with(|cell| {
            *cell.borrow_mut() = Some(list_state);
        });

        LazyColumn(
            Modifier::empty(),
            list_state,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
            |scope| {
                scope.items(
                    240,
                    None::<fn(usize) -> u64>,
                    Some(|index: usize| (index % 5) as u64),
                    |index| {
                        let height = match index % 5 {
                            0 => 44.0,
                            1 => 72.0,
                            2 => 96.0,
                            3 => 56.0,
                            _ => 128.0,
                        };
                        Column(
                            Modifier::empty().fill_max_width().height(height),
                            ColumnSpec::default(),
                            move || {
                                Text(
                                    format!("Item {}", index),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    },
                );
            },
        );
    });

    let root = composition.root().expect("lazy column root");
    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let viewport_size = ViewportSize {
        width: 320.0,
        height: 260.0,
    };
    let renderer = HeadlessRenderer::new();
    let deltas = [
        -220.0, -220.0, -180.0, -200.0, -240.0, -160.0, -140.0, 96.0, 96.0, 72.0, 72.0, 64.0,
        -128.0, -128.0, 88.0, 88.0, 88.0, -144.0, -144.0, 104.0, 104.0, 104.0,
    ];
    let mut last_top_item: Option<usize> = None;

    for (step, delta) in deltas.into_iter().enumerate() {
        list_state.dispatch_scroll_delta(delta);
        let layout = measure_tree(&mut composition, root, viewport_size);
        let visible = collect_visible_item_texts(
            &renderer.render(&layout),
            Rect {
                x: 0.0,
                y: 0.0,
                width: viewport_size.width,
                height: viewport_size.height,
            },
        );

        assert!(
            !visible.is_empty(),
            "step {step}: expected at least one visible item after delta {delta}"
        );

        for pair in visible.windows(2) {
            assert!(
                pair[1].0 > pair[0].0,
                "step {step}: rendered item order regressed after delta {delta}: {:?}",
                visible
            );
        }

        if let Some(previous_top_item) = last_top_item {
            let current_top_item = visible[0].0;
            if delta < 0.0 {
                assert!(
                    current_top_item >= previous_top_item,
                    "step {step}: forward scroll moved top item backward from {previous_top_item} to {current_top_item}"
                );
            } else if delta > 0.0 {
                assert!(
                    current_top_item <= previous_top_item,
                    "step {step}: reverse scroll backtracked from top item {previous_top_item} to {current_top_item}"
                );
            }
        }

        last_top_item = Some(visible[0].0);
    }

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[test]
fn lazy_column_variable_height_bursty_reverse_scroll_keeps_rendered_items_ordered() {
    let mut composition = run_test_composition(|| {
        let list_state = remember_lazy_list_state();
        LAST_LAZY_STATE.with(|cell| {
            *cell.borrow_mut() = Some(list_state);
        });

        LazyColumn(
            Modifier::empty(),
            list_state,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            |scope| {
                scope.items(
                    240,
                    None::<fn(usize) -> u64>,
                    Some(|index: usize| (index % 4) as u64),
                    |index| {
                        let height = match index % 8 {
                            0 => 36.0,
                            1 => 48.0,
                            2 => 220.0,
                            3 => 64.0,
                            4 => 84.0,
                            5 => 156.0,
                            6 => 52.0,
                            _ => 108.0,
                        };
                        Column(
                            Modifier::empty().fill_max_width().height(height),
                            ColumnSpec::default(),
                            move || {
                                Text(
                                    format!("Item {}", index),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    },
                );
            },
        );
    });

    let root = composition.root().expect("lazy column root");
    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let viewport_size = ViewportSize {
        width: 320.0,
        height: 260.0,
    };
    let renderer = HeadlessRenderer::new();
    let bursts: &[&[f32]] = &[
        &[-120.0, -140.0, -160.0],
        &[-180.0, -180.0],
        &[-96.0, -96.0, -96.0],
        &[88.0, 88.0, 88.0],
        &[72.0, 72.0, 72.0],
        &[-110.0, -110.0, -110.0],
        &[96.0, 96.0, 96.0, 96.0],
        &[64.0, 64.0, 64.0],
    ];
    let mut last_top_item: Option<usize> = None;

    for (step, burst) in bursts.iter().enumerate() {
        let total_delta: f32 = burst.iter().copied().sum();
        for delta in burst.iter().copied() {
            list_state.dispatch_scroll_delta(delta);
        }
        let layout = measure_tree(&mut composition, root, viewport_size);
        let visible = collect_visible_item_texts(
            &renderer.render(&layout),
            Rect {
                x: 0.0,
                y: 0.0,
                width: viewport_size.width,
                height: viewport_size.height,
            },
        );

        assert!(
            !visible.is_empty(),
            "step {step}: expected at least one visible item after burst {:?}",
            burst
        );

        for pair in visible.windows(2) {
            assert!(
                pair[1].0 > pair[0].0,
                "step {step}: rendered item order regressed after burst {:?}: {:?}",
                burst,
                visible
            );
        }

        if let Some(previous_top_item) = last_top_item {
            let current_top_item = visible[0].0;
            if total_delta < 0.0 {
                assert!(
                    current_top_item >= previous_top_item,
                    "step {step}: forward burst moved top item backward from {previous_top_item} to {current_top_item} after {:?}",
                    burst
                );
            } else if total_delta > 0.0 {
                assert!(
                    current_top_item <= previous_top_item,
                    "step {step}: reverse burst backtracked from top item {previous_top_item} to {current_top_item} after {:?}",
                    burst
                );
            }
        }

        last_top_item = Some(visible[0].0);
    }

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}
