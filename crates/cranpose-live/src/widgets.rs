//! Starter live adapters backed by Cranpose's native widgets.
use cranpose_ui::{ButtonSpec, ColumnSpec, Modifier, RowSpec, TextStyle};

use crate::composable;

/// Displays a live text value.
#[composable]
pub fn Text(text: String) {
    cranpose_ui::Text(text, Modifier::empty(), TextStyle::default());
}

/// Arranges live content vertically.
#[composable]
pub fn Column(content: impl FnMut() + 'static) {
    cranpose_ui::Column(Modifier::empty(), ColumnSpec::default(), content);
}

/// Arranges live content horizontally.
#[composable]
pub fn Row(content: impl FnMut() + 'static) {
    cranpose_ui::Row(Modifier::empty(), RowSpec::default(), content);
}

/// Displays a button connected to a live action.
#[composable]
pub fn Button(label: String, on_click: impl FnMut() + 'static) {
    cranpose_ui::Button(
        Modifier::empty(),
        ButtonSpec::default(),
        on_click,
        move || {
            cranpose_ui::Text(label.clone(), Modifier::empty(), TextStyle::default());
        },
    );
}
