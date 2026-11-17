#![allow(clippy::print_stdout, reason = "a test prints what it measured")]

use std::{alloc::System, cell::Cell, rc::Rc};

use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_core::{mutableStateOf, remember};
use cranpose_ui::{
    Constraints, Layout, MeasureLayoutOptions, MeasureResult, MemoryHeadroom, Modifier,
    TestComposition, Text, TextLayoutOptions, TextStyle, measure_layout,
    measure_layout_with_options, prepare_text_layout, run_test_composition, set_memory_headroom,
    text::AnnotatedString,
    widgets::{
        box_widget::{Box, BoxSpec},
        column::{Column, ColumnSpec},
        row::{Row, RowSpec},
    },
};
use cranpose_ui_graphics::{Color, Size};
use cranpose_ui_layout::{Measurable, Measurables, MeasurePolicy, MeasureScope, Placement};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[macro_use]
#[path = "../../../coroflow/tests/support/main_thread_test.rs"]
mod main_thread_test;

#[path = "../support/thread_exit.rs"]
mod thread_exit;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const PASSES: usize = 10;
// The allocations of a pass that lays a long paragraph out at a new width.
const LONG_PARAGRAPH_ALLOCATIONS: usize = 10;

// Rows of 10 points as tall as their parent is wide.
fn rows(count: usize) {
    for _ in 0..count {
        Box(
            Modifier::empty().fill_max_width().height(10.0),
            BoxSpec::default(),
            || {},
        );
    }
}

/// Allocations per pass of remeasuring a column of `rows` rows, each pass at
/// a new width, so every row measures again.
fn remeasure_allocations(count: usize) -> usize {
    pass_allocations(run_test_composition(move || {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || rows(count),
        );
    }))
}

// Allocations per pass of measuring `composition` at a new width.
fn pass_allocations(composition: TestComposition) -> usize {
    pass_allocations_at(composition, |pass| 100.0 + pass as f32)
}

// Allocations per pass of measuring `composition` at `width(pass)`, after
// two passes that warm it up.
fn pass_allocations_at(mut composition: TestComposition, width: impl Fn(usize) -> f32) -> usize {
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let mut measure = |pass: usize| {
        let size = Size::new(width(pass), 4000.0);
        measure_layout(&mut applier, root, size).expect("the column measures");
    };
    measure(0);
    measure(1);
    let region = Region::new(GLOBAL);
    for pass in 2..2 + PASSES {
        measure(pass);
    }
    let allocations = region.change().allocations;
    applier.clear_runtime_handle();
    allocations / PASSES
}

// Allocations of the first measure of a column of `count` rows.
fn first_measure_allocations(count: usize) -> usize {
    let mut composition = run_test_composition(move || {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || rows(count),
        );
    });
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let region = Region::new(GLOBAL);
    measure_layout(&mut applier, root, Size::new(100.0, 4000.0)).expect("the column measures");
    let allocations = region.change().allocations;
    applier.clear_runtime_handle();
    allocations
}

// A parent measures a new child through its entry in the parent's one
// list of children, which takes no allocation of its own.
fn a_first_measure_allocates_at_most_two_per_row() {
    let few = first_measure_allocations(10);
    let many = first_measure_allocations(110);
    let per_row = many.saturating_sub(few) as f64 / 100.0;
    println!("allocations per row at a first measure: {per_row}");
    assert!(
        per_row <= 2.0,
        "{per_row} allocations per row at a first measure"
    );
}

