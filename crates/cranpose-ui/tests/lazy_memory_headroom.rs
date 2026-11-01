use std::cell::Cell;

use cranpose_core::remember;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState};
use cranpose_ui::{
    LazyColumn, LazyColumnSpec, MemoryHeadroom, Modifier, Text, TextStyle, composable,
};

use crate::lazy_screen::{LazyScreen, NODES_PER_ROW, ROW_HEIGHT, TitledRows, VIEWPORT};

thread_local! {
    // The rows of `CountedRows` composed now, and the most composed at once.
    static ROWS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

// What each row of `CountedRows` remembers: it counts the row while the
// row's composition lives, in the reuse pool too.
struct RowCount;

impl RowCount {
    fn new() -> Self {
        ROWS.with(|rows| {
            let (now, most) = rows.get();
            rows.set((now + 1, most.max(now + 1)));
        });
        Self
    }
}

impl Drop for RowCount {
    fn drop(&mut self) {
        ROWS.with(|rows| {
            let (now, most) = rows.get();
            rows.set((now - 1, most));
        });
    }
}

// Fifty rows of `list` that count themselves in `ROWS`.
#[composable]
fn CountedRows(list: LazyListState) {
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(50), move |index| {
                remember(RowCount::new);
                Text(
                    format!("Row {index}"),
                    Modifier::empty().height(ROW_HEIGHT),
                    TextStyle::default(),
                );
            });
        },
    );
}

#[test]
fn a_list_with_little_memory_left_keeps_only_the_rows_it_shows() {
    let mut ample = LazyScreen::new(VIEWPORT, TitledRows);
    let mut tight = LazyScreen::new(VIEWPORT, TitledRows);
    tight.set_headroom(MemoryHeadroom::Tight);
    for row in 1..=10 {
        ample.scroll_to(row, 0.0);
        tight.scroll_to(row, 0.0);
        assert_eq!(
            tight.texts(),
            ample.texts(),
            "the frame at row {row} draws the same rows"
        );
    }
    // Fifteen rows fill the viewport. Ample memory keeps two rows composed on
    // each side; tight memory none, and one row in the reuse pool.
    let (kept, shown) = (ample.nodes(), tight.nodes());
    assert!(
        shown + 3 * NODES_PER_ROW <= kept,
        "tight memory keeps {shown} nodes, ample {kept}"
    );

    // A list that runs short of memory lets its rows beyond the viewport go
    // on its next pass.
    ample.set_headroom(MemoryHeadroom::Tight);
    for screen in [&mut ample, &mut tight] {
        screen.scroll_to(11, 0.0);
        screen.texts();
    }
    assert_eq!(ample.nodes(), tight.nodes());
}

#[test]
fn memory_turns_tight_below_a_quarter_free_and_ample_again_above_three_eighths() {
    let total = 1000;
    let ample = MemoryHeadroom::Ample;
    assert_eq!(ample.of_heap(250, total), MemoryHeadroom::Ample);
    assert_eq!(ample.of_heap(249, total), MemoryHeadroom::Tight);
    let tight = MemoryHeadroom::Tight;
    assert_eq!(tight.of_heap(375, total), MemoryHeadroom::Tight);
    assert_eq!(tight.of_heap(376, total), MemoryHeadroom::Ample);
}

#[test]
fn a_list_with_little_memory_left_keeps_no_layout_nodes_for_later_rows() {
    let mut ample = LazyScreen::new(VIEWPORT, TitledRows);
    let mut tight = LazyScreen::new(VIEWPORT, TitledRows);
    tight.set_headroom(MemoryHeadroom::Tight);
    // Jumps that bring rows no pass composed before.
    for row in [35, 0, 35] {
        ample.scroll_to(row, 0.0);
        tight.scroll_to(row, 0.0);
        assert_eq!(
            tight.texts(),
            ample.texts(),
            "the frame at row {row} draws the same rows"
        );
    }
    assert_eq!(
        tight.recycled_layout_nodes(),
        0,
        "tight memory frees the layout nodes the rows leave"
    );
}

#[test]
fn a_jump_with_little_memory_left_lets_the_rows_it_leaves_go_before_it_composes_new_ones() {
    let mut screen = LazyScreen::new(VIEWPORT, CountedRows);
    screen.set_headroom(MemoryHeadroom::Tight);
    // Fifteen rows fill the viewport; tight memory keeps one more in the
    // reuse pool once a pass lets the rows beyond the viewport go. Then
    // jumps away from the rows shown, forward and back.
    screen.scroll_to(1, 0.0);
    screen.texts();
    for row in [35, 0] {
        ROWS.with(|rows| rows.set((rows.get().0, rows.get().0)));
        screen.scroll_to(row, 0.0);
        let texts = screen.texts();
        assert_eq!(
            texts.first().map(|text| text.0.clone()),
            Some(format!("Row {row}"))
        );
        let (now, most) = ROWS.with(Cell::get);
        assert!(
            most <= 16,
            "the jump to row {row} composed {most} rows at once, {now} after it"
        );
    }
}
