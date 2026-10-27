//! A lazy vertical grid lays its items out in lines of cells, composes only
//! the lines near its viewport, gives a sticky header a whole line, and
//! scrolls by items, through the public shell.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListIntervalContent, LazyListScope};
use cranpose_testing::{robot::TestRenderer, robot_assertions::find_semantics_node};
use cranpose_ui::{
    Box, BoxSpec, GridCells, LayoutDirection, LazyGridState, LazyHorizontalGrid,
    LazyHorizontalGridSpec, LazyVerticalGrid, LazyVerticalGridSpec, Modifier,
    ProvideLayoutDirection, Rect, rememberLazyGridState,
};

const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 200.0;
const CELL_HEIGHT: f32 = 40.0;
const COLUMN_WIDTH: f32 = 50.0;

/// What a grid holds, in order.
#[derive(Clone, Copy)]
enum Part {
    Header(&'static str),
    Items(usize),
}

struct Grid {
    shell: AppShell<TestRenderer>,
    state: Rc<RefCell<Option<LazyGridState>>>,
}

fn tagged(name: String, height: f32) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(height)
            .content_description(name),
        BoxSpec::default(),
        || {},
    );
}

/// Adds `parts` to a grid's content, naming each item by its index.
fn add_parts(scope: &mut LazyListIntervalContent, parts: &'static [Part], cell: fn(String)) {
    let mut first = 0;
    for &part in parts {
        match part {
            Part::Header(name) => {
                scope.sticky_header(move || cell(name.into()));
                first += 1;
            }
            Part::Items(count) => {
                let start = first;
                scope.items(LazyItems::new(count), move |index| {
                    cell(format!("item {}", start + index));
                });
                first += count;
            }
        }
    }
}

fn row_cell(name: String) {
    tagged(name, CELL_HEIGHT);
}

fn column_cell(name: String) {
    Box(
        Modifier::empty()
            .fill_max_height()
            .width(COLUMN_WIDTH)
            .content_description(name),
        BoxSpec::default(),
        || {},
    );
}

impl Grid {
    fn new(spec: LazyVerticalGridSpec, parts: &'static [Part], direction: LayoutDirection) -> Self {
        Self::composed(direction, move |grid| {
            LazyVerticalGrid(
                Modifier::empty().size_points(WIDTH, HEIGHT),
                grid,
                spec.clone(),
                |scope| add_parts(scope, parts, row_cell),
            );
        })
    }

    fn horizontal(
        spec: LazyHorizontalGridSpec,
        parts: &'static [Part],
        direction: LayoutDirection,
    ) -> Self {
        Self::composed(direction, move |grid| {
            LazyHorizontalGrid(
                Modifier::empty().size_points(WIDTH, HEIGHT),
                grid,
                spec.clone(),
                |scope| add_parts(scope, parts, column_cell),
            );
        })
    }

    fn composed(direction: LayoutDirection, grid: impl Fn(LazyGridState) + 'static) -> Self {
        let state = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&state);
        let grid = Rc::new(grid);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let grid_state = rememberLazyGridState();
                captured.replace(Some(grid_state.clone()));
                let grid = Rc::clone(&grid);
                ProvideLayoutDirection(direction, move || grid(grid_state.clone()));
            },
        );
        shell.set_viewport(WIDTH, 400.0);
        shell.set_buffer_size(WIDTH as u32, 400);
        shell.set_semantics_enabled(true);
        shell.update();
        Self { shell, state }
    }

    fn state(&self) -> LazyGridState {
        self.state.borrow().clone().expect("composed grid")
    }

    fn bounds(&mut self, name: &str) -> Option<Rect> {
        let tree = self.shell.semantics_tree().expect("semantics");
        find_semantics_node(tree.root(), &|node| {
            node.description.as_deref() == Some(name)
        })
        .map(|node| node.bounds)
    }

    fn origin(&mut self, name: &str) -> Option<(f32, f32)> {
        self.bounds(name).map(|bounds| {
            (
                (bounds.x * 100.0).round() / 100.0,
                (bounds.y * 100.0).round() / 100.0,
            )
        })
    }
}

fn width(grid: &mut Grid, name: &str) -> Option<f32> {
    grid.bounds(name).map(|bounds| bounds.width)
}