// A column over a row of three boxes.
fn column_over_a_row(below: impl Fn() + 'static) {
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default(),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default(),
                || {
                    for _ in 0..3 {
                        Box(
                            Modifier::empty().weight(1.0).height(10.0),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
            below();
        },
    );
}

// `depth` more such columns nested inside one another.
fn nested_columns(depth: usize) {
    column_over_a_row(move || {
        if depth > 0 {
            nested_columns(depth - 1);
        }
    });
}

// As many columns, side by side under the first.
fn sibling_columns(count: usize) {
    column_over_a_row(move || {
        for _ in 0..count {
            column_over_a_row(|| {});
        }
    });
}

// Rows and columns nested deeper than any fixed count of spare lists
// measure again with the lists of the measures before: a pass over 64
// nested columns allocates what a pass over 64 sibling columns does.
fn remeasuring_deeply_nested_rows_and_columns_allocates_nothing_more() {
    let sibling = pass_allocations(run_test_composition(|| sibling_columns(63)));
    let nested = pass_allocations(run_test_composition(|| nested_columns(63)));
    println!("allocations a pass of 64 columns, siblings: {sibling}, nested: {nested}");
    assert_eq!(nested, sibling, "a deep nest of rows and columns allocates");
}

// A column of `width` rows over `depth` more such columns, nested or side
// by side.
fn wide_columns(depth: usize, width: usize, nested: bool) {
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default(),
        move || {
            rows(width);
            if nested && depth > 0 {
                wide_columns(depth - 1, width, true);
            } else if !nested {
                for _ in 0..depth {
                    wide_columns(0, width, false);
                }
            }
        },
    );
}

// The bytes a fresh thread still holds after it measured `depth` wide
// columns once and dropped their composition.
fn bytes_left_by_wide_columns(depth: usize, nested: bool) -> isize {
    std::thread::spawn(move || {
        let region = Region::new(GLOBAL);
        let mut composition = run_test_composition(move || wide_columns(depth, 300, nested));
        let root = composition.root().expect("a composed root");
        let handle = composition.runtime_handle();
        {
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle);
            measure_layout(&mut applier, root, Size::new(100.0, 4000.0))
                .expect("the columns measure");
            applier.clear_runtime_handle();
        }
        drop(composition);
        let change = region.change();
        change.bytes_allocated.cast_signed() - change.bytes_deallocated.cast_signed()
    })
    .join()
    .expect("the measuring thread")
}

// Rows and columns keep the lists of their last measure whole and at most
// 64 KiB in the others once their composition is gone: wide columns nested
// 16 deep, whose lists for 300 children take about 25 KiB a level, leave at
// most 64 KiB more than as many side by side leave.
fn wide_nested_columns_leave_a_bounded_set_of_lists() {
    let sibling = bytes_left_by_wide_columns(15, false);
    let nested = bytes_left_by_wide_columns(15, true);
    println!("bytes left by 16 wide columns, siblings: {sibling}, nested: {nested}");
    assert!(
        nested - sibling <= 64 * 1024,
        "nested wide columns left {} bytes more",
        nested - sibling
    );
}

