use super::*;
use cranpose_core::{location_key, Composition, MemoryApplier, MutableState, NodeId};
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope, LazyListState};
use std::cell::{Cell, RefCell};

thread_local! {
    static LAST_LAZY_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
    static GROWING_LAZY_LIST_CALL_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[composable]
#[allow(non_snake_case)]
fn ScrollIndicatorLazyList() {
    let list_state = remember_lazy_list_state();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            format!("First visible {}", list_state.first_visible_item_index()),
            Modifier::empty(),
            TextStyle::default(),
        );
        LazyColumn(
            Modifier::empty().fill_max_width().height(240.0),
            list_state,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            |scope| {
                scope.items(
                    80,
                    None::<fn(usize) -> u64>,
                    None::<fn(usize) -> u64>,
                    |index| {
                        Text(
                            format!("Row {}", index),
                            Modifier::empty().height(48.0),
                            TextStyle::default(),
                        );
                    },
                );
            },
        );
    });
}

#[composable]
#[allow(non_snake_case)]
fn ChildScrollIndicator(list_state: LazyListState) {
    Text(
        format!(
            "Child first visible {}",
            list_state.first_visible_item_index()
        ),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
#[allow(non_snake_case)]
fn ChildScrollIndicatorLazyList() {
    let list_state = remember_lazy_list_state();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        ChildScrollIndicator(list_state);
        LazyColumn(
            Modifier::empty().fill_max_width().height(240.0),
            list_state,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            |scope| {
                scope.items(
                    80,
                    None::<fn(usize) -> u64>,
                    None::<fn(usize) -> u64>,
                    |index| {
                        Text(
                            format!("Row {}", index),
                            Modifier::empty().height(48.0),
                            TextStyle::default(),
                        );
                    },
                );
            },
        );
    });
}

fn render_texts(composition: &mut Composition<MemoryApplier>, root: NodeId) -> Vec<String> {
    render_text_records(composition, root)
        .into_iter()
        .map(|record| record.value)
        .collect()
}

#[derive(Debug)]
struct RenderedText {
    value: String,
    y: f32,
}

fn render_text_records(
    composition: &mut Composition<MemoryApplier>,
    root: NodeId,
) -> Vec<RenderedText> {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 400.0,
                height: 400.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Text { rect, value, .. } => Some(RenderedText {
                value: value.clone(),
                y: rect.y,
            }),
            _ => None,
        })
        .collect()
}

fn measure_root(composition: &mut Composition<MemoryApplier>, root: NodeId, size: Size) {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let _ = applier.compute_layout(root, size).expect("layout");
    applier.clear_runtime_handle();
}

fn row_header_indices(records: &[RenderedText]) -> Vec<usize> {
    records
        .iter()
        .filter_map(|record| {
            record
                .value
                .strip_prefix("Row Header ")
                .and_then(|index| index.parse().ok())
        })
        .collect()
}

fn assert_consecutive_rows(indices: &[usize], context: &str) {
    assert!(
        !indices.is_empty(),
        "expected visible lazy rows after {context}"
    );
    assert!(
        indices.windows(2).all(|window| window[1] == window[0] + 1),
        "lazy rows should stay in visual order after {context}: {indices:?}"
    );
}

