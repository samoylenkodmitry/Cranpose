//! A lazy vertical grid lays its items out in lines of cells, composes only
//! the lines near its viewport, gives a sticky header a whole line, and
//! scrolls by items, through the public shell.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope};
use cranpose_testing::{robot::TestRenderer, robot_assertions::find_semantics_node};
use cranpose_ui::{
    Box, BoxSpec, GridCells, LayoutDirection, LazyGridState, LazyVerticalGrid,
    LazyVerticalGridSpec, Modifier, ProvideLayoutDirection, Rect, rememberLazyGridState,
};

const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 200.0;
const CELL_HEIGHT: f32 = 40.0;

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

impl Grid {
    fn new(spec: LazyVerticalGridSpec, parts: &'static [Part], direction: LayoutDirection) -> Self {
        let state = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&state);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let grid = rememberLazyGridState();
                captured.replace(Some(grid.clone()));
                let spec = spec.clone();
                ProvideLayoutDirection(direction, move || {
                    LazyVerticalGrid(
                        Modifier::empty().size_points(WIDTH, HEIGHT),
                        grid.clone(),
                        spec.clone(),
                        |scope| {
                            let mut first = 0;
                            for &part in parts {
                                match part {
                                    Part::Header(name) => {
                                        scope.sticky_header(move || {
                                            tagged(name.into(), CELL_HEIGHT);
                                        });
                                        first += 1;
                                    }
                                    Part::Items(count) => {
                                        let start = first;
                                        scope.items(LazyItems::new(count), move |index| {
                                            tagged(format!("item {}", start + index), CELL_HEIGHT);
                                        });
                                        first += count;
                                    }
                                }
                            }
                        },
                    );
                });
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
