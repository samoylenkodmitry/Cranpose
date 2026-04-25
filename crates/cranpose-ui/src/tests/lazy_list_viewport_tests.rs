use super::*;
use cranpose_core::NodeId;
use cranpose_foundation::lazy::{
    remember_lazy_list_state, remember_lazy_list_state_with_position, LazyListScope,
};
use cranpose_ui_graphics::Rect;
use cranpose_ui_graphics::Size as ViewportSize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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

fn collect_visible_item_state_texts(
    scene: &crate::renderer::RecordedRenderScene,
    viewport: Rect,
) -> Vec<(usize, i32, f32)> {
    let mut items = Vec::new();
    for operation in scene.operations() {
        let crate::renderer::RenderOp::Text { rect, value, .. } = operation else {
            continue;
        };
        let Some(rest) = value.strip_prefix("Item ") else {
            continue;
        };
        let Some((index, state)) = rest.split_once(" state ") else {
            continue;
        };
        let intersects_vertically =
            rect.y < viewport.y + viewport.height && rect.y + rect.height > viewport.y;
        if !intersects_vertically {
            continue;
        }
        let (Some(index), Some(state)) = (index.parse().ok(), state.parse().ok()) else {
            continue;
        };
        items.push((index, state, rect.y));
    }
    items.sort_by(|left, right| left.2.partial_cmp(&right.2).expect("finite y"));
    items
}

fn find_nearest_draw_ancestor_for_text<'a>(
    node: &'a crate::LayoutBox,
    text: &str,
) -> Option<&'a crate::LayoutBox> {
    fn visit<'a>(
        node: &'a crate::LayoutBox,
        text: &str,
        draw_ancestors: &mut Vec<&'a crate::LayoutBox>,
    ) -> Option<&'a crate::LayoutBox> {
        let has_draw_content = !node.node_data.modifier_slices().draw_commands().is_empty();
        if has_draw_content {
            draw_ancestors.push(node);
        }

        let result = if node.node_data.modifier_slices().text_content() == Some(text) {
            draw_ancestors.last().copied()
        } else {
            node.children
                .iter()
                .find_map(|child| visit(child, text, draw_ancestors))
        };

        if has_draw_content {
            draw_ancestors.pop();
        }

        result
    }

    visit(node, text, &mut Vec::new())
}

#[test]
fn lazy_column_jump_does_not_alias_remembered_item_state() {
    let captured_slots = Rc::new(RefCell::new(HashMap::new()));
    let mut composition = run_test_composition({
        let captured_slots = Rc::clone(&captured_slots);
        move || {
            let list_state = remember_lazy_list_state();
            LAST_LAZY_STATE.with(|cell| {
                *cell.borrow_mut() = Some(list_state);
            });

            LazyColumn(
                Modifier::empty(),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                {
                    let captured_slots = Rc::clone(&captured_slots);
                    move |scope| {
                        scope.items(
                            300,
                            Some(|index: usize| index as u64),
                            Some(|index: usize| (index % 3) as u64),
                            {
                                let captured_slots = Rc::clone(&captured_slots);
                                move |index| {
                                    let slot = cranpose_core::with_current_composer(|composer| {
                                        composer.remember(|| (index as i32) * 10)
                                    });
                                    let state_value = slot.with(|value| *value);
                                    captured_slots.borrow_mut().insert(index, slot);
                                    Text(
                                        format!("Item {index} state {state_value}"),
                                        Modifier::empty().height(40.0),
                                        TextStyle::default(),
                                    );
                                }
                            },
                        );
                    }
                },
            );
        }
    });

    let root = composition.root().expect("lazy column root");
    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let viewport_size = ViewportSize {
        width: 320.0,
        height: 180.0,
    };
    let renderer = HeadlessRenderer::new();

    let initial_layout = measure_tree(&mut composition, root, viewport_size);
    let initial_visible = collect_visible_item_state_texts(
        &renderer.render(&initial_layout),
        Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_size.width,
            height: viewport_size.height,
        },
    );
    assert!(
        !initial_visible.is_empty(),
        "initial lazy viewport should render items",
    );
    for (index, _, _) in &initial_visible {
        let slot = captured_slots
            .borrow()
            .get(index)
            .cloned()
            .expect("visible item slot should be captured");
        slot.replace(10_000 + *index as i32);
    }

    list_state.scroll_to_item(120, 0.0);
    let far_layout = measure_tree(&mut composition, root, viewport_size);
    let far_visible = collect_visible_item_state_texts(
        &renderer.render(&far_layout),
        Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_size.width,
            height: viewport_size.height,
        },
    );
    assert!(
        far_visible.iter().any(|(index, _, _)| *index >= 120),
        "jump should render far lazy items: {far_visible:?}",
    );
    for (index, state, _) in &far_visible {
        assert_eq!(
            *state,
            (*index as i32) * 10,
            "lazy jump must not alias remembered state from another item",
        );
    }

    list_state.scroll_to_item(initial_visible[0].0, 0.0);
    let restored_layout = measure_tree(&mut composition, root, viewport_size);
    let restored_visible = collect_visible_item_state_texts(
        &renderer.render(&restored_layout),
        Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_size.width,
            height: viewport_size.height,
        },
    );
    let mut restored_count = 0;
    for (index, _, _) in initial_visible {
        let Some((_, restored_state, _)) = restored_visible
            .iter()
            .find(|(restored_index, _, _)| *restored_index == index)
        else {
            continue;
        };
        restored_count += 1;
        assert_eq!(
            *restored_state,
            10_000 + index as i32,
            "jumping away and back must preserve remembered state for keyed item {index}",
        );
    }
    assert!(
        restored_count > 0,
        "jumping back should restore at least one initially visible keyed item: {restored_visible:?}",
    );

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
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

