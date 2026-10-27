use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState};
use cranpose_ui::{
    LazyColumn, LazyColumnSpec, Modifier, Text, TextStyle, composable, retained_measure_count,
};

use crate::lazy_screen::{LazyScreen, ROW_HEIGHT, VIEWPORT};

type LiveRows = Rc<RefCell<BTreeSet<usize>>>;

// Fifty rows of `list`; row 3 grows by 16 px while `grown`. A row is in
// `live` while its effects run.
#[composable]
fn Rows(grown: MutableState<bool>, live: LiveRows, list: LazyListState) {
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(50), move |index| {
                let (text, height) = match index {
                    3 if grown.value() => ("Row 3 grown".to_owned(), ROW_HEIGHT + 16.0),
                    _ => (format!("Row {index}"), ROW_HEIGHT),
                };
                Text(text, Modifier::empty().height(height), TextStyle::default());
                let live = Rc::clone(&live);
                cranpose_core::DisposableEffect(index, move |scope| {
                    live.borrow_mut().insert(index);
                    scope.on_dispose(move || {
                        live.borrow_mut().remove(&index);
                    })
                });
            });
        },
    );
}

// The rows on a lazy screen, with the switch that grows row 3 and the rows
// whose effects run.
struct Screen {
    lazy: LazyScreen,
    grown: MutableState<bool>,
    live: LiveRows,
}

impl Screen {
    fn new() -> Self {
        let live = LiveRows::default();
        let rows_live = Rc::clone(&live);
        let captured = Rc::new(Cell::new(None));
        let capture = Rc::clone(&captured);
        let lazy = LazyScreen::new(VIEWPORT, move |list| {
            let grown = rememberMutableStateOf(|| false);
            capture.set(Some(grown));
            Rows(grown, Rc::clone(&rows_live), list);
        });
        let grown = captured
            .get()
            .expect("the composition remembered the switch");
        Self { lazy, grown, live }
    }

    // A screen whose first layout shows the list at `index`, `offset`.
    fn at(index: usize, offset: f32) -> Self {
        let mut screen = Self::new();
        screen.lazy.scroll_to(index, offset);
        screen
    }
}

#[test]
fn a_list_scrolled_a_little_at_a_time_draws_each_frame_as_a_list_shown_there_at_once() {
    let mut screen = Screen::new();
    screen.lazy.texts();
    let retained_before = retained_measure_count();
    let mut frames = 0u64;
    // From 50 px on, 2.5 px a frame, counted in half pixels.
    for step in 0..40u16 {
        let half_pixels = 100 + 5 * step;
        let scroll = f32::from(half_pixels) / 2.0;
        let index = usize::from(half_pixels / 48);
        let offset = f32::from(half_pixels % 48) / 2.0;
        screen.lazy.scroll_to(index, offset);
        let texts = screen.lazy.texts();
        let mut fresh = Screen::at(index, offset);
        assert_eq!(texts, fresh.lazy.texts(), "the frame at {scroll} px");
        assert_eq!(
            screen.lazy.position(),
            fresh.lazy.position(),
            "the position at {scroll} px"
        );
        frames += 1;
    }
    for delta in [-1.5_f32, -3.25, 2.0, -0.75, 5.5] {
        screen.lazy.scroll_by(delta);
        let texts = screen.lazy.texts();
        let (index, offset) = screen.lazy.position();
        assert_eq!(
            texts,
            Screen::at(index, offset).lazy.texts(),
            "the frame after {delta} px"
        );
        frames += 1;
    }
    let retained = retained_measure_count() - retained_before;
    assert!(
        retained * 2 > frames,
        "liveness: most frames keep the rows of the frame before, {retained} of {frames} measured from them"
    );
}

#[test]
fn a_row_that_changes_in_the_frame_the_list_scrolls_shows_its_new_text_and_size() {
    let mut screen = Screen::new();
    screen.lazy.scroll_to(1, 2.0);
    screen.lazy.texts();
    screen.lazy.scroll_to(1, 4.0);
    screen.lazy.texts();

    screen.lazy.context.enter(|| screen.grown.set(true));
    screen.lazy.scroll_to(1, 6.0);
    let texts = screen.lazy.texts();

    let row_3_top = 2.0 * ROW_HEIGHT - 6.0;
    for row in [
        ("Row 3 grown".to_owned(), row_3_top),
        ("Row 4".to_owned(), row_3_top + ROW_HEIGHT + 16.0),
    ] {
        assert!(
            texts.contains(&row),
            "{row:?} is on the screen, got {texts:?}"
        );
    }
}

#[test]
fn a_row_that_leaves_the_kept_rows_ends_its_effects() {
    let mut screen = Screen::new();
    screen.lazy.texts();
    // The 360 px viewport shows fifteen rows at a row's edge and sixteen
    // between, and the list keeps two more composed after them.
    for offset in [5.0, 0.0, 7.5, 0.0, 12.0, 0.0] {
        screen.lazy.scroll_to(0, offset);
        screen.lazy.texts();
        let last = if offset > 0.0 { 17 } else { 16 };
        assert_eq!(
            *screen.live.borrow(),
            (0..=last).collect::<BTreeSet<_>>(),
            "the rows composed at {offset} px"
        );
    }
}
