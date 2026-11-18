#![allow(clippy::print_stdout, reason = "a test prints what it measured")]

use std::{alloc::System, cell::Cell, ops::Range, rc::Rc};

use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_core::{mutableStateOf, remember};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, rememberLazyListState};
use cranpose_ui::{
    Constraints, Layout, LazyColumn, LazyColumnSpec, MeasureLayoutOptions, MeasureResult,
    MemoryHeadroom, Modifier, TestComposition, Text, TextLayoutOptions, TextStyle, measure_layout,
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
// The allocations of a lazy row measured again at a new width: what a debug
// build allocates to compose its kept slot again for the check of
// `SubcomposeMeasureScope::subcompose`.
const LAZY_ROW_ALLOCATIONS: f64 = 8.0;
// The allocations of a pass of a lazy column scrolled a little.
const LAZY_SCROLL_ALLOCATIONS: usize = 19;
// The allocations of a pass of a lazy column scrolled by a row.
const LAZY_ROW_SCROLL_ALLOCATIONS: usize = 39;
// The allocations of a measure pass of an empty lazy column at a new width.
const EMPTY_LAZY_PASS_ALLOCATIONS: usize = 11;

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
// list of children, which takes no allocation of its own, and the child's
// measurement sits in its cache.
fn a_first_measure_allocates_at_most_one_per_row() {
    let few = first_measure_allocations(10);
    let many = first_measure_allocations(110);
    let per_row = many.saturating_sub(few) as f64 / 100.0;
    println!("allocations per row at a first measure: {per_row}");
    assert!(
        per_row <= 1.0,
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

// A lazy column measures in a subcomposition of its own each pass; with no
// rows, the pass allocates what that subcomposition needs.
fn an_empty_lazy_column_pass_allocates_within_its_budget() {
    let allocations = pass_allocations(run_test_composition(|| {
        LazyColumn(
            Modifier::empty().fill_max_size(),
            rememberLazyListState(),
            LazyColumnSpec::default(),
            |scope| {
                scope.items(LazyItems::new(0), |_| {});
            },
        );
    }));
    println!("allocations a pass of an empty lazy column: {allocations}");
    assert!(
        allocations <= EMPTY_LAZY_PASS_ALLOCATIONS,
        "{allocations} allocations a pass of an empty lazy column"
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

// Allocations per pass of a lazy column `visible` rows tall, at a new width
// each pass, so each visible row measures again from the slot it keeps.
fn lazy_remeasure_allocations(visible: usize) -> usize {
    pass_allocations(run_test_composition(move || {
        LazyColumn(
            Modifier::empty()
                .fill_max_width()
                .height(10.0 * visible as f32),
            rememberLazyListState(),
            LazyColumnSpec::default(),
            |scope| {
                scope.items(LazyItems::new(1_000), |_| rows(1));
            },
        );
    }))
}

// A lazy column measured at a new width composes none of its rows again:
// a row that keeps its slot measures again with no list of its roots.
fn remeasuring_lazy_rows_allocates_no_root_lists() {
    let few = lazy_remeasure_allocations(10);
    let many = lazy_remeasure_allocations(40);
    let per_row = many.saturating_sub(few) as f64 / 30.0;
    println!("allocations per remeasured lazy row: {per_row}");
    assert!(
        per_row <= LAZY_ROW_ALLOCATIONS,
        "{per_row} allocations per remeasured lazy row"
    );
}

// Allocations per pass of a lazy column 10 rows tall scrolled by `delta`
// before each pass, after two passes that warm it up.
fn lazy_scroll_allocations(delta: f32) -> usize {
    let captured = Rc::new(Cell::new(None));
    let capture = Rc::clone(&captured);
    let mut composition = run_test_composition(move || {
        let list = rememberLazyListState();
        capture.set(Some(list));
        LazyColumn(
            Modifier::empty().fill_max_width().height(100.0),
            list,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(LazyItems::new(1_000), |_| rows(1));
            },
        );
    });
    let list = captured.get().expect("the composition remembered its list");
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let mut pass = || {
        list.dispatch_scroll_delta(-delta);
        measure_layout(&mut applier, root, Size::new(100.0, 4000.0)).expect("the list measures");
    };
    pass();
    pass();
    let region = Region::new(GLOBAL);
    for _ in 0..PASSES {
        pass();
    }
    let allocations = region.change().allocations;
    applier.clear_runtime_handle();
    allocations / PASSES
}

// A lazy column places its items in the list of the items its pass before
// placed: scrolled a pixel, it keeps its rows; scrolled a row, a row comes
// in, which the pass from the kept rows refuses before the pass that
// composes it.
fn scrolling_lazy_rows_allocates_within_its_budget() {
    for (delta, budget) in [
        (1.0, LAZY_SCROLL_ALLOCATIONS),
        (10.0, LAZY_ROW_SCROLL_ALLOCATIONS),
    ] {
        let allocations = lazy_scroll_allocations(delta);
        println!("allocations a pass of a lazy column scrolled {delta} px: {allocations}");
        assert!(
            allocations <= budget,
            "{allocations} allocations a pass of a lazy column scrolled {delta} px"
        );
    }
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
// box and parts, and the row's list of children. The size modifiers of the
// boxes take no allocation. The rows and the boxes share their measure
// policies.
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
            per_row <= 9,
            "{per_row} allocations a row, {headroom:?} memory"
        );
    }
}

// Allocations and reallocations a pass of a body that reads a state and
// composes a column of `count` boxes again, which keeps them all.
fn column_recomposition_allocations(count: usize) -> usize {
    let tick_handle = Rc::new(Cell::new(None));
    let output = Rc::clone(&tick_handle);
    let mut composition = run_test_composition(move || {
        let tick = remember(|| mutableStateOf(0_u32)).with(|state| *state);
        output.set(Some(tick));
        let value = tick.value();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let _: u32 = value;
            for _ in 0..count {
                Box(Modifier::empty(), BoxSpec::default(), || {});
            }
        });
    });
    let tick = tick_handle.get().expect("the tick state");
    let mut pass = |index: u32| {
        tick.set_value(index);
        while composition
            .process_invalid_scopes()
            .expect("the column composes again")
        {}
    };
    for index in 1..3 {
        pass(index);
    }
    let region = Region::new(GLOBAL);
    let passes = u32::try_from(PASSES).expect("ten passes");
    for index in 3..3 + passes {
        pass(index);
    }
    let change = region.change();
    (change.allocations + change.reallocations) / PASSES
}