#[composable]
#[allow(non_snake_case)]
fn SelectableScrolledLazyList(selected_index: MutableState<usize>) {
    let list_state = remember_lazy_list_state();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    LazyColumn(
        Modifier::empty().fill_max_width().height(210.0),
        list_state,
        LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
        |scope| {
            scope.items(
                80,
                Some(|index: usize| index as u64),
                None::<fn(usize) -> u64>,
                move |index| {
                    let is_selected = selected_index.value() == index;
                    Column(
                        Modifier::empty().fill_max_width().height(52.0).padding(4.0),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(2.0)),
                        move || {
                            Text(
                                format!("Row Header {index}"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                            let field_state = if is_selected { "Selected" } else { "Idle" };
                            Text(
                                format!("{field_state} Field {index}"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                },
            );
        },
    );
}

#[test]
fn lazy_list_item_recomposes_on_state_change() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let label_state = MutableState::with_runtime(0u32, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            let list_state = remember_lazy_list_state();
            LazyColumn(
                Modifier::empty(),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.item(Some(0), None, {
                        move || {
                            Text(
                                format!("Animated {}", label_state.value()),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        }
                    });
                },
            );
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Animated 0"),
        "expected initial text to be rendered in lazy list item"
    );

    label_state.set(1);
    composition
        .process_invalid_scopes()
        .expect("recompose lazy list item");

    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Animated 1"),
        "expected updated text to recompose inside lazy list item"
    );
}

#[composable]
#[allow(non_snake_case)]
fn ThemedLazyList(theme_state: MutableState<bool>) {
    let list_state = remember_lazy_list_state();
    let label = if theme_state.value() {
        "Use Light".to_string()
    } else {
        "Use Dark".to_string()
    };
    LazyColumn(
        Modifier::empty(),
        list_state,
        LazyColumnSpec::default(),
        |scope| {
            let label = label.clone();
            scope.item(Some(0), None, move || {
                Text(label.clone(), Modifier::empty(), TextStyle::default());
            });
        },
    );
}

#[test]
fn lazy_list_item_recomposes_when_composable_parent_capture_changes() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let theme_state = MutableState::with_runtime(false, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            ThemedLazyList(theme_state);
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Use Dark"),
        "expected initial parent-captured text to be rendered in lazy list item"
    );

    theme_state.set(true);
    composition
        .process_invalid_scopes()
        .expect("recompose lazy list composable parent capture");

    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Use Light"),
        "expected lazy list item to refresh when composable parent-captured state changes"
    );
}

#[test]
fn scrolled_lazy_list_scoped_row_recompose_does_not_ghost_old_rows() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let selected_index = MutableState::with_runtime(usize::MAX, runtime);

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
    composition
        .render(location_key(file!(), line!(), column!()), || {
            SelectableScrolledLazyList(selected_index);
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let viewport = Size {
        width: 360.0,
        height: 260.0,
    };
    measure_root(&mut composition, root, viewport);

    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    list_state.scroll_to_item(24, 0.0);
    measure_root(&mut composition, root, viewport);

    let before_records = render_text_records(&mut composition, root);
    let visible_before = row_header_indices(&before_records);
    assert_consecutive_rows(&visible_before, "scrolling before row selection");
    assert!(
        visible_before.first().copied().unwrap_or_default() >= 20,
        "test must operate on a scrolled viewport, got rows {visible_before:?}"
    );
    assert!(
        before_records
            .iter()
            .all(|record| !record.value.ends_with(" 0")),
        "offscreen row zero must not render after scrolling: {before_records:?}"
    );

    let selected_visible_row = visible_before
        .get(2)
        .copied()
        .expect("scrolled viewport should expose at least three rows");
    selected_index.set(selected_visible_row);
    while composition
        .process_invalid_scopes()
        .expect("row selection scoped recomposition")
    {}
    measure_root(&mut composition, root, viewport);

    let after_records = render_text_records(&mut composition, root);
    let visible_after = row_header_indices(&after_records);
    assert_eq!(
        visible_after, visible_before,
        "row-local recomposition must not shift the scrolled lazy viewport; before={before_records:?} after={after_records:?}"
    );

    let selected_label = format!("Selected Field {selected_visible_row}");
    let idle_label = format!("Idle Field {selected_visible_row}");
    assert_eq!(
        after_records
            .iter()
            .filter(|record| record.value == selected_label)
            .count(),
        1,
        "selected row should render exactly once after scoped recomposition: {after_records:?}"
    );
    assert!(
        after_records
            .iter()
            .all(|record| record.value != idle_label),
        "selected row must not keep a stale idle field after recomposition: {after_records:?}"
    );

    for hidden_index in 0..20 {
        let hidden_header = format!("Row Header {hidden_index}");
        let hidden_idle = format!("Idle Field {hidden_index}");
        assert!(
            after_records
                .iter()
                .all(|record| record.value != hidden_header && record.value != hidden_idle),
            "offscreen row {hidden_index} must not ghost into the scrolled viewport: {after_records:?}"
        );
    }

    let mut previous_y = None;
    for record in after_records
        .iter()
        .filter(|record| record.value.starts_with("Row Header "))
    {
        if let Some(previous_y) = previous_y {
            assert!(
                record.y > previous_y,
                "row headers should keep increasing y positions after scoped recomposition: {after_records:?}"
            );
        }
        previous_y = Some(record.y);
    }

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[composable]
#[allow(non_snake_case)]
fn GrowingLazyList(item_count: MutableState<usize>) {
    let list_state = remember_lazy_list_state();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });
    let count = item_count.value();
    GROWING_LAZY_LIST_CALL_COUNT.with(|call_count| {
        call_count.set(call_count.get() + 1);
    });

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            format!("Count {count}"),
            Modifier::empty(),
            TextStyle::default(),
        );
        LazyColumn(
            Modifier::empty(),
            list_state,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            |scope| {
                scope.items(
                    count,
                    None::<fn(usize) -> u64>,
                    None::<fn(usize) -> u64>,
                    |index| {
                        Column(
                            Modifier::empty().fill_max_width().height(96.0),
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
}

#[test]
fn lazy_list_updates_scroll_bounds_when_item_count_grows_without_scrolling() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let item_count = MutableState::with_runtime(2usize, runtime.clone());

    GROWING_LAZY_LIST_CALL_COUNT.with(|call_count| call_count.set(0));
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            GrowingLazyList(item_count);
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let viewport = Size {
        width: 320.0,
        height: 260.0,
    };
    measure_root(&mut composition, root, viewport);

    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    assert_eq!(list_state.layout_info().total_items_count, 2);
    assert!(
        !list_state.can_scroll_forward(),
        "initial short list should not scroll"
    );

    item_count.set(24);
    let mut recomposed = false;
    while composition
        .process_invalid_scopes()
        .expect("recompose after item count growth")
    {
        recomposed = true;
    }
    assert!(
        recomposed,
        "expected composition to re-run after item count growth"
    );
    GROWING_LAZY_LIST_CALL_COUNT.with(|call_count| {
        assert!(
            call_count.get() >= 2,
            "expected LazyColumn parent composable to execute again after item count growth"
        );
    });
    measure_root(&mut composition, root, viewport);

    assert_eq!(list_state.layout_info().total_items_count, 24);
    assert!(
        list_state.can_scroll_forward(),
        "lazy list should become scrollable when item count grows beyond viewport"
    );
    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Count 24"),
        "expected parent composition to observe the grown item count"
    );

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[test]
fn scroll_to_item_invalidates_indicator_scope_and_updates_visible_index_text() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });

    composition
        .render(location_key(file!(), line!(), column!()), || {
            ScrollIndicatorLazyList();
        })
        .expect("initial render");

    let root = composition.root().expect("root node");
    let viewport = Size {
        width: 320.0,
        height: 320.0,
    };
    measure_root(&mut composition, root, viewport);

    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "First visible 0"),
        "expected initial indicator text before scrolling"
    );

    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    list_state.scroll_to_item(20, 0.0);

    assert!(
        composition.should_render(),
        "scroll_to_item must invalidate composition when a scope reads first_visible_item_index()"
    );

    let mut recomposed = false;
    while composition
        .process_invalid_scopes()
        .expect("scroll indicator recomposition")
    {
        recomposed = true;
        measure_root(&mut composition, root, viewport);
    }
    measure_root(&mut composition, root, viewport);

    assert!(
        recomposed,
        "expected scroll_to_item to trigger recomposition"
    );

    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "First visible 20"),
        "expected indicator text to track scroll_to_item target; texts={texts:?}"
    );

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[test]
fn scroll_to_item_updates_child_indicator_scope() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    LAST_LAZY_STATE.with(|cell| cell.borrow_mut().take());

    composition
        .render(key, || {
            ChildScrollIndicatorLazyList();
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let initial_texts = render_texts(&mut composition, root);
    assert!(
        initial_texts
            .iter()
            .any(|text| text == "Child first visible 0"),
        "expected initial child indicator text, got {initial_texts:?}"
    );

    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    list_state.scroll_to_item(20, 0.0);

    assert!(
        composition.should_render(),
        "scroll_to_item must invalidate composition when a child composable scope reads first_visible_item_index()"
    );

    let mut recomposed = false;
    while composition
        .process_invalid_scopes()
        .expect("recompose child indicator")
    {
        recomposed = true;
    }
    assert!(
        recomposed,
        "expected scroll_to_item to trigger recomposition"
    );

    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Child first visible 20"),
        "expected child indicator text to track scroll_to_item target; texts={texts:?}"
    );
}
