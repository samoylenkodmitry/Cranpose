use std::cell::RefCell;

use cranpose_app_shell::{AppShell, RootId};
use cranpose_core::{location_key, movable};
use cranpose_testing::TestRenderer;
use cranpose_ui::{Box, BoxSpec, LayoutBox, Modifier, Size};

use super::*;

const STRIP: Size = Size {
    width: 80.0,
    height: 36.0,
};
const BODY: Size = Size {
    width: 90.0,
    height: 30.0,
};
const WINDOW: Size = Size {
    width: 200.0,
    height: 100.0,
};

#[composable]
#[allow(non_snake_case)]
fn PageBody(page: u64) {
    let _ = page;
    Box(Modifier::empty().size(BODY), BoxSpec::default(), || {});
}

#[composable]
#[allow(non_snake_case)]
fn TestChrome(view: WindowView) {
    let active = view.active();
    cranpose_ui::Column(
        Modifier::empty(),
        cranpose_ui::ColumnSpec::default(),
        move || {
            Box(Modifier::empty().size(STRIP), BoxSpec::default(), || {});
            movable(("page", active), move || PageBody(active));
        },
    );
}

type Placement = Option<(f32, f32)>;

fn find_box_sized(layout: &LayoutBox, size: Size) -> Placement {
    if layout.rect.width == size.width && layout.rect.height == size.height {
        return Some((layout.rect.x, layout.rect.y));
    }
    layout
        .children
        .iter()
        .find_map(|child| find_box_sized(child, size))
}

fn window_root_at(shell: &AppShell<TestRenderer>, index: usize) -> u64 {
    shell
        .window_roots()
        .get(index)
        .map(|entry| entry.node as u64)
        .expect("a window root attached")
}

fn strip_and_body_in(shell: &mut AppShell<TestRenderer>, index: usize) -> (Placement, Placement) {
    shell.update();
    shell.update();
    let root = window_root_at(shell, index);
    let mut surface = shell.surface(RootId::Window(root)).expect("window surface");
    surface.with_layout_tree(|tree| {
        let root = tree.expect("window layout").root();
        (find_box_sized(root, STRIP), find_box_sized(root, BODY))
    })
}

#[test]
fn a_page_torn_into_a_new_window_lays_out_below_that_windows_strip() {
    let captured: Rc<RefCell<Option<Windows>>> = Rc::new(RefCell::new(None));
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let captured = Rc::clone(&captured);
            move || {
                let captured = Rc::clone(&captured);
                TornWindowsHost(
                    "test-tabs",
                    Rules::tabs("t", WINDOW, STRIP.height),
                    vec![(1, WINDOW), (2, WINDOW)],
                    move |view| {
                        *captured.borrow_mut() = Some(view.windows().clone());
                        TestChrome(view.clone());
                    },
                );
            }
        },
    );
    shell.update();
    shell.add_window_surface(
        window_root_at(&shell, 0),
        TestRenderer::default(),
        (200, 100),
        (200.0, 100.0),
    );
    shell.update();
    let windows = (*captured.borrow()).clone().expect("chrome ran");
    windows.press(1, Point::new(40.0, 15.0), Some(Point::new(240.0, 175.0)));
    windows.drag_step(1, Point::new(40.0, 80.0), Some(Point::new(240.0, 240.0)));
    let model = windows.model().get_non_reactive();
    assert_eq!(
        model.windows().len(),
        2,
        "the tab tore into a second window"
    );
    assert_eq!(model.window_of(1), Some(2));

    shell.update();
    shell.add_window_surface(
        window_root_at(&shell, 1),
        TestRenderer::default(),
        (200, 100),
        (200.0, 100.0),
    );
    let (strip, body) = strip_and_body_in(&mut shell, 1);
    assert_eq!(strip, Some((0.0, 0.0)), "the strip is at the top");
    assert_eq!(body, Some((0.0, 36.0)), "the page lays out below the strip");

    windows.release();
    let state = *windows
        .shared
        .borrow()
        .states
        .get(&2)
        .expect("the second window remembered its state");
    state.set_size(Size::new(WINDOW.width + 10.0, WINDOW.height));
    let (strip, body) = strip_and_body_in(&mut shell, 1);
    assert_eq!(strip, Some((0.0, 0.0)), "the strip stays at the top");
    assert_eq!(
        body,
        Some((0.0, 36.0)),
        "a window recomposed with its chrome skipped keeps the page below the strip"
    );
}
