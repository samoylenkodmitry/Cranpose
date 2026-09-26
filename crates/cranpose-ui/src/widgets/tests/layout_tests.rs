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

    composition
        .render(location_key(file!(), line!(), column!()), {
            let observed_state = state;
            move || {
                Layout(
                    Modifier::empty(),
                    crate::layout::policies::EmptyMeasurePolicy,
                    {
                        let observed_state = observed_state;
                        move || {
                            let _ = observed_state.value();
                            INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
                        }
                    },
                );
            }
        })
        .expect("initial layout render");

    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    state.set_value(1);
    composition
        .process_invalid_scopes()
        .expect("layout content recomposition");

    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 2));
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
            Spacer(crate::modifier::Size::default());
        }),
    ];
    assert_eq!(
        widgets, [layout; 4],
        "a widget composing through `Layout` pays for a second group, its read \
         observation and its snapshot on every recomposition"
    );
}
