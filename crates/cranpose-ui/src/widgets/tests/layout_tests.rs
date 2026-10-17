use std::cell::Cell;

use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};

use super::*;

#[test]
fn layout_recomposes_when_content_reads_state() {
    let _app_context = crate::render_state::app_context_test_scope();
    thread_local! {
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0_i32, runtime);
    let key = location_key(file!(), line!(), column!());
    // A layout is inline: a read in its content at the root recomposes the
    // root, which the host's reconcile runs again.
    let mut content = move || {
        Layout(
            Modifier::empty(),
            crate::layout::policies::EmptyMeasurePolicy,
            move || {
                let _ = state.value();
                INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
            },
        );
    };

    composition
        .render(key, &mut content)
        .expect("initial layout render");

    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    state.set_value(1);
    composition
        .reconcile(key, &mut content)
        .expect("layout content recomposition");

    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 2));
}

#[composable]
fn ReadsInRowAndBox(state: MutableState<i32>, runs: Rc<Cell<usize>>, seen: Rc<Cell<i32>>) {
    runs.set(runs.get() + 1);
    crate::widgets::Row(
        Modifier::empty(),
        crate::widgets::RowSpec::default(),
        move || {
            let seen = Rc::clone(&seen);
            crate::widgets::Box(
                Modifier::empty(),
                crate::widgets::BoxSpec::default(),
                move || seen.set(state.value()),
            );
        },
    );
}

#[test]
fn a_state_read_in_layout_content_recomposes_the_composable_that_called_the_layout() {
    // Compose's Box and Row are inline: what their content reads belongs to
    // the composable around them, which composes them again.
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let state = MutableState::with_runtime(0_i32, composition.runtime_handle());
    let runs = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(-1));
    composition
        .render(location_key(file!(), line!(), column!()), {
            let (runs, seen) = (Rc::clone(&runs), Rc::clone(&seen));
            move || ReadsInRowAndBox(state, Rc::clone(&runs), Rc::clone(&seen))
        })
        .expect("initial render");
    assert_eq!((runs.get(), seen.get()), (1, 0));

    state.set_value(7);
    composition
        .process_invalid_scopes()
        .expect("recompose the reader");

    assert_eq!(
        (runs.get(), seen.get()),
        (2, 7),
        "the content saw the new value through its caller's recomposition"
    );
}

#[test]
fn subcompose_missing_policy_cell_measures_empty_layout() {
    let result = empty_subcompose_measure_result(Constraints {
        min_width: 12.0,
        max_width: 120.0,
        min_height: 8.0,
        max_height: 90.0,
    });

    assert_eq!(result.size.width, 12.0);
    assert_eq!(result.size.height, 8.0);
    assert!(result.placements.is_empty());
}

fn groups_composed_by(content: impl FnMut() + 'static) -> usize {
    let _app_context = crate::render_state::app_context_test_scope();
    crate::run_test_composition(content)
        .composition
        .debug_slot_table_stats()
        .group_count
}

#[test]
fn a_widget_that_is_one_layout_composes_as_few_groups_as_a_bare_layout() {
    use crate::{
        layout::policies::EmptyMeasurePolicy,
        widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Spacer},
    };
    let layout = groups_composed_by(|| {
        Layout(Modifier::empty(), EmptyMeasurePolicy, || {});
    });
    let widgets = [
        groups_composed_by(|| {
            Row(Modifier::empty(), RowSpec::default(), || {});
        }),
        groups_composed_by(|| {
            Column(Modifier::empty(), ColumnSpec::default(), || {});
        }),
        groups_composed_by(|| {
            Box(Modifier::empty(), BoxSpec::default(), || {});
        }),
        groups_composed_by(|| {
            Spacer(Modifier::empty().size_points(0.0, 0.0));
        }),
    ];
    assert_eq!(
        widgets, [layout; 4],
        "a widget composing through `Layout` pays for a second group, its read \
         observation and its snapshot on every recomposition"
    );
}

#[composable]
fn ReportsCallerModifierChange(revision: MutableState<u32>, seen: Rc<RefCell<Vec<bool>>>) {
    let padded = Modifier::empty().padding(4.0);
    let modifier = if revision.value() < 2 {
        padded
    } else {
        padded.semantics(|_| {})
    };
    seen.borrow_mut().push(caller_modifier_changed(&modifier));
}

#[test]
fn a_caller_modifier_changes_only_when_it_is_not_strictly_equal() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let revision = MutableState::with_runtime(0_u32, composition.runtime_handle());
    let seen = Rc::new(RefCell::new(Vec::new()));
    composition
        .render(location_key(file!(), line!(), column!()), {
            let seen = Rc::clone(&seen);
            move || ReportsCallerModifierChange(revision, Rc::clone(&seen))
        })
        .expect("initial render");
    for next in 1..=3 {
        revision.set_value(next);
        composition
            .process_invalid_scopes()
            .expect("recompose with the next modifier");
    }
    assert_eq!(
        *seen.borrow(),
        [false, false, true, true],
        "a rebuilt equal modifier is unchanged; a new closure is a change"
    );
}

#[test]
fn rows_and_columns_default_to_compose_alignments() {
    use cranpose_ui_layout::{HorizontalAlignment, LinearArrangement, VerticalAlignment};

    use crate::widgets::{ColumnSpec, RowSpec};

    let row = RowSpec::default();
    assert_eq!(row.horizontal_arrangement, LinearArrangement::Start);
    assert_eq!(row.vertical_alignment, VerticalAlignment::Top);
    let column = ColumnSpec::default();
    assert_eq!(column.vertical_arrangement, LinearArrangement::Start);
    assert_eq!(column.horizontal_alignment, HorizontalAlignment::Start);
}