#[test]
fn lazy_column_tall_text_item_keeps_rendered_height_in_sync_with_lazy_measurement() {
    let tall_body = Rc::new(
        "This comment body is intentionally long so the first lazy item becomes taller than the viewport once it wraps to the available width. "
            .repeat(20),
    );

    let mut composition = run_test_composition({
        let tall_body = Rc::clone(&tall_body);
        move || {
            let list_state = remember_lazy_list_state_with_position(0, 180.0);
            LAST_LAZY_STATE.with(|cell| {
                *cell.borrow_mut() = Some(list_state);
            });

            LazyColumn(
                Modifier::empty(),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
                {
                    let tall_body = Rc::clone(&tall_body);
                    move |scope| {
                        scope.item(Some(0), None, {
                            let tall_body = Rc::clone(&tall_body);
                            move || {
                                Column(
                                    Modifier::empty()
                                        .fill_max_width()
                                        .background(Color(0.18, 0.24, 0.32, 1.0))
                                        .padding(12.0),
                                    ColumnSpec::new()
                                        .vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                                    {
                                        let tall_body = Rc::clone(&tall_body);
                                        move || {
                                            Text(
                                                "Tall item".to_string(),
                                                Modifier::empty(),
                                                TextStyle::default(),
                                            );
                                            Text(
                                                (*tall_body).clone(),
                                                Modifier::empty(),
                                                TextStyle::default(),
                                            );
                                        }
                                    },
                                );
                            }
                        });

                        scope.item(Some(1), None, move || {
                            Column(
                                Modifier::empty()
                                    .fill_max_width()
                                    .background(Color(0.26, 0.32, 0.18, 1.0))
                                    .padding(12.0),
                                ColumnSpec::default(),
                                move || {
                                    Text(
                                        "Next item".to_string(),
                                        Modifier::empty(),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        });
                    }
                },
            );
        }
    });

    let root = composition.root().expect("lazy column root");
    let viewport_size = ViewportSize {
        width: 240.0,
        height: 260.0,
    };
    let layout = measure_tree(&mut composition, root, viewport_size);
    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let layout_info = list_state.layout_info();
    let measured_first = layout_info
        .visible_items_info
        .iter()
        .find(|item| item.index == 0)
        .expect("first item should remain visible after initial scroll offset");

    let first_box = find_nearest_draw_ancestor_for_text(layout.root(), "Tall item")
        .expect("tall item box should be present");
    let second_box = find_nearest_draw_ancestor_for_text(layout.root(), "Next item")
        .expect("second item box should be present");
    let rendered_gap = second_box.rect.y - (first_box.rect.y + first_box.rect.height);

    assert!(
        (first_box.rect.height - measured_first.size).abs() < 1.0,
        "lazy list measured the first item at {:.1}px but rendered it at {:.1}px",
        measured_first.size,
        first_box.rect.height
    );
    assert!(
        (rendered_gap - 12.0).abs() < 1.0,
        "expected only list spacing between first and second items, got gap {:.1}px",
        rendered_gap
    );

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}
