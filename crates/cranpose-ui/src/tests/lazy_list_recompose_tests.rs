use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, RepeatMode, StartOffset,
};
use cranpose_core::{location_key, Composition, MemoryApplier, MutableState, NodeId};
use cranpose_foundation::lazy::{rememberLazyListState, LazyItems, LazyListScope, LazyListState};

use super::*;
use crate::{
    text::{AnnotatedString, PreparedTextLayout, TextLayoutOptions, TextMeasurer, TextMetrics},
    text_layout_result::TextLayoutResult,
};

thread_local! {
    static LAST_LAZY_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
    static GROWING_LAZY_LIST_CALL_COUNT: Cell<usize> = const { Cell::new(0) };
    static LAST_INNER_ROW_SCROLL_STATE: RefCell<Option<ScrollState>> = const { RefCell::new(None) };
}

struct CountingPreparedTextMeasurer {
    prepare_calls: Rc<Cell<usize>>,
}

struct TallMultilineTextMeasurer;

impl CountingPreparedTextMeasurer {
    fn new(prepare_calls: Rc<Cell<usize>>) -> Self {
        Self { prepare_calls }
    }
}

impl TextMeasurer for CountingPreparedTextMeasurer {
    fn measure(&self, text: &AnnotatedString, _style: &TextStyle) -> TextMetrics {
        TextMetrics {
            width: text.text.chars().count() as f32 * 8.0,
            height: 18.0,
            line_height: 18.0,
            line_count: 1,
        }
    }

    fn prepare_with_options_for_node(
        &self,
        _node_id: Option<NodeId>,
        text: &AnnotatedString,
        style: &TextStyle,
        _options: TextLayoutOptions,
        _max_width: Option<f32>,
    ) -> PreparedTextLayout {
        if text.text.starts_with("Stable Row ") {
            self.prepare_calls.set(self.prepare_calls.get() + 1);
        }
        PreparedTextLayout {
            text: text.clone(),
            visual_style: style.clone(),
            metrics: self.measure(text, style),
            did_overflow: false,
        }
    }

    fn get_offset_for_position(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        _offset: usize,
    ) -> f32 {
        0.0
    }

    fn layout(&self, text: &AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
        TextLayoutResult::monospaced(&text.text, 8.0, 18.0)
    }
}

impl TextMeasurer for TallMultilineTextMeasurer {
    fn measure(&self, text: &AnnotatedString, _style: &TextStyle) -> TextMetrics {
        let line_count = text.text.lines().count().max(1);
        let width = text
            .text
            .lines()
            .map(|line| line.chars().count() as f32 * 7.0)
            .fold(0.0_f32, f32::max);
        TextMetrics {
            width,
            height: line_count as f32 * 20.0,
            line_height: 20.0,
            line_count,
        }
    }

    fn prepare_with_options_for_node(
        &self,
        _node_id: Option<NodeId>,
        text: &AnnotatedString,
        style: &TextStyle,
        _options: TextLayoutOptions,
        _max_width: Option<f32>,
    ) -> PreparedTextLayout {
        PreparedTextLayout {
            text: text.clone(),
            visual_style: style.clone(),
            metrics: self.measure(text, style),
            did_overflow: false,
        }
    }

    fn get_offset_for_position(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        _offset: usize,
    ) -> f32 {
        0.0
    }

    fn layout(&self, text: &AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
        TextLayoutResult::monospaced(&text.text, 7.0, 20.0)
    }
}

#[composable]
#[allow(non_snake_case)]
fn ScrollIndicatorLazyList() {
    let list_state = rememberLazyListState();
    LAST_LAZY_STATE.with(|cell| {
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
        LazyColumn(
            Modifier::empty().fill_max_width().height(240.0),
            list_state,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            |scope| {
                scope.items(80, |index| {
                    Text(
                        format!("Row {}", index),
                        Modifier::empty().height(48.0),
                        TextStyle::default(),
                    );
                });
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
    let list_state = rememberLazyListState();
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
                scope.items(80, |index| {
                    Text(
                        format!("Row {}", index),
                        Modifier::empty().height(48.0),
                        TextStyle::default(),
                    );
                });
            },
        );
    });
}