#[test]
fn fixed_columns_share_the_width_in_whole_pixels_and_lines_stack_with_spacing() {
    let mut grid = Grid::new(
        LazyVerticalGridSpec::new(GridCells::Fixed(3)).spacing(10.0),
        &[Part::Items(10)],
        LayoutDirection::Ltr,
    );
    // 280 pixels for three cells: the pixel left over goes to the first.
    assert_eq!(width(&mut grid, "item 0"), Some(94.0));
    assert_eq!(width(&mut grid, "item 1"), Some(93.0));
    assert_eq!(width(&mut grid, "item 2"), Some(93.0));
    assert_eq!(grid.origin("item 0"), Some((0.0, 0.0)));
    assert_eq!(grid.origin("item 1"), Some((104.0, 0.0)));
    assert_eq!(grid.origin("item 2"), Some((207.0, 0.0)));
    assert_eq!(grid.origin("item 3"), Some((0.0, 50.0)), "the next line");
}

#[test]
fn adaptive_columns_fit_as_many_as_their_minimum_allows() {
    let mut grid = Grid::new(
        LazyVerticalGridSpec::new(GridCells::Adaptive(90.0)),
        &[Part::Items(6)],
        LayoutDirection::Ltr,
    );
    assert_eq!(
        grid.origin("item 2"),
        Some((200.0, 0.0)),
        "three columns of 100"
    );
    assert_eq!(grid.origin("item 3"), Some((0.0, CELL_HEIGHT)));
}

#[test]
fn fixed_size_columns_keep_their_size_and_leave_the_rest_empty() {
    let mut grid = Grid::new(
        LazyVerticalGridSpec::new(GridCells::FixedSize(90.0)).spacing(10.0),
        &[Part::Items(6)],
        LayoutDirection::Ltr,
    );
    assert_eq!(width(&mut grid, "item 0"), Some(90.0));
    assert_eq!(width(&mut grid, "item 2"), Some(90.0));
    assert_eq!(
        grid.origin("item 2"),
        Some((200.0, 0.0)),
        "three columns of 90 and 10 between them fit in 300"
    );
    assert_eq!(grid.origin("item 3"), Some((0.0, 50.0)));

    let mut grid = Grid::new(
        LazyVerticalGridSpec::new(GridCells::FixedSize(400.0)),
        &[Part::Items(2)],
        LayoutDirection::Ltr,
    );
    assert_eq!(
        width(&mut grid, "item 0"),
        Some(WIDTH),
        "a size wider than the grid gives one column of the whole width"
    );
    assert_eq!(grid.origin("item 1"), Some((0.0, CELL_HEIGHT)));
}

#[test]
fn a_header_takes_a_whole_line_and_holds_the_top() {
    let mut grid = Grid::new(
        LazyVerticalGridSpec::new(GridCells::Fixed(3)),
        &[
            Part::Header("header A"),
            Part::Items(7),
            Part::Header("header B"),
            Part::Items(7),
        ],
        LayoutDirection::Ltr,
    );
    assert_eq!(
        grid.bounds("header A").map(|bounds| bounds.width),
        Some(WIDTH)
    );
    assert_eq!(
        grid.origin("item 1"),
        Some((0.0, CELL_HEIGHT)),
        "cells start below it"
    );
    assert_eq!(
        grid.origin("item 7"),
        Some((0.0, 3.0 * CELL_HEIGHT)),
        "the last short line of the section"
    );
    assert_eq!(grid.origin("header B"), Some((0.0, 4.0 * CELL_HEIGHT)));
    grid.state().scroll_to_item(4, 0.0);
    grid.shell.update();
    assert_eq!(
        grid.origin("header A"),
        Some((0.0, 0.0)),
        "the header holds the top"
    );
    assert_eq!(
        grid.origin("item 4"),
        Some((0.0, 0.0)),
        "its line scrolled under it"
    );
}

#[test]
fn scroll_to_item_brings_its_line_to_the_top_and_reports_items() {
    let mut grid = Grid::new(
        LazyVerticalGridSpec::new(GridCells::Fixed(3)),
        &[Part::Items(300)],
        LayoutDirection::Ltr,
    );
    assert_eq!(grid.origin("item 150"), None, "far lines are not composed");
    grid.state().scroll_to_item(151, 0.0);
    grid.shell.update();
    assert_eq!(grid.origin("item 150"), Some((0.0, 0.0)));
    assert_eq!(grid.origin("item 152"), Some((200.0, 0.0)));
    assert_eq!(grid.state().first_visible_item_index(), 150);
    assert_eq!(grid.origin("item 0"), None, "lines far above leave");
}

