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

/// A single clickable menu label.
fn menu_item(label: &str, action: Rc<dyn Fn()>) {
    Text(
        label.to_string(),
        Modifier::empty()
            .padding(10.0)
            .clickable(move |_point| action()),
        menu_text_style(),
    );
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
