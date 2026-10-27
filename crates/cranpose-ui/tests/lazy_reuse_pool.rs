use crate::lazy_screen::{LazyScreen, NODES_PER_ROW, TitledRows, VIEWPORT};

#[test]
fn a_list_that_jumps_back_to_its_start_keeps_no_more_rows_than_its_scroll_takes() {
    let mut screen = LazyScreen::new(VIEWPORT, TitledRows);
    // A row a frame, as a list scrolls by itself, down to row 30.
    for row in 1..=30 {
        screen.scroll_to(row, 0.0);
        screen.texts();
    }
    let before = screen.nodes();
    screen.scroll_to(0, 0.0);
    screen.texts();
    // On a row a frame again, for more passes than a row waits in the pool.
    for row in 1..=20 {
        screen.scroll_to(row, 0.0);
        screen.texts();
    }
    let after = screen.nodes();
    assert!(
        after <= before + 2 * NODES_PER_ROW,
        "the rows the jump left go, but for what the scroll takes back: {before} nodes before the jump, {after} after"
    );
}