// Allocations per pass of a column of `count` rows that share its height by
// weight, so the column's policy measures them again at each new width.
fn weighted_remeasure_allocations(count: usize) -> usize {
    pass_allocations(run_test_composition(move || {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || {
                for _ in 0..count {
                    Box(
                        Modifier::empty().fill_max_width().weight(1.0),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    }))
}

// A column of 300 rows measures again with the lists of its last measure:
// a pass allocates no more than a pass over a column of 10 rows. Each runs
// on a fresh thread, whose rows and columns hold no spare lists of earlier
// measures to grow.
fn remeasuring_a_wide_column_allocates_nothing_more() {
    let on_fresh_thread = |count| {
        std::thread::spawn(move || weighted_remeasure_allocations(count))
            .join()
            .expect("the measuring thread")
    };
    let narrow = on_fresh_thread(10);
    let wide = on_fresh_thread(300);
    println!("allocations a pass of a column of 300 rows: {wide}, of 10 rows: {narrow}");
    assert_eq!(
        wide, narrow,
        "a column of 300 rows allocates as it measures again"
    );
}

/// Measuring a node again fills the memory of its last measurement: a
/// remeasured row allocates nothing.
fn remeasuring_a_row_allocates_nothing() {
    let few = remeasure_allocations(10);
    let many = remeasure_allocations(110);
    let per_row = many.saturating_sub(few) as f64 / 100.0;
    println!("allocations per remeasured row: {per_row}");
    assert!(per_row == 0.0, "{per_row} allocations per remeasured row");
}

// Places its children one under another, last first when `reversed`.
#[derive(Clone, PartialEq)]
struct StackedRows {
    reversed: bool,
}

impl MeasurePolicy for StackedRows {
    fn measure(
        &self,
        _: &dyn MeasureScope,
        measurables: Measurables<'_>,
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::with_capacity(measurables.len());
        let mut place = |index: usize, child: &dyn Measurable| {
            let placeable = child.measure(constraints);
            placements.push(Placement::new(
                placeable.node_id(),
                0.0,
                index as f32 * 10.0,
                0.0,
            ));
        };
        if self.reversed {
            measurables
                .iter()
                .enumerate()
                .rev()
                .for_each(|(index, child)| place(index, child));
        } else {
            measurables
                .iter()
                .enumerate()
                .for_each(|(index, child)| place(index, child));
        }
        let (width, height) =
            constraints.constrain(constraints.max_width, 10.0 * measurables.len() as f32);
        MeasureResult::new(Size::new(width, height), placements)
    }

    fn min_intrinsic_width(&self, _: Measurables<'_>, _: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_width(&self, _: Measurables<'_>, _: f32) -> f32 {
        0.0
    }

    fn min_intrinsic_height(&self, _: Measurables<'_>, _: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _: Measurables<'_>, _: f32) -> f32 {
        0.0
    }
}

// A parent that places 600 children in reverse order finds each child's
// placement with no allocation: a pass allocates what one that places them
// in order does.
fn placing_children_out_of_order_allocates_nothing() {
    let stacked = |reversed: bool| {
        pass_allocations(run_test_composition(move || {
            Layout(
                Modifier::empty().fill_max_width(),
                StackedRows { reversed },
                || rows(600),
            );
        }))
    };
    let (in_order, reversed) = (stacked(false), stacked(true));
    println!(
        "allocations a pass of 600 children placed in order: {in_order}, reversed: {reversed}"
    );
    assert_eq!(reversed, in_order, "the reversed placements allocate");
}

/// A layout pass as an app shell runs it, with no layout or semantics tree,
/// lends its applier to the host the pass before gave back, and each node
/// fills its last measurement: remeasuring a column of three rows allocates
/// the builder alone.
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
    assert!(allocations <= 1, "{allocations} allocations a pass");
}

/// A row whose width changes inside a box of fixed size measures again, and
/// so does the box, while the column around them keeps its measurement:
/// each node that measures again fills its last measurement, and the pass
/// allocates the builder alone.
fn a_kept_column_lets_its_changed_children_fill_their_measurements() {
    let width_handle = Rc::new(Cell::new(None));
    let output = Rc::clone(&width_handle);
    let mut composition = run_test_composition(move || {
        let width = remember(|| mutableStateOf(10.0_f32)).with(|state| *state);
        output.set(Some(width));
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || {
                Box(
                    Modifier::empty().fill_max_width().height(10.0),
                    BoxSpec::default(),
                    || {},
                );
                Box(
                    Modifier::empty().size(Size::new(50.0, 10.0)),
                    BoxSpec::default(),
                    move || {
                        Box(
                            Modifier::empty().width(width.value()).height(10.0),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    });
    let width = width_handle.get().expect("the width state");
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let options = MeasureLayoutOptions {
        collect_semantics: false,
        build_layout_tree: false,
    };
    let mut allocations = 0;
    for pass in 0..PASSES + 2 {
        width.set_value(10.0 + pass as f32);
        while composition
            .process_invalid_scopes()
            .expect("the row recomposes")
        {}
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle.clone());
        let region = Region::new(GLOBAL);
        measure_layout_with_options(&mut applier, root, Size::new(100.0, 4000.0), options)
            .expect("the column measures");
        if pass >= 2 {
            allocations += region.change().allocations;
        }
        applier.clear_runtime_handle();
    }
    let allocations = allocations / PASSES;
    println!("allocations of a pass under a kept column: {allocations}");
    assert!(allocations <= 1, "{allocations} allocations a pass");
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

// A paragraph of 10,000 characters, far more glyphs than the layouter keeps
// room for between short texts, measured at a width a few words wider each
// pass, so it breaks its lines anew: it shapes in the glyph storage of the
// pass before.
fn remeasuring_a_long_paragraph_keeps_its_glyph_storage() {
    let text = "word ".repeat(2_000);
    let composition = run_test_composition(move || {
        Text(text.clone(), Modifier::empty(), TextStyle::default());
    });
    let allocations = pass_allocations_at(composition, |pass| 200.0 + 60.0 * pass as f32);
    println!("allocations of remeasuring a long paragraph: {allocations}");
    assert!(
        allocations <= LONG_PARAGRAPH_ALLOCATIONS,
        "{allocations} allocations a pass"
    );
}

// A long paragraph's glyph room stays from one frame's layout to the next
// frame's, and goes when the texts rest: the next long paragraph takes it
// anew.
fn a_rest_gives_back_a_long_paragraph_glyph_room() {
    let text = AnnotatedString::from("word ".repeat(2_000).as_str());
    let style = TextStyle::default();
    let context = cranpose_ui::AppContext::new();
    context.enter(|| {
        let layout_at = |width: f32| {
            context.start_text_frame();
            let region = Region::new(GLOBAL);
            let options = TextLayoutOptions::default();
            drop(prepare_text_layout(&text, &style, options, Some(width)));
            region.change().allocations
        };
        layout_at(200.0);
        let kept = layout_at(260.0);
        context.trim();
        let after_rest = layout_at(320.0);
        println!("allocations of a long paragraph's layout: {kept}, after a rest: {after_rest}");
        assert!(after_rest > kept, "the rest kept the glyph room");
    });
}

// Allocations of composing a column of `count` rows of two boxes each,
// with `headroom` memory left.
fn rows_of_boxes_allocations(count: usize, headroom: MemoryHeadroom) -> usize {
    let region = Region::new(GLOBAL);
    let composition = run_test_composition(move || {
        set_memory_headroom(headroom);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for _ in 0..count {
                Row(Modifier::empty(), RowSpec::default(), || {
                    Box(
                        Modifier::empty().size_points(10.0, 10.0),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty().size_points(10.0, 10.0),
                        BoxSpec::default(),
                        || {},
                    );
                });
            }
        });
    });
    let allocations = region.change().allocations;
    drop(composition);
    allocations
}

// Composing a row of two boxes makes its three layout nodes, each with its
// box and parts, and the size modifiers of the boxes, one allocation each. The rows and the boxes
// share their measure policies, and a row keeps its two children in itself.
// With little memory left the recycle pool keeps no shells, so a node made
// fresh makes no shell to seed it with either.
fn composing_a_row_of_boxes_allocates_within_its_budget() {
    for headroom in [MemoryHeadroom::Ample, MemoryHeadroom::Tight] {
        rows_of_boxes_allocations(4, headroom);
        let per_row = (rows_of_boxes_allocations(40, headroom)
            - rows_of_boxes_allocations(20, headroom))
            / 20;
        println!("allocations of composing a row of two boxes, {headroom:?} memory: {per_row}");
        assert!(
            per_row <= 10,
            "{per_row} allocations a row, {headroom:?} memory"
        );
    }
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
fn layout_and_modifiers_stay_within_their_allocation_budgets() {
    a_first_measure_allocates_at_most_two_per_row();
    remeasuring_a_row_allocates_nothing();
    remeasuring_deeply_nested_rows_and_columns_allocates_nothing_more();
    wide_nested_columns_leave_a_bounded_set_of_lists();
    remeasuring_a_wide_column_allocates_nothing_more();
    placing_children_out_of_order_allocates_nothing();
    a_layout_pass_lends_its_applier_without_rebuilding_it();
    a_kept_column_lets_its_changed_children_fill_their_measurements();
    an_animation_frame_allocates_only_its_recomposition();
    remeasuring_a_long_paragraph_keeps_its_glyph_storage();
    a_rest_gives_back_a_long_paragraph_glyph_room();
    composing_a_row_of_boxes_allocates_within_its_budget();
    if !cfg!(feature = "inspection") {
        a_modifier_chain_allocates_its_elements_and_one_shared_box();
    }
    resolving_text_direction_does_not_allocate();
    owned_bitmaps_allocate_only_metadata_and_clones_share_storage();
    owned_bitmaps_do_not_retain_spare_pixel_capacity();
    a_thread_that_held_a_composition_frees_its_memory_at_exit();
}

main_thread_test!(layout_and_modifiers_stay_within_their_allocation_budgets);
