//! Saved scroll positions: a list, a staggered grid and a pager that another
//! screen covered come back where they were when it is popped; a grid comes
//! back to its first visible item with another column count, and a
//! scrolling column to its offset; and a position survives a restart
//! through the platform preferences.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

mod support;

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_navigation::{
    NavController, NavHost, rememberNavController, rememberSaveableLazyGridState,
    rememberSaveableLazyListState, rememberSaveableLazyStaggeredGridState,
    rememberSaveablePagerState, rememberSaveableScrollState,
};
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, GridCells, LazyGridState, LazyItems, LazyListScope,
    LazyListState, LazyStaggeredGridState, LazyVerticalGrid, LazyVerticalGridSpec, Modifier,
    PagerState, ScrollState, Size,
};
use support::{Host, Screen, fresh_store, save, serial};

/// The states the home screen composed last.
#[derive(Clone, Default)]
struct Home {
    list: Rc<Cell<Option<LazyListState>>>,
    staggered: Rc<RefCell<Option<LazyStaggeredGridState>>>,
    pager: Rc<Cell<Option<PagerState>>>,
}

impl Home {
    fn list(&self) -> LazyListState {
        self.list.get().expect("home composed")
    }

    fn staggered(&self) -> LazyStaggeredGridState {
        self.staggered.borrow().clone().expect("home composed")
    }

    fn pager(&self) -> PagerState {
        self.pager.get().expect("home composed")
    }
}

fn app(home: &Home) -> (Host, Rc<Cell<Option<NavController<Screen>>>>) {
    let nav = Rc::new(Cell::new(None));
    let (captured, screens) = (Rc::clone(&nav), home.clone());
    let host = Host::new(move || {
        let controller = rememberNavController(Screen::Home);
        captured.set(Some(controller));
        let screens = screens.clone();
        NavHost(controller, move |screen| {
            if screen == Screen::Home {
                screens
                    .list
                    .set(Some(rememberSaveableLazyListState("list")));
                *screens.staggered.borrow_mut() =
                    Some(rememberSaveableLazyStaggeredGridState("staggered"));
                screens
                    .pager
                    .set(Some(rememberSaveablePagerState("pager", 0)));
            }
        });
    });
    (host, nav)
}

#[test]
fn scroll_positions_come_back_when_the_covering_screen_is_popped() {
    let _serial = serial();
    fresh_store();
    let home = Home::default();
    let (mut host, nav) = app(&home);
    host.settle();
    home.list().scroll_to_item(30, 12.0);
    home.staggered().scroll_to_item(7, 3.0);
    home.pager().scroll_to_page(4);
    host.settle();

    let nav = nav.get().expect("composed");
    nav.navigate(Screen::Detail(1));
    host.settle();
    assert!(nav.pop_back_stack());
    host.settle();

    let list = home.list();
    assert_eq!(
        (
            list.first_visible_item_index(),
            list.first_visible_item_scroll_offset()
        ),
        (30, 12.0)
    );
    assert_eq!(home.staggered().first_visible_item_index(), 7);
    assert_eq!(home.pager().lazy_list_state().first_visible_item_index(), 4);
}

/// A list at the root, whose scope saves in the platform preferences.
fn root_list(list: &Rc<Cell<Option<LazyListState>>>) -> Host {
    let captured = Rc::clone(list);
    let mut host = Host::new(move || {
        captured.set(Some(rememberSaveableLazyListState("list")));
    });
    host.settle();
    host
}

#[test]
fn a_position_survives_a_restart() {
    let _serial = serial();
    fresh_store();
    let list: Rc<Cell<Option<LazyListState>>> = Rc::default();
    {
        let mut host = root_list(&list);
        list.get().expect("composed").scroll_to_item(18, 4.0);
        host.settle();
        save();
    }
    let restarted: Rc<Cell<Option<LazyListState>>> = Rc::default();
    let _host = root_list(&restarted);
    let list = restarted.get().expect("composed");
    assert_eq!(
        (
            list.first_visible_item_index(),
            list.first_visible_item_scroll_offset()
        ),
        (18, 4.0)
    );
}

/// Whether the grid shows, and how many columns it has.
type Controls = (MutableState<bool>, MutableState<usize>);

/// A scrolling column and a grid that a flag shows, the grid with a column
/// count another state picks.
#[test]
fn a_grid_and_a_column_come_back_where_they_were() {
    let _serial = serial();
    fresh_store();
    let grid: Rc<RefCell<Option<LazyGridState>>> = Rc::default();
    let column: Rc<Cell<Option<ScrollState>>> = Rc::default();
    let controls: Rc<Cell<Option<Controls>>> = Rc::default();
    let (captured, scrolled, installed) =
        (Rc::clone(&grid), Rc::clone(&column), Rc::clone(&controls));
    let mut robot = create_headless_robot_test(300, 400, move || {
        let shown = rememberMutableStateOf(|| true);
        let columns = rememberMutableStateOf(|| 2_usize);
        installed.set(Some((shown, columns)));
        if !shown.get() {
            return;
        }
        let scroll = rememberSaveableScrollState("column");
        scrolled.set(Some(scroll));
        Column(
            Modifier::empty()
                .size(Size::new(300.0, 100.0))
                .vertical_scroll(scroll, false),
            ColumnSpec::default(),
            || {
                Box(
                    Modifier::empty().size(Size::new(300.0, 1000.0)),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
        let state = rememberSaveableLazyGridState("grid");
        *captured.borrow_mut() = Some(state.clone());
        LazyVerticalGrid(
            Modifier::empty().size(Size::new(300.0, 300.0)),
            state,
            LazyVerticalGridSpec::new(GridCells::Fixed(columns.get())),
            |scope| {
                scope.items(LazyItems::new(200), |_| {
                    Box(Modifier::empty().height(40.0), BoxSpec::default(), || {});
                });
            },
        );
    });
    robot.wait_for_idle();
    let composed = || grid.borrow().clone().expect("composed");
    composed().scroll_to_item(20, 0.0);
    column.get().expect("composed").scroll_to(250.0);
    robot.wait_for_idle();
    let (shown, columns) = controls.get().expect("composed");
    shown.set(false);
    robot.wait_for_idle();
    columns.set(3);
    shown.set(true);
    robot.wait_for_idle();
    let first = composed().first_visible_item_index();
    assert!(
        (first..first + 3).contains(&20),
        "the first line holds item 20: it starts at {first}"
    );
    assert_eq!(column.get().expect("composed").value(), 250.0);
}
