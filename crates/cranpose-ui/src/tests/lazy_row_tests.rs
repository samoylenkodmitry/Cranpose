//! Composition tests for `LazyRow`, mirroring the coverage
//! `lazy_list_viewport_tests.rs`/`lazy_list_recompose_tests.rs` give
//! `LazyColumn`: items composed and measured against the viewport, item
//! count driving scroll bounds through recomposition, and — the behavior
//! unique to the horizontal orientation — rendered items actually moving
//! along the x axis (not y) when the list is scrolled.

use std::cell::RefCell;

use cranpose_core::{location_key, Composition, MemoryApplier, MutableState, NodeId};
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope, LazyListState};
use cranpose_ui_graphics::Size as ViewportSize;

use super::*;

thread_local! {
    static LAST_LAZY_ROW_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
    static GROWING_LAZY_ROW_CALL_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[test]
fn lazy_row_unbounded_width_matches_effective_viewport() {
    let mut composition = run_test_composition(|| {
        let list_state = rememberLazyListState();
        LAST_LAZY_ROW_STATE.with(|cell| {
            *cell.borrow_mut() = Some(list_state);
        });

        LazyRow(
            Modifier::empty(),
            list_state,
            LazyRowSpec::default(),
            |scope| {
                scope.items(100, |_| {
                    Spacer(Size {
                        width: 100.0,
                        height: 0.0,
                    });
                });
            },
        );
    });

    let root = composition.root().expect("lazy row root");
    let handle = composition.runtime_handle();
    let measurements = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let result = measure_layout(
            &mut applier,
            root,
            ViewportSize {
                width: f32::INFINITY,
                height: 320.0,
            },
        )
        .expect("layout measurement");
        applier.clear_runtime_handle();
        result
    };

    let list_state = LAST_LAZY_ROW_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let expected_width = list_state.layout_info().viewport_size;
    let actual_width = measurements.root_size().width;

    assert!(actual_width.is_finite());
    assert!(
        (actual_width - expected_width).abs() < 0.01,
        "expected lazy row width to match effective viewport {expected_width}, got {actual_width}"
    );

    LAST_LAZY_ROW_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[composable]
#[allow(non_snake_case)]
fn GrowingLazyRow(item_count: MutableState<usize>) {
    let list_state = rememberLazyListState();
    LAST_LAZY_ROW_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });
    let count = item_count.value();
    GROWING_LAZY_ROW_CALL_COUNT.with(|call_count| {
        call_count.set(call_count.get() + 1);
    });

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            format!("Count {count}"),
            Modifier::empty(),
            TextStyle::default(),
        );
        LazyRow(
            Modifier::empty().height(120.0),
            list_state,
            LazyRowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
            |scope| {
                scope.items(count, |index| {
                    Row(
                        Modifier::empty().height(48.0).width(96.0),
                        RowSpec::default(),
                        move || {
                            Text(
                                format!("Item {}", index),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                });
            },
        );
    });
}

#[test]
fn lazy_row_updates_scroll_bounds_when_item_count_grows_without_scrolling() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let item_count = MutableState::with_runtime(2usize, runtime.clone());

    GROWING_LAZY_ROW_CALL_COUNT.with(|call_count| call_count.set(0));
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            GrowingLazyRow(item_count);
        })
        .expect("initial render");

    let root = composition.root().expect("lazy row root");
    let viewport = Size {
        width: 260.0,
        height: 320.0,
    };
    measure_root(&mut composition, root, viewport);

    let list_state = LAST_LAZY_ROW_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    assert_eq!(list_state.layout_info().total_items_count, 2);
    assert!(
        !list_state.can_scroll_forward(),
        "initial short row should not scroll"
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
    GROWING_LAZY_ROW_CALL_COUNT.with(|call_count| {
        assert!(
            call_count.get() >= 2,
            "expected LazyRow parent composable to execute again after item count growth"
        );
    });
    measure_root(&mut composition, root, viewport);

    assert_eq!(list_state.layout_info().total_items_count, 24);
    assert!(
        list_state.can_scroll_forward(),
        "lazy row should become scrollable when item count grows beyond viewport"
    );
    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Count 24"),
        "expected parent composition to observe the grown item count"
    );

    LAST_LAZY_ROW_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[composable]
