use cranpose_ui::MemoryHeadroom;

use crate::lazy_screen::{LazyScreen, NODES_PER_ROW, TitledRows, VIEWPORT};

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
