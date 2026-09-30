use std::alloc::System;

use cranpose_ui::{
    Modifier, measure_layout, run_test_composition,
    widgets::{
        box_widget::{Box, BoxSpec},
        column::{Column, ColumnSpec},
    },
};
use cranpose_ui_graphics::{Color, Size};
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
fn remeasuring_a_row_allocates_only_its_measurement() {
    let few = remeasure_allocations(10);
    let many = remeasure_allocations(110);
    let per_row = (many - few) as f64 / 100.0;
    println!("allocations per remeasured row: {per_row}");
    assert!(per_row <= 1.0, "{per_row} allocations per remeasured row");
}

/// A modifier chain built link by link keeps its elements in one shared
/// allocation it extends in place, so each link costs its element alone.
fn a_modifier_chain_allocates_its_elements_and_one_shared_box() {
    drop(Modifier::empty().padding(1.0).background(Color::RED));
    let region = Region::new(GLOBAL);
    let modifier = Modifier::empty()
        .padding(1.0)
        .background(Color::RED)
        .rounded_corners(3.0);
    let change = region.change();
    drop(modifier);
    assert_eq!(change.allocations, 4, "{change:?}");
}

fn resolving_text_direction_does_not_allocate() {
    use std::hint::black_box;

    use cranpose_ui::text::TextDirection;

    let region = Region::new(GLOBAL);
    for text in [
        "$1,053.980",
        "Latin",
        "שלום",
        "١٢٣ Latin",
        "\u{2067}שלום\u{2069}Latin",
    ] {
        black_box(TextDirection::Content.resolve(black_box(text)));
    }
    let change = region.change();
    assert_eq!(change.allocations, 0, "{change:?}");
    assert_eq!(change.reallocations, 0, "{change:?}");
}

fn owned_bitmaps_allocate_only_metadata_and_clones_share_storage() {
    use cranpose_ui_graphics::ImageBitmap;

    let pixels = vec![255; 128 * 128 * 4];
    let pixel_bytes = pixels.len();
    let region = Region::new(GLOBAL);
    let bitmap = ImageBitmap::from_rgba8(128, 128, pixels).expect("owned bitmap");
    let created = region.change();
    assert_eq!(created.allocations, 1, "{created:?}");
    assert_eq!(created.reallocations, 0, "{created:?}");
    assert!(created.bytes_allocated <= 128, "{created:?}");
    let clone_region = Region::new(GLOBAL);
    let clone = bitmap.clone();
    drop(bitmap);
    let shared = clone_region.change();
    assert_eq!(shared.allocations, 0, "{shared:?}");
    assert_eq!(shared.deallocations, 0, "{shared:?}");
    drop(clone);
    let released = clone_region.change();
    assert_eq!(released.deallocations, 2, "{released:?}");
    assert_eq!(
        released.bytes_deallocated,
        pixel_bytes + created.bytes_allocated,
        "{released:?}"
    );
}

fn owned_bitmaps_do_not_retain_spare_pixel_capacity() {
    use cranpose_ui_graphics::ImageBitmap;

    let mut pixels = Vec::with_capacity(65536);
    pixels.extend_from_slice(&[1, 2, 3, 255]);
    let bitmap = ImageBitmap::from_rgba8(1, 1, pixels).expect("small bitmap");
    let region = Region::new(GLOBAL);
    drop(bitmap);
    let released = region.change();
    assert!(released.bytes_deallocated <= 128, "{released:?}");
}

/// One test, so no other test's allocations land in these regions: the
/// counting allocator counts every thread.
#[test]
fn layout_and_modifiers_stay_within_their_allocation_budgets() {
    remeasuring_a_row_allocates_only_its_measurement();
    a_modifier_chain_allocates_its_elements_and_one_shared_box();
    resolving_text_direction_does_not_allocate();
    owned_bitmaps_allocate_only_metadata_and_clones_share_storage();
    owned_bitmaps_do_not_retain_spare_pixel_capacity();
}
