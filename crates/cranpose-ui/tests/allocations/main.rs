use std::alloc::System;

use cranpose_ui::{
    Modifier, measure_layout, run_test_composition,
    widgets::{
        box_widget::{Box, BoxSpec},
        column::{Column, ColumnSpec},
    },
};
use cranpose_ui_graphics::Size;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const PASSES: usize = 10;

/// Allocations per pass of remeasuring a column of `rows` rows, each pass at
/// a new width, so every row measures again.
fn remeasure_allocations(rows: usize) -> usize {
    let mut composition = run_test_composition(move || {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || {
                for _ in 0..rows {
                    Box(
                        Modifier::empty().fill_max_width().height(10.0),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    });
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let mut measure = |width: f32| {
        measure_layout(&mut applier, root, Size::new(width, 4000.0)).expect("the column measures");
    };
    measure(100.0);
    measure(101.0);
    let region = Region::new(GLOBAL);
    for pass in 0..PASSES {
        measure(102.0 + pass as f32);
    }
    let allocations = region.change().allocations;
    applier.clear_runtime_handle();
    allocations / PASSES
}

/// Measuring a node again allocates its new measurement and nothing that
/// could be kept from the last pass.
#[test]
fn remeasuring_a_row_allocates_only_its_measurement() {
    let few = remeasure_allocations(10);
    let many = remeasure_allocations(110);
    let per_row = (many - few) as f64 / 100.0;
    println!("allocations per remeasured row: {per_row}");
    assert!(per_row <= 1.0, "{per_row} allocations per remeasured row");
}
