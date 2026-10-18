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