#[allow(non_snake_case)]
fn HorizontalScrollIndicatorLazyRow() {
    let list_state = rememberLazyListState();
    LAST_LAZY_ROW_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            format!("First visible {}", list_state.first_visible_item_index()),
            Modifier::empty(),
            TextStyle::default(),
        );
        Text(
            format!("Can scroll back {}", list_state.can_scroll_backward()),
            Modifier::empty(),
            TextStyle::default(),
        );
        LazyRow(
            Modifier::empty().fill_max_height().width(240.0),
            list_state,
            LazyRowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
            |scope| {
                scope.items(80, |index| {
                    Text(
                        format!("Item {}", index),
                        Modifier::empty().width(48.0),
                        TextStyle::default(),
                    );
                });
            },
        );
    });
}

#[test]
fn lazy_row_gesture_scroll_moves_rendered_items_along_the_horizontal_axis() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    LAST_LAZY_ROW_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });

    composition
        .render(location_key(file!(), line!(), column!()), || {
            HorizontalScrollIndicatorLazyRow();
        })
        .expect("initial render");

    let root = composition.root().expect("lazy row root");
    let viewport = Size {
        width: 320.0,
        height: 320.0,
    };
    measure_root(&mut composition, root, viewport);
    let initial_texts = render_texts(&mut composition, root);
    assert!(
        initial_texts.iter().any(|text| text == "First visible 0"),
        "test must subscribe a composition scope to the scroll position"
    );
    assert!(
        initial_texts
            .iter()
            .any(|text| text == "Can scroll back false"),
        "test must subscribe a composition scope to lazy scroll bounds"
    );
    let initial_records = render_text_records(&mut composition, root);
    let initial_item_0_x = text_x(&initial_records, "Item 0");
    let initial_item_0_y = text_y(&initial_records, "Item 0");

    // A delta smaller than one item's span (48px width + 8px spacing) keeps
    // item 0 within the retained/composed window, so the assertion below
    // exercises axis-correctness rather than virtualization edge cases.
    let list_state = LAST_LAZY_ROW_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    list_state.dispatch_scroll_delta(-40.0);
    measure_root(&mut composition, root, viewport);

    assert!(
        list_state.can_scroll_backward_non_reactive(),
        "gesture scroll should update retained lazy scroll bounds"
    );
    assert!(
        composition
            .process_invalid_scopes()
            .expect("gesture scroll invalidation processing"),
        "gesture scroll must recompose scopes that observe public scroll position"
    );
    measure_root(&mut composition, root, viewport);
    let scrolled_texts = render_texts(&mut composition, root);
    assert!(
        scrolled_texts
            .iter()
            .any(|text| text == "Can scroll back true"),
        "scroll observer text did not track gesture-updated backward capability; got {scrolled_texts:?}"
    );

    let scrolled_records = render_text_records(&mut composition, root);
    let scrolled_item_0_x = text_x(&scrolled_records, "Item 0");
    let scrolled_item_0_y = text_y(&scrolled_records, "Item 0");
    assert!(
        (initial_item_0_x - scrolled_item_0_x - 40.0).abs() < 0.5,
        "scrolling a lazy row should move its rendered items along x by the scroll delta: \
         initial_x={initial_item_0_x}, scrolled_x={scrolled_item_0_x}"
    );
    assert!(
        (initial_item_0_y - scrolled_item_0_y).abs() < 0.01,
        "scrolling a lazy row must not move its rendered items along y: \
         initial_y={initial_item_0_y}, scrolled_y={scrolled_item_0_y}"
    );

    LAST_LAZY_ROW_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[derive(Debug)]
struct RenderedText {
    value: String,
    x: f32,
    y: f32,
}

fn render_texts(composition: &mut Composition<MemoryApplier>, root: NodeId) -> Vec<String> {
    render_text_records(composition, root)
        .into_iter()
        .map(|record| record.value)
        .collect()
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
                width: 320.0,
                height: 320.0,
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
                x: rect.x,
                y: rect.y,
            }),
            _ => None,
        })
        .collect()
}

fn text_x(records: &[RenderedText], value: &str) -> f32 {
    records
        .iter()
        .find(|record| record.value == value)
        .unwrap_or_else(|| panic!("expected rendered text {value:?}, got {records:?}"))
        .x
}

fn text_y(records: &[RenderedText], value: &str) -> f32 {
    records
        .iter()
        .find(|record| record.value == value)
        .unwrap_or_else(|| panic!("expected rendered text {value:?}, got {records:?}"))
        .y
}

fn measure_root(composition: &mut Composition<MemoryApplier>, root: NodeId, size: Size) {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let _ = applier.compute_layout(root, size).expect("layout");
    applier.clear_runtime_handle();
}
