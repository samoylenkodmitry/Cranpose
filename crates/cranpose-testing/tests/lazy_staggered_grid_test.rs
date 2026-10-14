//! A lazy staggered grid puts each item into the lane that ends first, lets
//! an item take the whole line, composes only the items near its viewport,
//! and stops at the end of its longest lane, through the public shell.

use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListIntervalContent, LazyListScope};
use cranpose_testing::{robot::TestRenderer, robot_assertions::find_semantics_node};
use cranpose_ui::{
    Box, BoxSpec, GridCells, LayoutDirection, LazyHorizontalStaggeredGrid,
    LazyHorizontalStaggeredGridSpec, LazyStaggeredGridState, LazyVerticalStaggeredGrid,
    LazyVerticalStaggeredGridSpec, Modifier, ProvideLayoutDirection, Rect,
    rememberLazyStaggeredGridState,
};

const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 200.0;

struct Grid {
    shell: AppShell<TestRenderer>,
    state: Rc<RefCell<Option<LazyStaggeredGridState>>>,
    /// The items that composed.
    composed: Rc<RefCell<BTreeSet<usize>>>,
}

/// An item `length` long along the grid's axis that names itself by its
/// index and notes that it composed.
fn item(index: usize, length: f32, vertical: bool, composed: &RefCell<BTreeSet<usize>>) {
    composed.borrow_mut().insert(index);
    let modifier = if vertical {
        Modifier::empty().fill_max_width().height(length)
    } else {
        Modifier::empty().fill_max_height().width(length)
    };
    Box(
        modifier.content_description(format!("item {index}")),
        BoxSpec::default(),
        || {},
    );
}

/// `lengths` items, the ones in `full` across the whole line.
fn add_items(
    scope: &mut LazyListIntervalContent,
    lengths: &'static [f32],
    full: &'static [usize],
    vertical: bool,
    composed: &Rc<RefCell<BTreeSet<usize>>>,
) {
    let composed = Rc::clone(composed);
    scope.items(
        LazyItems::new(lengths.len()).span(move |line, index| {
            if full.contains(&index) {
                line.max_line_span
            } else {
                1
            }
        }),
        move |index| item(index, lengths[index], vertical, &composed),
    );
}

impl Grid {
    fn vertical(
        spec: LazyVerticalStaggeredGridSpec,
        lengths: &'static [f32],
        full: &'static [usize],
        direction: LayoutDirection,
    ) -> Self {
        Self::composed(direction, move |grid, composed| {
            LazyVerticalStaggeredGrid(
                Modifier::empty().size_points(WIDTH, HEIGHT),
                grid,
                spec.clone(),
                |scope| add_items(scope, lengths, full, true, composed),
            );
        })
    }

    fn composed(
        direction: LayoutDirection,
        grid: impl Fn(LazyStaggeredGridState, &Rc<RefCell<BTreeSet<usize>>>) + 'static,
    ) -> Self {
        let state = Rc::new(RefCell::new(None));
        let composed = Rc::new(RefCell::new(BTreeSet::new()));
        let (captured, noted) = (Rc::clone(&state), Rc::clone(&composed));
        let grid = Rc::new(grid);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let grid_state = rememberLazyStaggeredGridState();
                captured.replace(Some(grid_state.clone()));
                let (grid, noted) = (Rc::clone(&grid), Rc::clone(&noted));
                ProvideLayoutDirection(direction, move || grid(grid_state.clone(), &noted));
            },
        );
        shell.set_viewport(WIDTH, 400.0);
        shell.set_buffer_size(WIDTH as u32, 400);
        shell.set_semantics_enabled(true);
        shell.update();
        Self {
            shell,
            state,
            composed,
        }
    }

    fn state(&self) -> LazyStaggeredGridState {
        self.state.borrow().clone().expect("composed grid")
    }

    fn bounds(&mut self, name: &str) -> Option<Rect> {
        let tree = self.shell.semantics_tree().expect("semantics");
        find_semantics_node(tree.root(), &|node| {
            node.description.as_deref() == Some(name)
        })
        .map(|node| node.bounds)
    }

    /// Where item `index` sits and how wide it is, to the hundredth.
    fn at(&mut self, index: usize) -> Option<(f32, f32, f32)> {
        let round = |value: f32| (value * 100.0).round() / 100.0;
        self.bounds(&format!("item {index}"))
            .map(|bounds| (round(bounds.x), round(bounds.y), round(bounds.width)))
    }

    fn scroll_by(&mut self, distance: f32) {
        self.state()
            .lazy_list_state()
            .dispatch_scroll_delta(-distance);
        self.shell.update();
    }
}

/// Lengths of 40 to 120 that repeat every five items.
static VARIED: [f32; 300] = {
    let mut lengths = [0.0; 300];
    let mut index = 0;
    while index < 300 {
        lengths[index] = 40.0 + (index * 3 % 5) as f32 * 20.0;
        index += 1;
    }
    lengths
};

#[test]
fn each_item_takes_the_lane_that_ends_first() {
    let mut grid = Grid::vertical(
        LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(2)),
        &[100.0, 50.0, 50.0, 80.0, 30.0],
        &[],
        LayoutDirection::Ltr,
    );
    assert_eq!(grid.at(0), Some((0.0, 0.0, 150.0)));
    assert_eq!(grid.at(1), Some((150.0, 0.0, 150.0)));
    assert_eq!(grid.at(2), Some((150.0, 50.0, 150.0)), "the shorter lane");
    assert_eq!(
        grid.at(3),
        Some((0.0, 100.0, 150.0)),
        "a tie goes to the first"
    );
    assert_eq!(grid.at(4), Some((150.0, 100.0, 150.0)));
}