// A pass that composes a parent's children again reads them into one list
// and gives the children it composes a list of their number at once: a
// column of 600 boxes allocates and grows no more a pass than one of 20.
fn composing_a_wide_column_again_allocates_what_a_narrow_one_does() {
    let narrow = column_recomposition_allocations(20);
    let wide = column_recomposition_allocations(600);
    println!("allocations of composing a column again: 20 boxes {narrow}, 600 boxes {wide}");
    assert!(
        wide <= narrow,
        "{wide} allocations a pass for 600 boxes, {narrow} for 20"
    );
}

// Allocations and reallocations a frame of the frames `counted`, after the
// frames `warm`, of a column that shows `count` items in each frame `shows`
// takes, under keys of that frame: a frame composes its items as new nodes,
// removes the items of the frame before and lays the column out. The first
// composition is frame 0.
fn keyed_items_allocations(
    (count, item): (usize, fn()),
    shows: fn(u32) -> bool,
    warm: Range<u32>,
    counted: Range<u32>,
) -> usize {
    let round_handle = Rc::new(Cell::new(None));
    let output = Rc::clone(&round_handle);
    let mut composition = run_test_composition(move || {
        let round = remember(|| mutableStateOf(0_u32)).with(|state| *state);
        output.set(Some(round));
        let value = round.value();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            if !shows(value) {
                return;
            }
            for index in 0..count {
                cranpose_core::key((value, index), item);
            }
        });
    });
    let round = round_handle.get().expect("the round state");
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut frame = |index: u32| {
        round.set_value(index);
        while composition
            .process_invalid_scopes()
            .expect("the column composes again")
        {}
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle.clone());
        measure_layout(&mut applier, root, Size::new(100.0, 4000.0)).expect("the column measures");
        applier.clear_runtime_handle();
    };
    warm.for_each(&mut frame);
    let frames = counted.len();
    let region = Region::new(GLOBAL);
    counted.for_each(&mut frame);
    let change = region.change();
    (change.allocations + change.reallocations) / frames
}

fn an_empty_box() {
    Box(Modifier::empty(), BoxSpec::default(), || {});
}

fn a_row_of_one_box() {
    Row(Modifier::empty(), RowSpec::default(), an_empty_box);
}

fn a_row_of_two_boxes() {
    Row(Modifier::empty(), RowSpec::default(), || {
        an_empty_box();
        an_empty_box();
    });
}

// A node that leaves the tree goes to the recycle pool whole, and the node
// that takes its place in the next frame takes its box, its parts and its
// lists: a frame that swaps rows of two boxes for new ones allocates what a
// frame that swaps rows of one box does. The pool fills over the first
// frames.
fn a_frame_that_swaps_nodes_for_new_ones_allocates_nothing_for_them() {
    let passes = u32::try_from(PASSES).expect("ten passes");
    let swapped = |rows| keyed_items_allocations(rows, |_| true, 1..16, 16..16 + passes);
    let one = swapped((8, a_row_of_one_box));
    let two = swapped((8, a_row_of_two_boxes));
    println!("allocations of a frame that swaps eight rows: of one box {one}, of two boxes {two}");
    assert!(
        two <= one,
        "{two} allocations for rows of two boxes, {one} for one"
    );
}

