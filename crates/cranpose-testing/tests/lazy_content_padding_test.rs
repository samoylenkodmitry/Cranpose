//! A lazy column's scroll position counts from the edge of its start
//! padding, as Compose's does: `scroll_to_item(k, 0)` puts item `k` there,
//! the items before it show in the padding, and an item that lies wholly in
//! the padding is not the first visible one.

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_testing::{robot::TestRenderer, robot_assertions::find_semantics_node};
use cranpose_ui::{Box, BoxSpec, LazyColumn, LazyColumnSpec, Modifier, PaddingValues};

const ROW: f32 = 20.0;
const TOP_PADDING: f32 = 30.0;

struct List {
    shell: AppShell<TestRenderer>,
    state: Rc<Cell<Option<LazyListState>>>,
}

impl List {
    /// A column 200 × 100 of 20 rows 20 high, with 30 of padding at the top
    /// and 10 at the bottom.
    fn new() -> Self {
        let state = Rc::new(Cell::new(None));
        let captured = Rc::clone(&state);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let list_state = rememberLazyListState();
                captured.set(Some(list_state));
                LazyColumn(
                    Modifier::empty().size_points(200.0, 100.0),
                    list_state,
                    LazyColumnSpec::new().content_padding(PaddingValues::new(
                        0.0,
                        TOP_PADDING,
                        0.0,
                        10.0,
                    )),
                    |scope| {
                        scope.items(LazyItems::new(20), |index| {
                            Box(
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(ROW)
                                    .content_description(format!("row{index}")),
                                BoxSpec::default(),
                                || {},
                            );
                        });
                    },
                );
            },
        );
        shell.set_viewport(200.0, 100.0);
        shell.set_buffer_size(200, 100);
        shell.set_semantics_enabled(true);
        shell.update();
        Self { shell, state }
    }

    fn state(&self) -> LazyListState {
        self.state.get().expect("composed list")
    }

    fn scroll_to(&mut self, index: usize, offset: f32) {
        self.state().scroll_to_item(index, offset);
        self.shell.update();
    }

    fn scroll_by(&mut self, delta: f32) {
        self.state().dispatch_scroll_delta(delta);
        self.shell.update();
    }

    fn first_visible(&self) -> (usize, f32) {
        let state = self.state();
        (
            state.first_visible_item_index(),
            state.first_visible_item_scroll_offset(),
        )
    }

    /// The tops of the rows numbered `rows`, `None` for one not placed.
    fn tops<const N: usize>(&mut self, rows: [usize; N]) -> [Option<f32>; N] {
        let tree = self.shell.semantics_tree().expect("semantics");
        rows.map(|row| {
            let name = format!("row{row}");
            find_semantics_node(tree.root(), &|node| {
                node.description.as_deref() == Some(name.as_str())
            })
            .map(|node| node.bounds.y)
        })
    }
}

#[test]
fn scroll_to_item_puts_the_item_at_the_edge_of_the_start_padding() {
    let mut list = List::new();
    assert_eq!(list.tops([0, 1]), [Some(30.0), Some(50.0)]);

    list.scroll_to(3, 0.0);
    assert_eq!(
        list.tops([3, 2, 1]),
        [Some(30.0), Some(10.0), Some(-10.0)],
        "row 3 starts after the padding, and the rows before it show in the padding"
    );
    assert_eq!(list.first_visible(), (3, 0.0));

    list.scroll_to(3, 5.0);
    assert_eq!(list.tops([3]), [Some(25.0)]);
    assert_eq!(list.first_visible(), (3, 5.0));
}

#[test]
fn a_row_in_the_start_padding_is_not_the_first_visible() {
    let mut list = List::new();
    list.scroll_by(-50.0);
    assert_eq!(
        list.tops([1, 2]),
        [Some(0.0), Some(20.0)],
        "row 1 lies in the padding"
    );
    assert_eq!(list.first_visible(), (2, 10.0));

    list.scroll_by(30.0);
    assert_eq!(list.tops([1, 0]), [Some(30.0), Some(10.0)]);
    assert_eq!(list.first_visible(), (1, 0.0));

    list.scroll_by(100.0);
    assert_eq!(list.tops([0]), [Some(30.0)], "the list stops at its start");
    assert_eq!(list.first_visible(), (0, 0.0));
}

#[test]
fn the_end_of_the_list_rests_above_the_end_padding() {
    let mut list = List::new();
    list.scroll_to(19, 0.0);
    assert_eq!(
        list.tops([19, 16]),
        [Some(70.0), Some(10.0)],
        "the last row ends at the end padding"
    );
    assert_eq!(list.first_visible(), (17, 0.0));
}