#[test]
fn an_item_across_the_whole_line_starts_below_every_lane() {
    let mut grid = Grid::vertical(
        LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(2)).spacing(10.0),
        &[100.0, 40.0, 30.0, 20.0, 20.0],
        &[2],
        LayoutDirection::Ltr,
    );
    assert_eq!(grid.at(1), Some((155.0, 0.0, 145.0)));
    assert_eq!(grid.at(2), Some((0.0, 110.0, WIDTH)), "the whole line");
    assert_eq!(grid.at(3), Some((0.0, 150.0, 145.0)));
    assert_eq!(grid.at(4), Some((155.0, 150.0, 145.0)));
}

#[test]
fn a_jump_far_ahead_composes_only_the_items_near_it_and_scrolls_back_to_the_start() {
    let mut grid = Grid::vertical(
        LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(3)),
        &VARIED,
        &[],
        LayoutDirection::Ltr,
    );
    grid.composed.borrow_mut().clear();
    grid.state().scroll_to_item(200, 0.0);
    grid.shell.update();
    assert_eq!(grid.at(200).map(|(_, y, _)| y), Some(0.0), "at the top");
    let composed = grid.composed.borrow().clone();
    assert!(
        composed.iter().all(|&index| (180..240).contains(&index)),
        "composed {composed:?}"
    );
    for _ in 0..80 {
        grid.scroll_by(-200.0);
    }
    assert_eq!(grid.state().first_visible_item_index(), 0);
    assert_eq!(grid.at(0), Some((0.0, 0.0, 100.0)));
    assert_eq!(grid.at(1).map(|(x, y, _)| (x, y)), Some((100.0, 0.0)));
}

#[test]
fn the_grid_stops_where_its_longest_lane_ends() {
    let mut grid = Grid::vertical(
        LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(2)),
        &[100.0, 50.0, 300.0, 50.0, 50.0],
        &[],
        LayoutDirection::Ltr,
    );
    grid.scroll_by(1_000.0);
    // Lane one holds items 1 and 2 and ends at 350, 150 past the viewport.
    assert_eq!(grid.at(2).map(|(_, y, _)| y), Some(-100.0));
    assert!(!grid.state().lazy_list_state().can_scroll_forward());
    grid.scroll_by(-1_000.0);
    assert_eq!(grid.at(0).map(|(_, y, _)| y), Some(0.0));
}

#[test]
fn a_right_to_left_grid_starts_its_lanes_at_the_right() {
    let mut grid = Grid::vertical(
        LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(3)),
        &[60.0, 40.0, 50.0, 20.0],
        &[],
        LayoutDirection::Rtl,
    );
    assert_eq!(grid.at(0), Some((200.0, 0.0, 100.0)));
    assert_eq!(grid.at(2), Some((0.0, 0.0, 100.0)));
    assert_eq!(grid.at(3), Some((100.0, 40.0, 100.0)), "under item 1");
}

#[test]
fn a_horizontal_grid_fills_rows_of_their_own_widths() {
    let composed = Rc::new(RefCell::new(BTreeSet::new()));
    let noted = Rc::clone(&composed);
    let mut grid = Grid::composed(LayoutDirection::Ltr, move |grid, _| {
        let noted = Rc::clone(&noted);
        LazyHorizontalStaggeredGrid(
            Modifier::empty().size_points(WIDTH, HEIGHT),
            grid,
            LazyHorizontalStaggeredGridSpec::new(GridCells::Fixed(2)),
            move |scope| add_items(scope, &[120.0, 60.0, 60.0], &[], false, &noted),
        );
    });
    let at = |grid: &mut Grid, index: usize| {
        grid.bounds(&format!("item {index}"))
            .map(|bounds| (bounds.x, bounds.y, bounds.height))
    };
    assert_eq!(at(&mut grid, 0), Some((0.0, 0.0, 100.0)));
    assert_eq!(at(&mut grid, 1), Some((0.0, 100.0, 100.0)));
    assert_eq!(
        at(&mut grid, 2),
        Some((60.0, 100.0, 100.0)),
        "after the shorter row's item"
    );
}

/// The items in view, by index, at their places.
fn shown(grid: &mut Grid) -> Vec<(usize, f32, f32)> {
    let tree = grid.shell.semantics_tree().expect("semantics");
    let mut shown = Vec::new();
    let mut nodes = vec![tree.root()];
    while let Some(node) = nodes.pop() {
        if let Some(index) = node
            .description
            .as_deref()
            .and_then(|name| name.strip_prefix("item "))
            .and_then(|index| index.parse().ok())
            && node.bounds.y + node.bounds.height > 0.0
            && node.bounds.y < HEIGHT
        {
            shown.push((index, node.bounds.x, node.bounds.y));
        }
        nodes.extend(node.children.iter());
    }
    shown.sort_by_key(|&(index, _, _)| index);
    shown
}

#[test]
fn items_in_view_move_with_the_scroll_while_the_ones_above_are_measured() {
    let mut grid = Grid::vertical(
        LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(3)),
        &VARIED,
        &[],
        LayoutDirection::Ltr,
    );
    grid.state().scroll_to_item(150, 0.0);
    grid.shell.update();
    for step in [-30.0, 30.0] {
        for _ in 0..40 {
            let before = shown(&mut grid);
            grid.scroll_by(step);
            for (index, x, y) in shown(&mut grid) {
                if let Some(&(_, was_x, was_y)) = before.iter().find(|item| item.0 == index) {
                    assert_eq!(x, was_x, "item {index} changed lanes");
                    assert!(
                        (y - (was_y - step)).abs() < 0.01,
                        "item {index} went from {was_y} to {y} on a {step} scroll"
                    );
                }
            }
        }
    }
}
