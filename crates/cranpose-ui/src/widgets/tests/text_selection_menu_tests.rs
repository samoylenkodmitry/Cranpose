use cranpose_foundation::PointerEvent;
use cranpose_ui_graphics::Point;

use super::*;
use crate::modifier::{ModifierNodeSlices, collect_slices_from_modifier};

fn button_handler(modifier: &Modifier) -> (Rc<dyn Fn(PointerEvent)>, ModifierNodeSlices) {
    let slices = collect_slices_from_modifier(modifier);
    assert_eq!(
        slices.pointer_inputs().len(),
        1,
        "menu button must install exactly one pointer-input gesture"
    );
    let handler = slices.pointer_inputs()[0].clone();
    (handler, slices)
}

fn down(x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(PointerEventKind::Down, Point { x, y }, Point { x, y })
}
fn up(x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(PointerEventKind::Up, Point { x, y }, Point { x, y })
}

#[test]
fn menu_button_consumes_the_tap_and_runs_the_action() {
    let _app_context = crate::render_state::app_context_test_scope();
    let ran = Rc::new(Cell::new(false));
    let action: Rc<dyn Fn()> = {
        let ran = Rc::clone(&ran);
        Rc::new(move || ran.set(true))
    };
    let modifier = menu_item_pointer_input("Copy", action, Rc::new(Cell::new(false)));
    let (handler, _slices) = button_handler(&modifier);

    let press = down(5.0, 5.0);
    handler(press.clone());
    assert!(
        press.is_consumed(),
        "the press must be consumed so it never reaches the field and collapses the selection"
    );
    assert!(!ran.get(), "the action fires on release, not on press");

    let release = up(6.0, 6.0);
    handler(release.clone());
    assert!(release.is_consumed(), "the release must be consumed too");
    assert!(
        ran.get(),
        "releasing after a press on the button runs the action"
    );
}

#[test]
fn menu_button_release_without_press_is_consumed_but_inert() {
    let _app_context = crate::render_state::app_context_test_scope();
    let ran = Rc::new(Cell::new(false));
    let action: Rc<dyn Fn()> = {
        let ran = Rc::clone(&ran);
        Rc::new(move || ran.set(true))
    };
    let modifier = menu_item_pointer_input("Cut", action, Rc::new(Cell::new(false)));
    let (handler, _slices) = button_handler(&modifier);

    let release = up(5.0, 5.0);
    handler(release.clone());
    assert!(
        release.is_consumed(),
        "a release on the menu is consumed so it never hits the field"
    );
    assert!(!ran.get(), "a release with no matching press must not act");
}

#[test]
fn menu_button_accepts_release_from_a_continuous_press() {
    let _app_context = crate::render_state::app_context_test_scope();
    let ran = Rc::new(Cell::new(false));
    let action: Rc<dyn Fn()> = {
        let ran = Rc::clone(&ran);
        Rc::new(move || ran.set(true))
    };
    let modifier = menu_item_pointer_input("Copy", action, Rc::new(Cell::new(true)));
    let (handler, _slices) = button_handler(&modifier);

    let release = up(5.0, 5.0);
    handler(release.clone());

    assert!(release.is_consumed());
    assert!(ran.get());
}

#[test]
fn menu_geometry_matches_the_reference() {
    assert_eq!(MENU_HEIGHT, 44.0);
    assert_eq!(ITEM_PADDING, 20.0);
    assert_eq!(SEPARATOR_WIDTH, 1.0);
    assert_eq!(SEPARATOR_HEIGHT, 17.0);
    assert_eq!(MENU_GAP_ABOVE_LINE, 15.0);
    assert_eq!(MENU_SCREEN_MARGIN, 20.0);
}

#[test]
fn pagination_reserves_the_disc_only_when_overflowing() {
    let _app_context = crate::render_state::app_context_test_scope();
    let style = menu_text_style(false);
    let items: Vec<TextMenuItem> = ["Copy", "Cut", "Paste", "Select all"]
        .iter()
        .map(|label| TextMenuItem::new(*label, || {}))
        .collect();

    let one = paginate(&items, &style, f32::INFINITY);
    assert_eq!(one.len(), 1, "everything fits on one page");
    assert_eq!(one[0].len(), 4);

    let total: f32 = items
        .iter()
        .map(|i| item_width(&i.label, &style))
        .sum::<f32>()
        + 3.0 * SEPARATOR_WIDTH;
    let narrow = paginate(&items, &style, total * 0.55);
    assert!(narrow.len() > 1, "a narrow window must page");
    assert!(narrow.iter().all(|page| !page.is_empty()));
    let all: Vec<usize> = narrow.iter().flatten().copied().collect();
    assert_eq!(all, vec![0, 1, 2, 3], "pages cover every item in order");
}

#[test]
fn a_menu_sits_above_its_line_unless_the_window_top_is_in_the_way() {
    let high = MenuAnchor {
        center_x: 100.0,
        line_top: 200.0,
        line_bottom: 220.0,
    };
    assert_eq!(
        high.menu_top(),
        200.0 - MENU_GAP_ABOVE_LINE - MENU_HEIGHT,
        "with room above, the menu's bottom rides the gap above the line"
    );
    let near_top = MenuAnchor {
        center_x: 100.0,
        line_top: 30.0,
        line_bottom: 50.0,
    };
    assert_eq!(
        near_top.menu_top(),
        50.0 + MENU_GAP_ABOVE_LINE,
        "without room above, the menu's top rides the gap below the line"
    );
}
