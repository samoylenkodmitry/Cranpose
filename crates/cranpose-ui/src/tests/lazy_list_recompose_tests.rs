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
            RenderOp::Text { value, .. } => Some(value.clone()),
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

#[test]
fn lazy_list_item_recomposes_on_state_change() {
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
