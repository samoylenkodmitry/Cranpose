//! Floating text-selection contextual menu (Copy / Cut / Paste / Select all).
//!
//! Rendered in the top-level overlay via [`Popup`] so it floats above the field
//! (and everything else) just above the active selection. Actions are supplied
//! by the caller (`BasicTextField` wires them to the clipboard plumbing).

#![allow(non_snake_case)]

use std::rc::Rc;

use crate::composable;
use crate::modifier::{Color, Modifier};
use crate::text::TextStyle;
use crate::widgets::box_widget::{Box, BoxSpec};
use crate::widgets::popup::Popup;
use crate::widgets::{Row, RowSpec, Text};
use crate::PointerInputScope;
use cranpose_foundation::PointerEventKind;
use cranpose_ui_graphics::{Point, Rect};

/// Background of the menu bar.
const MENU_BG: Color = Color(0.18, 0.18, 0.2, 0.96);
/// Menu item label color.
const MENU_FG: Color = Color(0.96, 0.96, 0.98, 1.0);
/// Estimated menu height (a single row of labels plus padding) used to float the
/// bar above the selection.
const MENU_HEIGHT: f32 = 40.0;

fn menu_text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(MENU_FG);
    style
}

/// A single tappable menu label.
///
/// The button drives its own [`pointer_input`](Modifier::pointer_input) gesture
/// rather than `clickable`. This is load-bearing: `clickable` only *consumes*
/// the pointer on the release (and never the press), so a `Down` on the menu
/// fell through to the text field below — which placed a caret and collapsed the
/// selection — before the release could run the action (worse after a scroll,
/// once the finger landed straight on the field). Consuming the whole
/// press→release gesture here keeps the tap on the menu: the field never sees
/// it, the selection survives, and the action runs on release.
fn menu_item(label: &str, action: Rc<dyn Fn()>) {
    Text(
        label.to_string(),
        Modifier::empty()
            .padding(10.0)
            .then(menu_item_pointer_input(label, action)),
        menu_text_style(),
    );
}

/// Builds the consuming tap gesture for a menu button: it swallows the press,
/// any moves, and the release, and fires `action` when the finger lifts after a
/// press that started on this button. Every event is consumed so the tap can
/// never fall through to the text field beneath the overlay. Keyed by the button
/// label so recomposition reuses the running gesture task.
pub(crate) fn menu_item_pointer_input(label: &str, action: Rc<dyn Fn()>) -> Modifier {
    let key = label.to_string();
    Modifier::empty().pointer_input(key, move |scope: PointerInputScope| {
        let action = Rc::clone(&action);
        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    // Only a release that follows a press *on this button* runs
                    // the action; a stray release without a press is ignored. The
                    // Down capture keeps the whole gesture on the button, so the
                    // field never sees it.
                    let mut pressed = false;
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Down => {
                                pressed = true;
                                event.consume();
                            }
                            PointerEventKind::Move => {
                                event.consume();
                            }
                            PointerEventKind::Up => {
                                if pressed {
                                    action();
                                }
                                pressed = false;
                                event.consume();
                            }
                            PointerEventKind::Cancel => {
                                pressed = false;
                                event.consume();
                            }
                            _ => {}
                        }
                    }
                })
                .await;
        }
    })
}

/// A floating Copy / Cut / Paste / Select-all menu shown just above the text
/// selection at `anchor` (window coordinates of the selection's top-center).
///
/// `can_paste` hides the Paste item when the clipboard is empty. Each action
/// runs against the focused field; the caller is expected to dismiss the menu.
#[composable]
pub fn TextSelectionMenu(
    anchor: Point,
    can_paste: bool,
    on_copy: impl Fn() + 'static,
    on_cut: impl Fn() + 'static,
    on_paste: impl Fn() + 'static,
    on_select_all: impl Fn() + 'static,
) {
    // Float the bar above the selection; keep it on-screen at the top edge.
    let popup_anchor = Rect {
        x: (anchor.x - 8.0).max(0.0),
        y: (anchor.y - MENU_HEIGHT).max(0.0),
        width: 0.0,
        height: 0.0,
    };

    let on_copy: Rc<dyn Fn()> = Rc::new(on_copy);
    let on_cut: Rc<dyn Fn()> = Rc::new(on_cut);
    let on_paste: Rc<dyn Fn()> = Rc::new(on_paste);
    let on_select_all: Rc<dyn Fn()> = Rc::new(on_select_all);

    Popup(popup_anchor, Point { x: 0.0, y: 0.0 }, move || {
        let on_copy = Rc::clone(&on_copy);
        let on_cut = Rc::clone(&on_cut);
        let on_paste = Rc::clone(&on_paste);
        let on_select_all = Rc::clone(&on_select_all);
        Box(
            Modifier::empty().background(MENU_BG).rounded_corners(6.0),
            BoxSpec::default(),
            move || {
                let on_copy = Rc::clone(&on_copy);
                let on_cut = Rc::clone(&on_cut);
                let on_paste = Rc::clone(&on_paste);
                let on_select_all = Rc::clone(&on_select_all);
                Row(Modifier::empty(), RowSpec::default(), move || {
                    menu_item("Copy", Rc::clone(&on_copy));
                    menu_item("Cut", Rc::clone(&on_cut));
                    if can_paste {
                        menu_item("Paste", Rc::clone(&on_paste));
                    }
                    menu_item("Select all", Rc::clone(&on_select_all));
                });
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier::{collect_slices_from_modifier, ModifierNodeSlices};
    use cranpose_foundation::PointerEvent;
    use cranpose_ui_graphics::Point;
    use std::cell::Cell;

    /// Collects the button's live pointer-input handler. Returns the owning
    /// [`ModifierNodeSlices`] too: it keeps the attached node (and its running
    /// coroutine) alive — dropping it would cancel the gesture and swallow the
    /// events.
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

    /// Bug 7: a tap (press then release) on a menu button consumes BOTH the
    /// press and the release — so the tap can never fall through to the text
    /// field below (which would collapse the selection) — and runs the action on
    /// release.
    #[test]
    fn menu_button_consumes_the_tap_and_runs_the_action() {
        let _app_context = crate::render_state::app_context_test_scope();
        let ran = Rc::new(Cell::new(false));
        let action: Rc<dyn Fn()> = {
            let ran = Rc::clone(&ran);
            Rc::new(move || ran.set(true))
        };
        let modifier = menu_item_pointer_input("Copy", action);
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

    /// A stray release with no preceding press on this button is still consumed
    /// (never reaches the field) but does not run the action.
    #[test]
    fn menu_button_release_without_press_is_consumed_but_inert() {
        let _app_context = crate::render_state::app_context_test_scope();
        let ran = Rc::new(Cell::new(false));
        let action: Rc<dyn Fn()> = {
            let ran = Rc::clone(&ran);
            Rc::new(move || ran.set(true))
        };
        let modifier = menu_item_pointer_input("Cut", action);
        let (handler, _slices) = button_handler(&modifier);

        let release = up(5.0, 5.0);
        handler(release.clone());
        assert!(
            release.is_consumed(),
            "a release on the menu is consumed so it never hits the field"
        );
        assert!(!ran.get(), "a release with no matching press must not act");
    }
}
