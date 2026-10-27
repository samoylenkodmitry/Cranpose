use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    AppContext, Column, ColumnSpec, LayoutEngine, LazyColumn, LazyColumnSpec, Modifier, Size, Text,
    TextStyle, composable,
};

const VIEWPORT: Size = Size::new(320.0, 360.0);
const ROW_HEIGHT: f32 = 24.0;
// Each row: the item's host, its column and two texts.
const NODES_PER_ROW: usize = 4;

// Fifty rows of `list`, each a title over a value.
#[composable]
fn Rows(list: LazyListState) {
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(50), move |index| {
                Column(
                    Modifier::empty().height(ROW_HEIGHT),
                    ColumnSpec::default(),
                    move || {
                        Text(
                            format!("Row {index}"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                        Text(
                            format!("{index} kWh"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            });
        },
    );
}

struct Screen {
    context: Rc<AppContext>,
    composition: Composition<MemoryApplier>,
    list: LazyListState,
}

impl Screen {
    fn new() -> Self {
        let context = AppContext::new();
        let mut composition = Composition::new(MemoryApplier::new());
        let captured = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&captured);
        context
            .enter(|| {
                composition.render(location_key(file!(), line!(), column!()), move || {
                    let list = rememberLazyListState();
                    *capture.borrow_mut() = Some(list);
                    Rows(list);
                })
            })
            .expect("initial composition");
        let list = captured
            .borrow()
            .as_ref()
            .copied()
            .expect("lazy list state");
        let mut screen = Self {
            context,
            composition,
            list,
        };
        screen.layout();
        screen
    }

    // Scrolls the list to `index`, `offset`, and lays the frame out.
    fn show(&mut self, index: usize, offset: f32) {
        self.context
            .enter(|| self.list.scroll_to_item(index, offset));
        while self
            .context
            .enter(|| self.composition.process_invalid_scopes())
            .expect("recomposition")
        {}
        self.layout();
    }

    fn layout(&mut self) {
        let composition = &mut self.composition;
        self.context.enter(|| {
            let root = composition.root().expect("lazy list root");
            let runtime = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(runtime);
            applier
                .compute_layout(root, VIEWPORT)
                .expect("lazy list layout");
            applier.clear_runtime_handle();
        });
    }

    fn nodes(&mut self) -> usize {
        self.composition.applier_mut().len()
    }
}

#[test]
fn a_list_that_jumps_back_to_its_start_keeps_no_more_rows_than_its_scroll_takes() {
    let mut screen = Screen::new();
    // A row a frame, as a list scrolls by itself, down to row 30.
    for row in 1..=30 {
        screen.show(row, 0.0);
    }
    let before = screen.nodes();
    screen.show(0, 0.0);
    // On a row a frame again, for more passes than a row waits in the pool.
    for row in 1..=20 {
        screen.show(row, 0.0);
    }
    let after = screen.nodes();
    assert!(
        after <= before + 2 * NODES_PER_ROW,
        "the rows the jump left go, but for what the scroll takes back: {before} nodes before the jump, {after} after"
    );
}