// The nodes that the first composition made fresh, which a frame then
// removed, serve the nodes of the next frame as the others do:
// twenty-four boxes that come back allocate what twelve do.
fn nodes_made_fresh_serve_the_nodes_of_a_later_frame() {
    let regrown =
        |count| keyed_items_allocations((count, an_empty_box), |frame| frame != 1, 1..2, 2..3);
    let twelve = regrown(12);
    let twenty_four = regrown(24);
    println!(
        "allocations of a frame that brings boxes back: twelve {twelve}, twenty-four {twenty_four}"
    );
    assert!(
        twenty_four <= twelve,
        "{twenty_four} allocations for twenty-four boxes, {twelve} for twelve"
    );
}

// A modifier holds a plain element by value: one alone takes no
// allocation, and a chain of them built link by link takes one shared
// allocation it extends in place.
fn a_modifier_chain_of_plain_elements_allocates_one_shared_box() {
    drop(Modifier::empty().padding(1.0).background(Color::RED));
    let region = Region::new(GLOBAL);
    let modifier = Modifier::empty().padding(1.0);
    let change = region.change();
    drop(modifier);
    assert_eq!(change.allocations, 0, "{change:?}");

    let region = Region::new(GLOBAL);
    let modifier = Modifier::empty()
        .padding(1.0)
        .background(Color::RED)
        .rounded_corners(3.0);
    let change = region.change();
    drop(modifier);
    assert_eq!(change.allocations, 1, "{change:?}");
}

// A node whose chain draws one background collects its slices and that draw
// into one block, with no growth.
fn slices_of_one_background_allocate_one_block() {
    let context = cranpose_ui::AppContext::new();
    let _scope = context.enter_scope();
    let node = || {
        cranpose_ui::LayoutNode::new(
            Modifier::empty().background(Color::RED).padding(1.0),
            Rc::new(StackedRows { reversed: false }),
        )
    };
    drop(node().modifier_slices_snapshot());
    let fresh = node();
    let region = Region::new(GLOBAL);
    let slices = fresh.modifier_slices_snapshot();
    let change = region.change();
    assert_eq!(
        slices.draw_commands().len(),
        1,
        "the chain draws one background"
    );
    assert_eq!(change.allocations, 1, "{change:?}");
    assert_eq!(change.reallocations, 0, "{change:?}");
}

// A text that draws one background keeps that draw in its slices: its first
// collection grows nothing, and collecting its slices again for another
// color allocates nothing.
fn slices_of_a_text_with_a_background_keep_their_room() {
    use cranpose_ui::text::{AnnotatedString, TextLayoutOptions, TextStyle};

    let context = cranpose_ui::AppContext::new();
    let _scope = context.enter_scope();
    let modifier = |color| {
        Modifier::empty()
            .background(color)
            .then(Modifier::with_element(
                cranpose_ui::TextModifierElement::new(
                    AnnotatedString::from("Behind"),
                    TextStyle::default(),
                    TextLayoutOptions::default(),
                    cranpose_ui::Density::new(2.0, 1.0),
                ),
            ))
    };
    let mut node = cranpose_ui::LayoutNode::new(
        modifier(Color::RED),
        Rc::new(StackedRows { reversed: false }),
    );
    let region = Region::new(GLOBAL);
    let slices = node.modifier_slices_snapshot();
    let first = region.change();
    assert_eq!(
        slices.draw_commands().len(),
        1,
        "the text draws one background"
    );
    drop(slices);

    let blue = modifier(Color::BLUE);
    let region = Region::new(GLOBAL);
    node.set_modifier(blue);
    let slices = node.modifier_slices_snapshot();
    let again = region.change();
    assert_eq!(
        slices.draw_commands().len(),
        1,
        "the text draws one background again"
    );
    println!("allocations of a text's slices: first {first:?}, again {again:?}");
    assert_eq!(first.reallocations, 0, "{first:?}");
    assert_eq!(again.reallocations, 0, "{again:?}");
    assert_eq!(again.allocations, 0, "{again:?}");
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
    a_first_measure_allocates_at_most_one_per_row();
    remeasuring_a_row_allocates_nothing();
    remeasuring_lazy_rows_allocates_no_root_lists();
    scrolling_lazy_rows_allocates_within_its_budget();
    an_empty_lazy_column_pass_allocates_within_its_budget();
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
    composing_a_wide_column_again_allocates_what_a_narrow_one_does();
    a_frame_that_swaps_nodes_for_new_ones_allocates_nothing_for_them();
    nodes_made_fresh_serve_the_nodes_of_a_later_frame();
    if !cfg!(feature = "inspection") {
        a_modifier_chain_of_plain_elements_allocates_one_shared_box();
    }
    slices_of_one_background_allocate_one_block();
    slices_of_a_text_with_a_background_keep_their_room();
    resolving_text_direction_does_not_allocate();
    owned_bitmaps_allocate_only_metadata_and_clones_share_storage();
    owned_bitmaps_do_not_retain_spare_pixel_capacity();
    a_thread_that_held_a_composition_frees_its_memory_at_exit();
}

main_thread_test!(layout_and_modifiers_stay_within_their_allocation_budgets);
