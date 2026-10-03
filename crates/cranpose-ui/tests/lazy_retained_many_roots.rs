use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    AppContext, HeadlessRenderer, LayoutEngine, LazyColumn, LazyColumnSpec, Modifier, RenderOp,
    Size, Text, TextStyle, composable,
};

const VIEWPORT: Size = Size::new(320.0, 360.0);

#[composable]
fn ManyRootLazyList(
    show_updated: MutableState<bool>,
    captured_list_state: Rc<RefCell<Option<LazyListState>>>,
) {
    let list_state = rememberLazyListState();
    *captured_list_state.borrow_mut() = Some(list_state);
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(
                LazyItems::new(12).key(|index| index as u64),
                move |index| match index {
                    0 => {
                        Text(
                            "Before",
                            Modifier::empty().height(24.0),
                            TextStyle::default(),
                        );
                    }
                    1 => {
                        if show_updated.value() {
                            for child in (0..10).rev() {
                                Text(
                                    format!("New {child}"),
                                    Modifier::empty().height(20.0),
                                    TextStyle::default(),
                                );
                            }
                        } else {
                            for child in 0..9 {
                                Text(
                                    format!("Old {child}"),
                                    Modifier::empty().height(24.0),
                                    TextStyle::default(),
                                );
                            }
                        }
                    }
                    2 => {
                        Text(
                            "Following",
                            Modifier::empty().height(24.0),
                            TextStyle::default(),
                        );
                    }
                    _ => {
                        Text(
                            format!("Row {index}"),
                            Modifier::empty().height(24.0),
                            TextStyle::default(),
                        );
                    }
                },
            );
        },
    );
}

#[derive(Debug)]
struct TextRecord {
    value: String,
    y: f32,
}

fn render_texts(
    context: &Rc<AppContext>,
    composition: &mut Composition<MemoryApplier>,
) -> Vec<TextRecord> {
    context.enter(|| {
        let root = composition.root().expect("lazy list root");
        let runtime = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(runtime);
        let layout = applier
            .compute_layout(root, VIEWPORT)
            .expect("lazy list layout");
        applier.clear_runtime_handle();
        HeadlessRenderer::new()
            .render(&layout)
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                RenderOp::Text { rect, value, .. } => Some(TextRecord {
                    value: value.clone(),
                    y: rect.y,
                }),
                _ => None,
            })
            .collect()
    })
}

fn text_y(records: &[TextRecord], value: &str) -> f32 {
    records
        .iter()
        .find(|record| record.value == value)
        .unwrap_or_else(|| panic!("expected text {value:?}, records={records:?}"))
        .y
}

#[test]
fn retained_lazy_item_remeasures_reordered_many_roots_during_scroll() {
    let context = AppContext::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let show_updated =
        context.enter(|| MutableState::with_runtime(false, composition.runtime_handle()));
    let captured_list_state = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&captured_list_state);
    context
        .enter(|| {
            composition.render(location_key(file!(), line!(), column!()), move || {
                ManyRootLazyList(show_updated, Rc::clone(&capture));
            })
        })
        .expect("initial composition");

    let list_state = captured_list_state
        .borrow()
        .as_ref()
        .copied()
        .expect("lazy list state");
    let initial = render_texts(&context, &mut composition);
    let old_roots: Vec<_> = initial
        .iter()
        .filter(|record| record.value.starts_with("Old "))
        .collect();
    assert_eq!(old_roots.len(), 9, "initial row should expose nine roots");
    assert!(
        (text_y(&initial, "Following") - 240.0).abs() < 0.5,
        "initial following row position should include the 216dp item before scrolling, got {initial:?}"
    );

    context.enter(|| {
        show_updated.set(true);
        list_state.dispatch_scroll_delta(-4.0);
    });
    while context
        .enter(|| composition.process_invalid_scopes())
        .expect("state update and scroll invalidation")
    {}

    let updated = render_texts(&context, &mut composition);
    let new_roots: Vec<_> = updated
        .iter()
        .filter(|record| record.value.starts_with("New "))
        .map(|record| record.value.clone())
        .collect();
    let expected: Vec<_> = (0..10).rev().map(|child| format!("New {child}")).collect();
    assert_eq!(
        new_roots, expected,
        "updated roots should use the new order"
    );
    assert!(
        !updated
            .iter()
            .any(|record| record.value.starts_with("Old ")),
        "old roots must disappear after recomposition, got {updated:?}"
    );
    assert!(
        (text_y(&updated, "New 9") - 20.0).abs() < 0.5,
        "scroll should keep the updated item visible, got {updated:?}"
    );
    assert!(
        (text_y(&updated, "New 0") - 200.0).abs() < 0.5,
        "the last updated root should follow the reordered roots, got {updated:?}"
    );
    assert!(
        (text_y(&updated, "Following") - 220.0).abs() < 0.5,
        "following item should move with the item height change from 216dp to 200dp, got {updated:?}"
    );
}