#[test]
fn a_right_to_left_grid_starts_its_lines_at_the_right() {
    let mut grid = Grid::new(
        LazyVerticalGridSpec::new(GridCells::Fixed(3)),
        &[Part::Items(3)],
        LayoutDirection::Rtl,
    );
    assert_eq!(grid.origin("item 0"), Some((200.0, 0.0)));
    assert_eq!(grid.origin("item 1"), Some((100.0, 0.0)));
    assert_eq!(grid.origin("item 2"), Some((0.0, 0.0)));
}

#[test]
fn keyed_cells_keep_their_state_when_an_insertion_moves_them_to_another_line() {
    use cranpose_core::{MutableState, remember};

    let created = Rc::new(RefCell::new(Vec::<u64>::new()));
    let ids: Rc<RefCell<Option<MutableState<Vec<u64>>>>> = Rc::new(RefCell::new(None));
    let (recorded, captured) = (Rc::clone(&created), Rc::clone(&ids));
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let list = remember(|| cranpose_core::mutableStateOf((1..=5).collect::<Vec<u64>>()))
                .with(|list| *list);
            captured.replace(Some(list));
            let items = list.get();
            let recorded = Rc::clone(&recorded);
            LazyVerticalGrid(
                Modifier::empty().size_points(WIDTH, HEIGHT),
                rememberLazyGridState(),
                LazyVerticalGridSpec::new(GridCells::Fixed(3)),
                move |scope| {
                    let keys = items.clone();
                    scope.items(
                        LazyItems::new(items.len()).key(move |index| keys[index]),
                        move |index| {
                            let id = items[index];
                            let recorded = Rc::clone(&recorded);
                            remember(move || recorded.borrow_mut().push(id));
                            tagged(format!("cell {id}"), CELL_HEIGHT);
                        },
                    );
                },
            );
        },
    );
    shell.set_viewport(WIDTH, 400.0);
    shell.set_buffer_size(WIDTH as u32, 400);
    shell.update();
    assert_eq!(*created.borrow(), [1, 2, 3, 4, 5]);
    let list = ids.borrow().expect("composed list");
    list.set(vec![0, 1, 2, 3, 4, 5]);
    shell.update();
    assert_eq!(
        *created.borrow(),
        [1, 2, 3, 4, 5, 0],
        "cell 3 moved to the second line and kept its composition"
    );
}

#[test]
fn a_horizontal_grid_fills_columns_top_to_bottom_and_mirrors_right_to_left() {
    let parts: &'static [Part] = &[Part::Header("header"), Part::Items(4)];
    let spec = LazyHorizontalGridSpec::new(GridCells::Fixed(2));
    let mut grid = Grid::horizontal(spec.clone(), parts, LayoutDirection::Ltr);
    assert_eq!(
        grid.bounds("header").map(|bounds| bounds.height),
        Some(HEIGHT)
    );
    assert_eq!(grid.origin("item 1"), Some((COLUMN_WIDTH, 0.0)));
    assert_eq!(grid.origin("item 2"), Some((COLUMN_WIDTH, 100.0)));
    assert_eq!(grid.origin("item 3"), Some((2.0 * COLUMN_WIDTH, 0.0)));
    let mut mirrored = Grid::horizontal(spec, parts, LayoutDirection::Rtl);
    assert_eq!(mirrored.origin("header"), Some((WIDTH - COLUMN_WIDTH, 0.0)));
    assert_eq!(
        mirrored.origin("item 1"),
        Some((WIDTH - 2.0 * COLUMN_WIDTH, 0.0)),
        "each cell mirrors by its own width"
    );
    assert_eq!(
        mirrored.origin("item 2"),
        Some((WIDTH - 2.0 * COLUMN_WIDTH, 100.0))
    );
}

/// A grid of three columns with 10 pixels between them, whose first items
/// take the cells `spans` gives, `REST` taking what their line has left.
fn spanned_grid(spans: &'static [usize], after: usize) -> Grid {
    Grid::composed(LayoutDirection::Ltr, move |grid| {
        LazyVerticalGrid(
            Modifier::empty().size_points(WIDTH, HEIGHT),
            grid,
            LazyVerticalGridSpec {
                horizontal_spacing: 10.0,
                ..LazyVerticalGridSpec::new(GridCells::Fixed(3))
            },
            move |scope| {
                scope.items(
                    LazyItems::new(spans.len()).span(|line, index| match spans[index] {
                        REST => line.max_current_line_span,
                        span => span,
                    }),
                    |index| row_cell(format!("item {index}")),
                );
                let first = spans.len();
                scope.items(after, move |index| {
                    row_cell(format!("item {}", first + index));
                });
            },
        );
    })
}

