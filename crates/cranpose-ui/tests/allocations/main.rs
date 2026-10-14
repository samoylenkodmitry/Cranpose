use std::alloc::System;

use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_ui::{
    MeasureLayoutOptions, Modifier, measure_layout, measure_layout_with_options,
    run_test_composition,
    widgets::{
        box_widget::{Box, BoxSpec},
        column::{Column, ColumnSpec},
    },
};
use cranpose_ui_graphics::{Color, Size};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[path = "../support/thread_exit.rs"]
mod thread_exit;

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

/// A layout pass as an app shell runs it, with no layout or semantics tree,
/// lends its applier to the host the pass before gave back: remeasuring a
/// column of three rows allocates the builder and each node's measurement.
fn a_layout_pass_lends_its_applier_without_rebuilding_it() {
    let mut composition = run_test_composition(|| {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            || {
                for _ in 0..3 {
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
    let options = MeasureLayoutOptions {
        collect_semantics: false,
        build_layout_tree: false,
    };
    let mut measure = |width: f32| {
        measure_layout_with_options(&mut applier, root, Size::new(width, 4000.0), options)
            .expect("the column measures");
    };
    measure(100.0);
    measure(101.0);
    let region = Region::new(GLOBAL);
    measure(102.0);
    let allocations = region.change().allocations;
    applier.clear_runtime_handle();
    println!("allocations of a shell layout pass over four nodes: {allocations}");
    assert!(allocations <= 6, "{allocations} allocations a pass");
}

/// An animation frame wakes the animation's task, which stores the new value
/// over the old one, and recomposes the scope that reads it. Waiting for the
/// frame and writing the value allocate nothing.
fn an_animation_frame_allocates_only_its_recomposition() {
    let mut composition = run_test_composition(|| {
        let transition = rememberInfiniteTransition("read");
        let progress = transition.animateFloat(
            0.0,
            1.0,
            infiniteRepeatable(
                AnimationSpec::linear(1_000),
                RepeatMode::Reverse,
                StartOffset::default(),
            ),
            "read",
        );
        let _ = progress.value();
    });
    let handle = composition.runtime_handle();
    let mut frame = |index: u64| {
        handle.drain_frame_callbacks(index * 16_666_667);
        handle.drain_ui();
        composition
            .process_invalid_scopes()
            .expect("the frame recomposes")
    };
    for index in 1..5 {
        frame(index);
    }
    let region = Region::new(GLOBAL);
    let recomposed = (5..5 + PASSES as u64).filter(|&index| frame(index)).count();
    let allocations = region.change().allocations / PASSES;
    assert_eq!(recomposed, PASSES, "every frame recomposes the reader");
    println!("allocations of an animation frame: {allocations}");
    assert!(allocations <= 6, "{allocations} allocations a frame");
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

fn live_bytes_after_second_run(run: impl Fn()) -> isize {
    run();
    let region = Region::new(GLOBAL);
    run();
    let change = region.change();
    change.bytes_allocated.cast_signed() - change.bytes_deallocated.cast_signed()
}

/// A thread that exits while an app thread-local still holds a composition
/// frees all its framework state, in either order of thread-local teardown.
fn a_thread_that_held_a_composition_frees_its_memory_at_exit() {
    let bare_thread = live_bytes_after_second_run(|| {
        std::thread::spawn(|| ())
            .join()
            .expect("an empty thread exits");
    });
    for before_hold in [
        thread_exit::touch_the_holder_first,
        thread_exit::touch_the_holder_after_another_runtime,
    ] {
        let held = live_bytes_after_second_run(|| {
            thread_exit::hold_composition_at_thread_exit(before_hold);
        });
        assert_eq!(held, bare_thread, "the exited thread kept framework memory");
    }
}

/// One test, so no other test's allocations land in these regions: the
/// counting allocator counts every thread.
#[test]
fn layout_and_modifiers_stay_within_their_allocation_budgets() {
    remeasuring_a_row_allocates_only_its_measurement();
    a_layout_pass_lends_its_applier_without_rebuilding_it();
    an_animation_frame_allocates_only_its_recomposition();
    if !cfg!(feature = "inspection") {
        a_modifier_chain_allocates_its_elements_and_one_shared_box();
    }
    resolving_text_direction_does_not_allocate();
    owned_bitmaps_allocate_only_metadata_and_clones_share_storage();
    owned_bitmaps_do_not_retain_spare_pixel_capacity();
    a_thread_that_held_a_composition_frees_its_memory_at_exit();
}