#[composable]
#[allow(non_snake_case)]
fn ReactiveSiblingLazyList(item_invocations: Rc<Cell<usize>>) {
    let list_state = rememberLazyListState();
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
            {
                let item_invocations = Rc::clone(&item_invocations);
                move |scope| {
                    scope.items(LazyItems::new(120).key(|index: usize| index as u64), {
                        let item_invocations = Rc::clone(&item_invocations);
                        move |index| {
                            item_invocations.set(item_invocations.get() + 1);
                            Text(
                                format!("Stable Row {}", index),
                                Modifier::empty().height(48.0),
                                TextStyle::default(),
                            );
                        }
                    });
                }
            },
        );
    });
}

#[composable]
#[allow(non_snake_case)]
fn StableKeyedCountingLazyList(item_invocations: Rc<Cell<usize>>) {
    let list_state = rememberLazyListState();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    LazyColumn(
        Modifier::empty().fill_max_width().height(240.0),
        list_state,
        LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        {
            let item_invocations = Rc::clone(&item_invocations);
            move |scope| {
                scope.items(LazyItems::new(120).key(|index: usize| index as u64), {
                    let item_invocations = Rc::clone(&item_invocations);
                    move |index| {
                        item_invocations.set(item_invocations.get() + 1);
                        Text(
                            format!("Cached Row {}", index),
                            Modifier::empty().height(48.0),
                            TextStyle::default(),
                        );
                    }
                });
            }
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn StatefulCachedLazyList(item_invocations: Rc<Cell<usize>>, label_state: MutableState<usize>) {
    let list_state = rememberLazyListState();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    LazyColumn(
        Modifier::empty().fill_max_width().height(120.0),
        list_state,
        LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        {
            let item_invocations = Rc::clone(&item_invocations);
            move |scope| {
                scope.item_keyed(Some(0), None, {
                    let item_invocations = Rc::clone(&item_invocations);
                    move || {
                        item_invocations.set(item_invocations.get() + 1);
                        Text(
                            format!("Cached Label {}", label_state.value()),
                            Modifier::empty().height(48.0),
                            TextStyle::default(),
                        );
                    }
                });
            }
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn VariableHeightCachedLazyList(short_body_state: MutableState<bool>) {
    let list_state = rememberLazyListState();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    LazyColumn(
        Modifier::empty().fill_max_width().height(320.0),
        list_state,
        LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move |scope| {
            scope.item_keyed(Some(0), None, move || {
                let body = if short_body_state.value() {
                    "Short retained comment".to_string()
                } else {
                    "This retained comment starts long enough to wrap across several lines. "
                        .repeat(12)
                };
                Column(
                    Modifier::empty()
                        .fill_max_width()
                        .background(Color(0.18, 0.24, 0.32, 1.0))
                        .padding(8.0),
                    ColumnSpec::default(),
                    move || {
                        Text(body.clone(), Modifier::empty(), TextStyle::default());
                    },
                );
            });

            scope.item_keyed(Some(1), None, || {
                Text(
                    "Following retained comment".to_string(),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            });
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn AnimatedLazyItemList() {
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_width().height(120.0),
        list_state,
        LazyColumnSpec::default(),
        |scope| {
            scope.item_keyed(Some(0), None, || {
                let transition = rememberInfiniteTransition("lazy_item_animation_regression");
                let pulse = transition.animateFloat(
                    0.0,
                    1.0,
                    infiniteRepeatable(
                        AnimationSpec::linear(400),
                        RepeatMode::Reverse,
                        StartOffset::default(),
                    ),
                    "lazy_item_animation_regression",
                );
                let display = (pulse.value() * 100.0).round() as i32;
                Text(
                    format!("Lazy Pulse: {display}"),
                    Modifier::empty().height(24.0),
                    TextStyle::default(),
                );
            });
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn TallCachedLazyTextList(body: Rc<String>) {
    let list_state = rememberLazyListState();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    LazyColumn(
        Modifier::empty().fill_max_width().height(640.0),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            let body = Rc::clone(&body);
            scope.item_keyed(Some(0), None, move || {
                Text((*body).clone(), Modifier::empty(), TextStyle::default());
            });
        },
    );
}

/// A `LazyColumn` whose single item hosts a horizontally scrollable `Row`.
///
/// This mirrors the on-device shape that froze: the inner scroll container is a
/// direct root child of the lazy item's slot, so a scroll-driven *layout-only*
/// repass has to survive the item's measurement cache.
#[composable]
#[allow(non_snake_case)]
fn LazyItemWithScrollableRow() {
    let list_state = rememberLazyListState();
    let row_scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
    LAST_INNER_ROW_SCROLL_STATE.with(|cell| {
        *cell.borrow_mut() = Some(row_scroll);
    });

    LazyColumn(
        Modifier::empty().fill_max_width().height(240.0),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            let row_scroll = row_scroll;
            scope.item_keyed(Some(0), None, move || {
                Row(
                    Modifier::empty()
                        .fill_max_width()
                        .height(60.0)
                        .horizontal_scroll(row_scroll, false),
                    RowSpec::default(),
                    || {
                        Text(
                            "Chip A".to_string(),
                            Modifier::empty().width(300.0).height(40.0),
                            TextStyle::default(),
                        );
                        Text(
                            "Chip B".to_string(),
                            Modifier::empty().width(300.0).height(40.0),
                            TextStyle::default(),
                        );
                        Text(
                            "Chip C".to_string(),
                            Modifier::empty().width(300.0).height(40.0),
                            TextStyle::default(),
                        );
                    },
                );
            });
        },
    );
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
    x: f32,
    y: f32,
}

#[derive(Debug)]
struct RenderedTextRect {
    value: String,
    height: f32,
}

fn render_text_records(
    composition: &mut Composition<MemoryApplier>,
    root: NodeId,
) -> Vec<RenderedText> {
    render_text_records_with_size(
        composition,
        root,
        Size {
            width: 320.0,
            height: 360.0,
        },
    )
}

fn render_text_records_with_size(
    composition: &mut Composition<MemoryApplier>,
    root: NodeId,
    size: Size,
) -> Vec<RenderedText> {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier.compute_layout(root, size).expect("layout");
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

fn render_text_rect_records_with_size(
    composition: &mut Composition<MemoryApplier>,
    root: NodeId,
    size: Size,
) -> Vec<RenderedTextRect> {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier.compute_layout(root, size).expect("layout");
    applier.clear_runtime_handle();
    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Text { rect, value, .. } => Some(RenderedTextRect {
                value: value.clone(),
                height: rect.height,
            }),
            _ => None,
        })
        .collect()
}

fn text_y(records: &[RenderedText], value: &str) -> f32 {
    records
        .iter()
        .find(|record| record.value == value)
        .unwrap_or_else(|| panic!("expected rendered text {value:?}, got {records:?}"))
        .y
}

fn text_x(records: &[RenderedText], value: &str) -> f32 {
    records
        .iter()
        .find(|record| record.value == value)
        .unwrap_or_else(|| panic!("expected rendered text {value:?}, got {records:?}"))
        .x
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
    let list_state = rememberLazyListState();
    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(list_state);
    });

    LazyColumn(
        Modifier::empty().fill_max_width().height(210.0),
        list_state,
        LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
        |scope| {
            scope.items(
                LazyItems::new(80).key(|index: usize| index as u64),
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
            let list_state = rememberLazyListState();
            LazyColumn(
                Modifier::empty(),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.item_keyed(Some(0), None, {
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
    let list_state = rememberLazyListState();
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
            scope.item_keyed(Some(0), None, move || {
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
fn scroll_state_recomposition_does_not_reprepare_stable_lazy_rows() {
    let _app_context = crate::render_state::app_context_test_scope();
    let prepare_calls = Rc::new(Cell::new(0));
    crate::text::set_text_measurer(CountingPreparedTextMeasurer::new(Rc::clone(&prepare_calls)));

    let item_invocations = Rc::new(Cell::new(0));
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, {
            let item_invocations = Rc::clone(&item_invocations);
            move || ReactiveSiblingLazyList(Rc::clone(&item_invocations))
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let _ = render_texts(&mut composition, root);
    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));

    list_state.dispatch_scroll_delta(-160.0);
    let _ = render_texts(&mut composition, root);

    prepare_calls.set(0);
    item_invocations.set(0);
    composition
        .process_invalid_scopes()
        .expect("process scroll-state invalidation");
    let _ = render_texts(&mut composition, root);

    assert_eq!(
        prepare_calls.get(),
        0,
        "scroll-indicator recomposition should reuse prepared text for stable visible lazy rows"
    );
    assert_eq!(
        item_invocations.get(),
        0,
        "scroll-indicator recomposition should not recompose stable visible lazy row content"
    );
}

#[test]
fn gesture_scroll_recomposes_scroll_position_observers() {
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

    let root = composition.root().expect("lazy list root");
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

    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    list_state.dispatch_scroll_delta(-320.0);
    measure_root(&mut composition, root, viewport);

    assert!(
        list_state.first_visible_item_index_non_reactive() > 0,
        "gesture scroll should move the retained lazy layout position"
    );
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
    let expected = format!(
        "First visible {}",
        list_state.first_visible_item_index_non_reactive()
    );
    assert!(
        scrolled_texts.iter().any(|text| text == &expected),
        "scroll observer text did not track gesture-updated position; expected {expected}, got {scrolled_texts:?}"
    );
    assert!(
        scrolled_texts.iter().any(|text| text == "Can scroll back true"),
        "scroll observer text did not track gesture-updated backward capability; got {scrolled_texts:?}"
    );

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[test]
fn repeated_measure_of_stable_keyed_lazy_list_reuses_retained_item_content() {
    let _app_context = crate::render_state::app_context_test_scope();
    let item_invocations = Rc::new(Cell::new(0));
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, {
            let item_invocations = Rc::clone(&item_invocations);
            move || StableKeyedCountingLazyList(Rc::clone(&item_invocations))
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let viewport = Size {
        width: 360.0,
        height: 260.0,
    };
    measure_root(&mut composition, root, viewport);
    assert!(
        item_invocations.get() > 0,
        "initial lazy measure must compose visible rows"
    );

    item_invocations.set(0);
    measure_root(&mut composition, root, viewport);

    assert_eq!(
        item_invocations.get(),
        0,
        "stable exact lazy items should activate retained nodes without re-running item content"
    );
}

#[test]
fn large_forward_lazy_scroll_reuses_skipped_active_slots_in_same_measure_pass() {
    let _app_context = crate::render_state::app_context_test_scope();
    let item_invocations = Rc::new(Cell::new(0));
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, {
            let item_invocations = Rc::clone(&item_invocations);
            move || StableKeyedCountingLazyList(Rc::clone(&item_invocations))
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let viewport = Size {
        width: 360.0,
        height: 260.0,
    };
    measure_root(&mut composition, root, viewport);
    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let reuse_count_before = list_state.stats().reuse_count;

    list_state.dispatch_scroll_delta(-800.0);
    measure_root(&mut composition, root, viewport);

    assert!(
        list_state.stats().reuse_count > reuse_count_before,
        "large forward scroll should recycle skipped active rows before measuring new trailing rows"
    );
}

#[test]
fn cached_lazy_item_keeps_unbounded_child_measurement_when_placed() {
    let _app_context = crate::render_state::app_context_test_scope();
    crate::text::set_text_measurer(TallMultilineTextMeasurer);

    let body = Rc::new(
        (0..48)
            .map(|index| format!("// retained tall code line {index:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), {
            let body = Rc::clone(&body);
            move || TallCachedLazyTextList(Rc::clone(&body))
        })
        .expect("initial render");
    let root = composition.root().expect("lazy list root");

    let viewport = Size {
        width: 1000.0,
        height: 700.0,
    };
    let initial_records = render_text_rect_records_with_size(&mut composition, root, viewport);
    let initial = initial_records
        .iter()
        .find(|record| record.value == *body)
        .unwrap_or_else(|| panic!("expected tall text render op, got {initial_records:?}"));
    assert!(
        initial.height > 900.0,
        "initial lazy item measurement should use unbounded main-axis constraints, got {initial_records:?}"
    );

    let cached_records = render_text_rect_records_with_size(&mut composition, root, viewport);
    let cached = cached_records
        .iter()
        .find(|record| record.value == *body)
        .unwrap_or_else(|| panic!("expected retained tall text render op, got {cached_records:?}"));
    assert!(
        cached.height > 900.0,
        "cached lazy item placement must retain the unbounded child measurement instead of viewport-clamping it, got {cached_records:?}"
    );
}

#[test]
fn scrolling_a_row_inside_a_cached_lazy_item_moves_its_rendered_children() {
    // Regression: a scrollable Row nested in a LazyColumn item updated its
    // ScrollState but never moved on screen. `dispatch_raw_delta` bubbles
    // *layout* dirtiness only, and the lazy item's cache-reuse gate consulted
    // `needs_measure` alone, so the cached measurement (with the stale offset)
    // was replayed forever. Assert rendered geometry, not state: asserting
    // `value_non_reactive()` is exactly what let this ship green.
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(
            location_key(file!(), line!(), column!()),
            LazyItemWithScrollableRow,
        )
        .expect("initial render");
    let root = composition.root().expect("lazy list root");

    let viewport = Size {
        width: 320.0,
        height: 240.0,
    };
    let initial_records = render_text_records_with_size(&mut composition, root, viewport);
    let initial_x = text_x(&initial_records, "Chip B");

    let row_scroll = LAST_INNER_ROW_SCROLL_STATE
        .with(|cell| *cell.borrow())
        .expect("inner row scroll state captured");
    assert!(
        row_scroll.max_value() > 100.0,
        "inner row must actually overflow its viewport, max_value={}",
        row_scroll.max_value()
    );

    let applied = row_scroll.dispatch_raw_delta(120.0);
    assert!(
        (applied - 120.0).abs() < 0.001,
        "scroll state should accept the full delta, applied={applied}"
    );

    let scrolled_records = render_text_records_with_size(&mut composition, root, viewport);
    let scrolled_x = text_x(&scrolled_records, "Chip B");

    assert!(
        (initial_x - scrolled_x - 120.0).abs() < 0.5,
        "scrolling the Row inside a lazy item must move its rendered children left by the \
         scroll delta: initial x={initial_x:.1}, after 120px scroll x={scrolled_x:.1}, \
         records={scrolled_records:?}"
    );
}

#[test]
fn invalidated_cached_lazy_item_recomposes_instead_of_reusing_stale_content() {
    let _app_context = crate::render_state::app_context_test_scope();
    let item_invocations = Rc::new(Cell::new(0));
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let label_state = MutableState::with_runtime(0usize, runtime);
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, {
            let item_invocations = Rc::clone(&item_invocations);
            move || StatefulCachedLazyList(Rc::clone(&item_invocations), label_state)
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let _ = render_texts(&mut composition, root);

    item_invocations.set(0);
    label_state.set(1);
    let texts = render_texts(&mut composition, root);

    assert!(
        item_invocations.get() > 0,
        "invalidated lazy item scope must re-run item content"
    );
    assert!(
        texts.iter().any(|text| text == "Cached Label 1"),
        "invalidated lazy item must render fresh state, got {texts:?}"
    );
}

#[test]
fn invalidated_cached_lazy_item_remeasures_when_text_height_shrinks() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let short_body_state = MutableState::with_runtime(false, runtime);
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, move || {
            VariableHeightCachedLazyList(short_body_state);
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let viewport = Size {
        width: 320.0,
        height: 360.0,
    };
    let initial_records = render_text_records_with_size(&mut composition, root, viewport);
    let initial_following_y = text_y(&initial_records, "Following retained comment");
    assert!(
        initial_following_y > 120.0,
        "long retained item should push following item down, got y={initial_following_y:.1}"
    );

    short_body_state.set(true);
    while composition
        .process_invalid_scopes()
        .expect("text height invalidation")
    {}

    let updated_records = render_text_records_with_size(&mut composition, root, viewport);
    let updated_following_y = text_y(&updated_records, "Following retained comment");
    assert!(
        updated_following_y < 80.0,
        "following item should move up after retained text shrinks; initial y={initial_following_y:.1}, updated y={updated_following_y:.1}, records={updated_records:?}"
    );
}
#[test]
fn visible_lazy_item_infinite_transition_advances_after_measure_subcomposition() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    composition
        .render(
            location_key(file!(), line!(), column!()),
            AnimatedLazyItemList,
        )
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let initial = render_texts(&mut composition, root)
        .into_iter()
        .find(|text| text.starts_with("Lazy Pulse:"))
        .expect("initial lazy pulse text");
    assert_eq!(initial, "Lazy Pulse: 0");

    let mut time = 0u64;
    for _ in 0..60 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        runtime.drain_ui();
        while composition
            .process_invalid_scopes()
            .expect("process animation invalidation")
        {}

        let current = render_texts(&mut composition, root)
            .into_iter()
            .find(|text| text.starts_with("Lazy Pulse:"))
            .expect("lazy pulse text after frame");
        if current != initial {
            return;
        }
    }

    panic!("visible lazy item infinite transition stayed frozen at {initial}");
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
    let list_state = rememberLazyListState();
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
                scope.items(count, |index| {
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
                });
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

/// A lazy item that grows a second *root* child when `show_extra` flips.
#[composable]
#[allow(non_snake_case)]
fn GrowingRootChildLazyItemList(show_extra: MutableState<bool>) {
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_width().height(240.0),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.item_keyed(Some(0), None, move || {
                Text(
                    "Grown base row".to_string(),
                    Modifier::empty().height(30.0),
                    TextStyle::default(),
                );
                if show_extra.value() {
                    Text(
                        "Grown extra root".to_string(),
                        Modifier::empty().height(30.0),
                        TextStyle::default(),
                    );
                }
            });
        },
    );
}

#[test]
fn recomposed_lazy_item_places_a_newly_emitted_root_child() {
    // Regression: `activate_exact_retained_slot_with_known_children` reported
    // `children_match = true` straight from the CACHED id list whenever the slot
    // was still active, without ever asking the applier what the slot's live
    // roots were. A root child added by recomposition was therefore invisible to
    // the lazy item's reuse gate and never got measured or placed.
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show_extra = MutableState::with_runtime(false, runtime);
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            GrowingRootChildLazyItemList(show_extra);
        })
        .expect("initial render");
    let root = composition.root().expect("lazy list root");
    let viewport = Size {
        width: 320.0,
        height: 240.0,
    };
    let initial = render_text_records_with_size(&mut composition, root, viewport);
    assert!(
        !initial.iter().any(|r| r.value == "Grown extra root"),
        "extra root must not render before the state flips, got {initial:?}"
    );

    show_extra.set(true);
    while composition
        .process_invalid_scopes()
        .expect("grow lazy item root children")
    {}

    let grown = render_text_records_with_size(&mut composition, root, viewport);
    assert!(
        grown.iter().any(|r| r.value == "Grown extra root"),
        "a root child newly emitted by a cached lazy item must be measured and placed, \
         got {grown:?}"
    );
    assert!(
        (text_y(&grown, "Grown extra root") - 30.0).abs() < 0.5,
        "the new root child must be placed below its sibling, got {grown:?}"
    );
}
