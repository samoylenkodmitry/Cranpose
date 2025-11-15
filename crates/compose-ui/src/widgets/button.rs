//! Button widget implementation

#![allow(non_snake_case)]

use crate::composable;
use crate::layout::policies::FlexMeasurePolicy;
use crate::modifier::Modifier;
use crate::widgets::Layout;
use compose_core::NodeId;
use compose_ui_layout::{HorizontalAlignment, LinearArrangement};

/// Creates a button widget with click handling.
///
/// This is now implemented using LayoutNode with FlexMeasurePolicy (column layout),
/// following the Jetpack Compose pattern of using Layout for all widgets.
/// The clickable behavior is provided via the `.clickable()` modifier, which is part
/// of the modern modifier chain system.
#[composable]
pub fn Button<F, G>(modifier: Modifier, on_click: F, content: G) -> NodeId
where
    F: FnMut() + 'static,
    G: FnMut() + 'static,
{
    use std::cell::RefCell;
    use std::rc::Rc;

    // Wrap the on_click handler in Rc<RefCell<>> to make it callable from Fn closure
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));

    // Add clickable modifier to handle click events
    let clickable_modifier = modifier.clickable(move |_point| {
        (on_click_rc.borrow_mut())();
    });

    // Use Layout with FlexMeasurePolicy (column) to arrange button content
    // This matches how Button is implemented in Jetpack Compose
    Layout(
        clickable_modifier,
        FlexMeasurePolicy::column(
            LinearArrangement::Center,
            HorizontalAlignment::CenterHorizontally,
        ),
        content,
    )
}
