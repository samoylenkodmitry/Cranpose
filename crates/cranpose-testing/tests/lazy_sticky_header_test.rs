//! A sticky header holds the start of a lazy column while its items scroll
//! under it, the next header pushes it out, and it takes the touches over
//! the items it covers, through the public shell.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_testing::{robot::TestRenderer, robot_assertions::find_semantics_node};
use cranpose_ui::{Box, BoxSpec, LazyColumn, LazyColumnSpec, LazyRow, LazyRowSpec, Modifier};

const ROW: f32 = 20.0;
const SECTION_ROWS: usize = 10;

struct List {
    shell: AppShell<TestRenderer>,
    state: Rc<Cell<Option<LazyListState>>>,
    taps: Rc<RefCell<Vec<String>>>,
}

fn row(name: String, taps: &Taps) {
    let (tapped, description) = (Rc::clone(taps), name.clone());
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(ROW)
            .clickable(move |_| tapped.borrow_mut().push(name.clone()))
            .content_description(description),
        BoxSpec::default(),
        || {},
    );
}

/// A column 200 wide and `height` high of `sections`, each a sticky header
/// and ten rows, all 20 high.
fn list(sections: &'static [&'static str], height: f32) -> List {
    List::new(move |list_state, tapped| {
        LazyColumn(
            Modifier::empty().size_points(200.0, height),
            list_state,
            LazyColumnSpec::default(),
            move |scope| {
                for &section in sections {
                    let header_taps = Rc::clone(&tapped);
                    scope.sticky_header(move || row(format!("{section} header"), &header_taps));
                    let row_taps = Rc::clone(&tapped);
                    scope.items(LazyItems::new(SECTION_ROWS), move |index| {
                        row(format!("{section}{index}"), &row_taps);
                    });
                }
            },
        );
    })
}

type Taps = Rc<RefCell<Vec<String>>>;

impl List {
    /// A 200 × 400 shell around the list `content` composes with its state.
    fn new(content: impl Fn(LazyListState, Taps) + 'static) -> Self {
        let state = Rc::new(Cell::new(None));
        let taps = Rc::new(RefCell::new(Vec::new()));
        let (captured, tapped) = (Rc::clone(&state), Rc::clone(&taps));
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let list_state = rememberLazyListState();
                captured.set(Some(list_state));
                content(list_state, Rc::clone(&tapped));
            },
        );
        shell.set_viewport(200.0, 400.0);
        shell.set_buffer_size(200, 400);
        shell.set_semantics_enabled(true);
        shell.update();
        Self { shell, state, taps }
    }

    fn scroll_to(&mut self, index: usize, offset: f32) {
        self.state
            .get()
            .expect("composed list")
            .scroll_to_item(index, offset);
        self.shell.update();
    }

    /// Where the item described as `name` lies, or `None` when it is not
    /// placed.
    fn bounds(&mut self, name: &str) -> Option<cranpose_ui::Rect> {
        let tree = self.shell.semantics_tree().expect("semantics");
        find_semantics_node(tree.root(), &|node| {
            node.description.as_deref() == Some(name)
        })
        .map(|node| node.bounds)
    }

    fn top(&mut self, name: &str) -> Option<f32> {
        self.bounds(name).map(|bounds| bounds.y)
    }

    fn tap(&mut self, x: f32, y: f32) -> Vec<String> {
        self.shell.set_cursor(x, y);
        self.shell.pointer_pressed();
        self.shell.pointer_released();
        self.shell.update();
        self.taps.take()
    }
}

#[test]
fn a_header_holds_the_top_while_its_rows_scroll_under_it() {
    let mut list = list(&["A", "B"], 100.0);
    assert_eq!(list.top("A header"), Some(0.0));
    list.scroll_to(3, 5.0);
    assert_eq!(list.top("A header"), Some(0.0), "the header stays");
    assert_eq!(list.top("A2"), Some(-5.0), "its rows scroll under it");
}

#[test]
fn a_header_far_above_the_viewport_comes_back_to_hold_the_top() {
    let mut list = list(&["A", "B"], 100.0);
    list.scroll_to(9, 0.0);
    assert_eq!(list.top("A8"), Some(0.0));
    assert_eq!(
        list.top("A header"),
        Some(0.0),
        "the header nine rows above is composed and held"
    );
}

#[test]
fn the_next_header_pushes_the_held_one_out() {
    let mut list = list(&["A", "B"], 100.0);
    // Rows A0..A9 sit at 20..220, so header B starts at 220: scrolled by
    // 210, it is 10 below the top.
    list.scroll_to(10, 10.0);
    assert_eq!(list.top("B header"), Some(10.0));
    assert_eq!(
        list.top("A header"),
        Some(-10.0),
        "header B pushes header A up"
    );
    list.scroll_to(11, 5.0);
    assert_eq!(list.top("B header"), Some(0.0), "header B holds the top");
    assert_eq!(list.top("A header"), None, "header A is gone");
}

#[test]
fn a_tap_on_the_held_header_reaches_the_header() {
    let mut list = list(&["A", "B"], 100.0);
    list.scroll_to(4, 10.0);
    assert_eq!(list.tap(100.0, 15.0), ["A header"], "not the row under it");
    assert_eq!(list.tap(100.0, 25.0), ["A4"], "below it the rows take taps");
}

#[test]
fn a_list_whose_rows_all_fit_keeps_its_headers_in_place() {
    let mut list = list(&["A"], 300.0);
    assert_eq!(list.top("A header"), Some(0.0));
    assert_eq!(list.top("A9"), Some(200.0));
    assert_eq!(list.tap(100.0, 30.0), ["A0"]);
}

#[test]
fn a_row_header_holds_the_start_edge() {
    let mut list = List::new(|list_state, _| {
        LazyRow(
            Modifier::empty().size_points(100.0, 40.0),
            list_state,
            LazyRowSpec::default(),
            |scope| {
                let column = |name: String| {
                    Box(
                        Modifier::empty()
                            .width(ROW)
                            .fill_max_height()
                            .content_description(name),
                        BoxSpec::default(),
                        || {},
                    );
                };
                scope.sticky_header(move || column("header".into()));
                scope.items(LazyItems::new(SECTION_ROWS), move |index| {
                    column(format!("column {index}"));
                });
            },
        );
    });
    list.scroll_to(4, 5.0);
    assert_eq!(list.bounds("header").map(|bounds| bounds.x), Some(0.0));
    assert_eq!(list.bounds("column 3").map(|bounds| bounds.x), Some(-5.0));
}
