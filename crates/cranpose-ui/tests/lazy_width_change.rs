// A lazy list laid out at a new width each frame measures its kept rows
// again; each frame draws what a list composed there at once draws.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState};
use cranpose_ui::{LazyColumn, LazyColumnSpec, Modifier, Size, Text, TextStyle, composable};

use crate::lazy_screen::{LazyScreen, ROW_HEIGHT};

const HEIGHT: f32 = 240.0;

// The rows of `ids`, keyed by their ids, one text each; the row of id 7 has
// `seven` texts, which it reads itself.
#[composable]
fn KeyedRows(list: LazyListState, ids: MutableState<Vec<u64>>, seven: MutableState<usize>) {
    let ids: Rc<[u64]> = ids.value().into();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            let keys = Rc::clone(&ids);
            let rows = Rc::clone(&ids);
            scope.items(
                LazyItems::new(ids.len()).key(move |index| keys[index]),
                move |index| {
                    let id = rows[index];
                    let texts = if id == 7 { seven.value() } else { 1 };
                    for text in 0..texts {
                        Text(
                            format!("Row {id}.{text}"),
                            Modifier::empty().height(ROW_HEIGHT),
                            TextStyle::default(),
                        );
                    }
                },
            );
        },
    );
}

// A screen of `KeyedRows` with the states that the test changes.
struct Screen {
    lazy: LazyScreen,
    ids: MutableState<Vec<u64>>,
    seven: MutableState<usize>,
}

impl Screen {
    fn new(width: f32, ids: Vec<u64>, seven: usize) -> Self {
        let captured = Rc::new(Cell::new(None));
        let capture = Rc::clone(&captured);
        let lazy = LazyScreen::new(Size::new(width, HEIGHT), move |list| {
            let first_ids = ids.clone();
            let ids = rememberMutableStateOf(move || first_ids);
            let seven = rememberMutableStateOf(move || seven);
            capture.set(Some((ids, seven)));
            KeyedRows(list, ids, seven);
        });
        let (ids, seven) = captured.get().expect("the composition remembered the rows");
        Self { lazy, ids, seven }
    }

    fn change(&mut self, change: impl FnOnce(&MutableState<Vec<u64>>, &MutableState<usize>)) {
        self.lazy.context.enter(|| change(&self.ids, &self.seven));
        self.lazy.settle();
    }

    // Lays the screen out `width` wide and checks it against a list of the
    // same rows composed at that width and position at once.
    fn check_at(&mut self, width: f32, frame: &str) {
        let texts = self.lazy.texts_at(Size::new(width, HEIGHT));
        let (index, offset) = self.lazy.position();
        let (ids, seven) = self
            .lazy
            .context
            .enter(|| (self.ids.value(), self.seven.value()));
        let mut fresh = Self::new(width, ids, seven);
        fresh.lazy.scroll_to(index, offset);
        assert_eq!(
            texts,
            fresh.lazy.texts_at(Size::new(width, HEIGHT)),
            "{frame}"
        );
    }
}

fn hundred_ids() -> Vec<u64> {
    (0..100).collect()
}

#[test]
fn a_row_removed_while_the_width_changes_leaves_no_trace() {
    let mut screen = Screen::new(320.0, hundred_ids(), 1);
    screen.check_at(310.0, "the first frame at a new width");
    screen.change(|ids, _| ids.update(|ids| ids.retain(|&id| id != 3)));
    screen.check_at(300.0, "the frame after row 3 left");
    screen.change(|ids, _| ids.update(|ids| ids.retain(|&id| id != 0)));
    screen.check_at(290.0, "the frame after row 0 left");
}

#[test]
fn a_row_whose_key_moves_while_the_width_changes_shows_at_its_new_place() {
    let mut screen = Screen::new(320.0, hundred_ids(), 2);
    screen.check_at(310.0, "the first frame at a new width");
    screen.change(|ids, _| {
        ids.update(|ids| {
            let moved = ids.remove(7);
            ids.insert(1, moved);
        });
    });
    screen.check_at(300.0, "the frame after row 7 moved up");
    screen.change(|ids, _| ids.update(|ids| ids.swap(0, 4)));
    screen.check_at(290.0, "the frame after rows 0 and 4 swapped");
}

#[test]
fn a_row_kept_out_of_view_shows_its_current_texts_when_it_comes_back() {
    let mut screen = Screen::new(320.0, hundred_ids(), 1);
    screen.check_at(310.0, "the first frame at a new width");
    screen.lazy.scroll_to(30, 0.0);
    screen.check_at(300.0, "the frame with row 7 out of view");
    screen.change(|_, seven| seven.set(3));
    screen.check_at(290.0, "the frame after row 7 changed out of view");
    screen.lazy.scroll_to(0, 0.0);
    screen.check_at(280.0, "the frame with row 7 back");
    screen.change(|_, seven| seven.set(2));
    screen.check_at(270.0, "the frame after row 7 lost a text in view");
}
