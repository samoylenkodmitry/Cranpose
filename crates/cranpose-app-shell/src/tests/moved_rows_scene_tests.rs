use cranpose_ui::{LayoutNode, TextFieldState};
use cranpose_ui_graphics::{ProjectiveTransform, WindowCoordinates};

use super::*;

const ROW_HEIGHT: f32 = 40.0;
const FIELD_ROW: usize = 2;
const FADING_ROW: usize = 3;

thread_local! {
    static LIST: Cell<Option<LazyListState>> = const { Cell::new(None) };
    static ALPHA: Cell<Option<MutableState<f32>>> = const { Cell::new(None) };
    static FIELD: Cell<Option<NodeId>> = const { Cell::new(None) };
}

/// A lazy column of plain rows, one row holding a text field, which
/// publishes its window origin, and one row whose layer fades.
#[composable]
fn RowsWithAFieldAndAFadingRow() {
    let list_state = rememberLazyListState();
    let alpha = rememberMutableStateOf(|| 1.0_f32);
    LIST.with(|slot| slot.set(Some(list_state)));
    ALPHA.with(|slot| slot.set(Some(alpha)));
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(40, move |index| {
                let row = Modifier::empty().fill_max_width().height(ROW_HEIGHT);
                match index {
                    FIELD_ROW => {
                        let state = cranpose_core::remember(|| TextFieldState::new("field"))
                            .with(|state| *state);
                        let field = cranpose_ui::BasicTextField(state, row, TextStyle::default());
                        FIELD.with(|slot| slot.set(Some(field)));
                    }
                    FADING_ROW => {
                        Box(
                            row.graphics_layer(move || GraphicsLayer {
                                alpha: alpha.get(),
                                ..GraphicsLayer::default()
                            })
                            .background(Color(0.2, 0.4, 0.8, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                    _ => {
                        Box(
                            row.background(Color(0.9, 0.9, 0.9, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                }
            });
        },
    );
}

/// The cell the field publishes its window coordinates into.
fn field_window(
    shell: &mut AppShell<ScopedUpdateCountingRenderer>,
    field: NodeId,
) -> Rc<Cell<WindowCoordinates>> {
    shell
        .app
        .composition
        .applier_mut()
        .with_node::<LayoutNode, _>(field, |node| {
            node.modifier_slices_snapshot().text_window().cloned()
        })
        .ok()
        .flatten()
        .expect("a text field publishes its window coordinates")
}

fn window_origin(window: &Cell<WindowCoordinates>) -> Point {
    window.get().local_to_window.map_point(Point::default())
}

/// Each scroll step moves every row of the list, while one row's layer
/// fades: the list's own update places the moved rows, so the scene draws
/// what a fresh scene draws, and the field's window origin follows its row.
#[test]
fn scrolled_rows_reach_the_scene_through_their_lists_update() {
    let _guard = test_guard();
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let mut shell = AppShell::new_with_size(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::new(RefCell::new(Vec::new())),
        ),
        location_key(file!(), line!(), column!()),
        RowsWithAFieldAndAFadingRow,
        (240, 200),
        (240.0, 200.0),
    );
    shell.update();
    shell.update();
    let list_state = LIST.with(Cell::get).expect("the list composed");
    let alpha = ALPHA.with(Cell::get).expect("the fading row composed");
    let field = FIELD.with(Cell::get).expect("the field row composed");

    // The first step brings a row in at the bottom; the second moves the
    // same rows.
    let mut scrolled = 0.0;
    for (step, (delta, fade)) in [(30.0, 0.5), (5.0, 0.25)].into_iter().enumerate() {
        rebuilds.set(0);
        updates.set(0);
        assert_eq!(list_state.dispatch_scroll_delta(-delta).abs(), delta);
        scrolled += delta;
        alpha.set_value(fade);
        let window = field_window(&mut shell, field);
        window.set(WindowCoordinates {
            size: Size::default(),
            local_to_window: ProjectiveTransform::translation(-500.0, -500.0),
        });
        shell.update();
        assert_eq!(
            (rebuilds.get(), updates.get()),
            (0, 1),
            "step {step} updates the scene in place"
        );

        let scoped_origin = window_origin(&window);
        let (retained, fresh) = retained_and_fresh_painted_rects(&mut shell);
        assert_eq!(
            retained, fresh,
            "step {step} draws the rows where a fresh scene draws them"
        );
        assert!(
            retained.contains(&(
                Rect {
                    x: 0.0,
                    y: FADING_ROW as f32 * ROW_HEIGHT - scrolled,
                    width: 240.0,
                    height: ROW_HEIGHT,
                },
                fade
            )),
            "step {step} fades the moved row: {retained:?}"
        );
        assert_eq!(
            scoped_origin,
            Point {
                x: 0.0,
                y: FIELD_ROW as f32 * ROW_HEIGHT - scrolled,
            },
            "step {step} publishes the moved field's window origin"
        );
        assert_eq!(window_origin(&window), scoped_origin);
    }
}

const KEYED_ROWS: usize = 24;

thread_local! {
    static REVERSED: Cell<Option<MutableState<bool>>> = const { Cell::new(None) };
    static COLUMN_SCROLL: Cell<Option<ScrollState>> = const { Cell::new(None) };
}

/// A scrolling column of keyed rows, each its own shade, in an order a state
/// reverses.
#[composable]
fn KeyedRowsThatReverse() {
    let reversed = rememberMutableStateOf(|| false);
    let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    REVERSED.with(|slot| slot.set(Some(reversed)));
    COLUMN_SCROLL.with(|slot| slot.set(Some(scroll)));
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        move || {
            let flipped = reversed.value();
            for slot in 0..KEYED_ROWS {
                keyed_row(if flipped { KEYED_ROWS - 1 - slot } else { slot });
            }
        },
    );
}

/// The column's rows come back in the reverse order while it scrolls: the
/// scene matches each row with the layer it drew before, past the point
/// where the order is too mixed to match in turn, and draws what a fresh
/// scene draws.
#[test]
fn reordered_rows_of_a_scrolled_column_reach_the_scene_in_their_new_order() {
    let _guard = test_guard();
    let rebuilds = Rc::new(Cell::new(0));
    let mut shell = AppShell::new_with_size(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::new(Cell::new(0)),
            Rc::new(RefCell::new(Vec::new())),
        ),
        location_key(file!(), line!(), column!()),
        KeyedRowsThatReverse,
        (240, 400),
        (240.0, 400.0),
    );
    shell.update();
    shell.update();
    let reversed = REVERSED.with(Cell::get).expect("the column composed");
    let scroll = COLUMN_SCROLL.with(Cell::get).expect("the column composed");

    rebuilds.set(0);
    reversed.set_value(true);
    assert!(scroll.dispatch_raw_delta(30.0) > 0.0);
    shell.update();

    let (retained, fresh) = retained_and_fresh_painted_rects(&mut shell);
    assert_eq!(rebuilds.get(), 0, "the frame updates the scene in place");
    assert_eq!(
        retained, fresh,
        "the scene draws the rows where a fresh scene draws them"
    );
    assert_eq!(
        retained.first().map(|(rect, _)| rect.y),
        Some(-30.0),
        "the first row drawn is the last one, scrolled"
    );
}

/// Where the column of [`KeyedRowsWithAMiddle`] differs in the middle.
#[derive(Clone, Copy, PartialEq)]
enum Middle {
    AsComposed,
    RowAdded,
    RowRemoved,
}

const MIDDLE_ROW: usize = KEYED_ROWS / 2;

thread_local! {
    static MIDDLE: Cell<Option<MutableState<Middle>>> = const { Cell::new(None) };
}

/// A scrolling column of keyed rows, each its own layer, where a state adds a
/// row in the middle or removes the middle row.
#[composable]
fn KeyedRowsWithAMiddle() {
    let middle = rememberMutableStateOf(|| Middle::AsComposed);
    let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    MIDDLE.with(|slot| slot.set(Some(middle)));
    COLUMN_SCROLL.with(|slot| slot.set(Some(scroll)));
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        move || {
            let middle = middle.value();
            let rows =
                (0..KEYED_ROWS).filter(|&row| row != MIDDLE_ROW || middle != Middle::RowRemoved);
            for row in rows {
                if row == MIDDLE_ROW && middle == Middle::RowAdded {
                    keyed_row(KEYED_ROWS);
                }
                keyed_row(row);
            }
        },
    );
}

/// A row of its own shade and layer, keyed by `row`.
fn keyed_row(row: usize) {
    cranpose_core::with_key(&row, || {
        let shade = row as f32 / (KEYED_ROWS + 1) as f32;
        Box(
            Modifier::empty()
                .fill_max_width()
                .height(ROW_HEIGHT)
                .graphics_layer(GraphicsLayer::default)
                .background(Color(shade, 0.5, 1.0 - shade, 1.0)),
            BoxSpec::default(),
            || {},
        );
    });
}

/// A row enters between the kept rows of a scrolling column, then two rows
/// leave from between them: the kept rows keep their order with a gap, and
/// the scene draws what a fresh scene draws.
#[test]
fn rows_that_enter_and_leave_between_kept_rows_reach_the_scene() {
    let _guard = test_guard();
    let rebuilds = Rc::new(Cell::new(0));
    let mut shell = AppShell::new_with_size(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::new(Cell::new(0)),
            Rc::new(RefCell::new(Vec::new())),
        ),
        location_key(file!(), line!(), column!()),
        KeyedRowsWithAMiddle,
        (240, 400),
        (240.0, 400.0),
    );
    shell.update();
    shell.update();
    let middle = MIDDLE.with(Cell::get).expect("the column composed");
    let scroll = COLUMN_SCROLL.with(Cell::get).expect("the column composed");

    for (step, change) in [Middle::RowAdded, Middle::RowRemoved]
        .into_iter()
        .enumerate()
    {
        rebuilds.set(0);
        middle.set_value(change);
        assert!(scroll.dispatch_raw_delta(20.0) > 0.0);
        shell.update();

        let (retained, fresh) = retained_and_fresh_painted_rects(&mut shell);
        assert_eq!(rebuilds.get(), 0, "step {step} updates the scene in place");
        assert_eq!(
            retained, fresh,
            "step {step} draws the rows where a fresh scene draws them"
        );
    }
}