const REST: usize = 0;

#[test]
fn spanned_items_take_their_cells_and_one_too_wide_for_its_line_starts_the_next() {
    let mut grid = spanned_grid(&[usize::MAX, 2, 1, 2, 2, REST], 30);
    let at = |grid: &mut Grid, name: &str| {
        grid.bounds(name)
            .map(|bounds| (bounds.x.round(), bounds.y.round(), bounds.width))
    };
    // 280 pixels of cells: 94, 93 and 93, at 0, 104 and 207.
    assert_eq!(
        at(&mut grid, "item 0"),
        Some((0.0, 0.0, WIDTH)),
        "a whole line"
    );
    assert_eq!(at(&mut grid, "item 1"), Some((0.0, 40.0, 197.0)));
    assert_eq!(at(&mut grid, "item 2"), Some((207.0, 40.0, 93.0)));
    assert_eq!(at(&mut grid, "item 3"), Some((0.0, 80.0, 197.0)));
    assert_eq!(
        at(&mut grid, "item 4"),
        Some((0.0, 120.0, 197.0)),
        "two cells do not fit beside item 3"
    );
    assert_eq!(
        at(&mut grid, "item 5"),
        Some((207.0, 120.0, 93.0)),
        "the cell its line has left"
    );
    assert_eq!(at(&mut grid, "item 6"), Some((0.0, 160.0, 94.0)));
    assert_eq!(
        at(&mut grid, "item 7"),
        Some((104.0, 160.0, 93.0)),
        "one-cell items fill lines after spanned ones"
    );
    grid.state().scroll_to_item(5, 0.0);
    grid.shell.update();
    assert_eq!(grid.state().first_visible_item_index(), 4);
    assert_eq!(at(&mut grid, "item 4"), Some((0.0, 0.0, 197.0)));
    assert_eq!(at(&mut grid, "item 6"), Some((0.0, 40.0, 94.0)));
    assert_eq!(at(&mut grid, "item 9"), Some((0.0, 80.0, 94.0)));
}

#[test]
fn one_cell_items_fill_the_rest_of_a_spanned_line() {
    let mut grid = spanned_grid(&[2], 4);
    assert_eq!(grid.origin("item 1"), Some((207.0, 0.0)));
    assert_eq!(grid.origin("item 2"), Some((0.0, CELL_HEIGHT)));
    assert_eq!(grid.origin("item 4"), Some((207.0, CELL_HEIGHT)));
}

#[test]
fn a_cell_that_a_span_change_moves_to_a_narrower_column_takes_its_width() {
    use cranpose_core::{MutableState, mutableStateOf, remember};

    let span: Rc<RefCell<Option<MutableState<usize>>>> = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&span);
    let mut grid = Grid::composed(LayoutDirection::Ltr, move |grid| {
        let state = remember(|| mutableStateOf(1usize)).with(|state| *state);
        captured.replace(Some(state));
        let first = state.get();
        LazyVerticalGrid(
            Modifier::empty().size_points(WIDTH, HEIGHT),
            grid,
            LazyVerticalGridSpec::new(GridCells::Fixed(3)).spacing(10.0),
            move |scope| {
                scope.items(
                    LazyItems::new(4).span(move |_, index| if index == 0 { first } else { 1 }),
                    |index| row_cell(format!("item {index}")),
                );
            },
        );
    });
    assert_eq!(width(&mut grid, "item 3"), Some(94.0), "the first column");
    span.borrow().expect("composed grid").set(2);
    grid.shell.update();
    assert_eq!(width(&mut grid, "item 0"), Some(197.0));
    assert_eq!(grid.origin("item 3"), Some((104.0, 50.0)));
    assert_eq!(width(&mut grid, "item 3"), Some(93.0), "the second column");
}

#[test]
fn a_one_pixel_wider_grid_gives_the_spare_pixel_to_the_next_column() {
    let mut grid = Grid::composed(LayoutDirection::Ltr, |grid| {
        LazyVerticalGrid(
            Modifier::empty().fill_max_width().height(HEIGHT),
            grid,
            LazyVerticalGridSpec::new(GridCells::Fixed(3)).spacing(10.0),
            |scope| scope.items(6, |index| row_cell(format!("item {index}"))),
        );
    });
    assert_eq!(width(&mut grid, "item 1"), Some(93.0));
    // 281 pixels of cells: the cells are as wide, and two get a pixel more.
    grid.shell.set_viewport(WIDTH + 1.0, 400.0);
    grid.shell.update();
    assert_eq!(width(&mut grid, "item 1"), Some(94.0));
    assert_eq!(width(&mut grid, "item 2"), Some(93.0));
}

/// A grid of three columns of focus targets, the one of `focused` holding
/// `requester`.
fn focus_grid(requester: cranpose_ui::FocusRequester, focused: usize) -> Grid {
    Grid::composed(LayoutDirection::Ltr, move |grid| {
        let requester = requester.clone();
        LazyVerticalGrid(
            Modifier::empty().size_points(WIDTH, HEIGHT),
            grid,
            LazyVerticalGridSpec::new(GridCells::Fixed(3)),
            move |scope| {
                let requester = requester.clone();
                scope.items(300, move |index| {
                    let cell = Modifier::empty()
                        .fill_max_width()
                        .height(CELL_HEIGHT)
                        .content_description(format!("item {index}"));
                    let cell = if index == focused {
                        cell.focus_requester(&requester)
                    } else {
                        cell
                    };
                    Box(cell.focus_target(), BoxSpec::default(), || {});
                });
            },
        );
    })
}

fn focused(grid: &mut Grid) -> Option<String> {
    let tree = grid.shell.semantics_tree().expect("semantics");
    find_semantics_node(tree.root(), &|node| node.focused).and_then(|node| node.description.clone())
}

#[test]
fn tab_moves_the_focus_into_the_line_below_the_viewport_and_reveals_it() {
    let requester = cranpose_ui::FocusRequester::new();
    let mut grid = focus_grid(requester.clone(), 14);
    requester
        .request_focus()
        .expect("focus the last cell in view");
    grid.shell.update();
    assert_eq!(focused(&mut grid).as_deref(), Some("item 14"));
    assert!(grid.shell.on_key_event(&cranpose_ui::KeyEvent::key_down(
        cranpose_ui::Key::Tab,
        "\t"
    )));
    grid.shell.update();
    assert_eq!(focused(&mut grid).as_deref(), Some("item 15"));
    let bounds = grid.bounds("item 15").expect("placed cell");
    assert!(bounds.y + bounds.height <= HEIGHT, "revealed: {bounds:?}");
}

#[test]
fn a_focused_cell_keeps_the_focus_when_the_grid_scrolls_it_far_out_of_view() {
    let requester = cranpose_ui::FocusRequester::new();
    let mut grid = focus_grid(requester.clone(), 4);
    requester.request_focus().expect("focus a cell");
    grid.shell.update();
    grid.state().scroll_to_item(240, 0.0);
    grid.shell.update();
    assert_eq!(grid.state().first_visible_item_index(), 240);
    assert_eq!(focused(&mut grid).as_deref(), Some("item 4"));
}

#[test]
fn a_focused_cell_keeps_the_focus_when_a_scroll_moves_it_just_above_the_viewport() {
    let requester = cranpose_ui::FocusRequester::new();
    let mut grid = focus_grid(requester.clone(), 4);
    requester.request_focus().expect("focus a cell");
    grid.shell.update();
    for _ in 0..4 {
        grid.state().lazy_list_state().dispatch_scroll_delta(-30.0);
        grid.shell.update();
    }
    assert_eq!(grid.state().first_visible_item_index(), 9);
    assert_eq!(focused(&mut grid).as_deref(), Some("item 4"));
}

#[test]
fn a_grid_tells_a_screen_reader_its_lines_and_columns() {
    let collection = |grid: &mut Grid| {
        let tree = grid.shell.semantics_tree().expect("semantics");
        find_semantics_node(tree.root(), &|node| node.details().collection.is_some())
            .and_then(|node| node.details().collection)
    };
    let mut vertical = Grid::new(
        LazyVerticalGridSpec::new(GridCells::Fixed(3)),
        &[Part::Header("header"), Part::Items(10)],
        LayoutDirection::Ltr,
    );
    assert_eq!(
        collection(&mut vertical),
        Some(cranpose_ui::CollectionInfo {
            rows: 5,
            columns: 3
        }),
        "a header line and four lines of cells"
    );
    let mut horizontal = Grid::horizontal(
        LazyHorizontalGridSpec::new(GridCells::Fixed(2)),
        &[Part::Items(5)],
        LayoutDirection::Ltr,
    );
    assert_eq!(
        collection(&mut horizontal),
        Some(cranpose_ui::CollectionInfo {
            rows: 2,
            columns: 3
        })
    );
}
