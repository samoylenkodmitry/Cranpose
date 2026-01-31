//! Button widget implementation

#![allow(non_snake_case)]

use crate::composable;
use crate::layout::policies::FlexMeasurePolicy;
use crate::modifier::Modifier;
use crate::widgets::Layout;
use cranpose_core::NodeId;
use cranpose_ui_layout::{HorizontalAlignment, LinearArrangement};

/// A clickable button with a background and content.
///
/// # When to use
/// Use this to trigger an action when clicked. The button serves as a container
/// for other composables (typically `Text`).
///
/// # Arguments
///
/// * `modifier` - Modifiers to apply to the button container (e.g., size, padding).
/// * `on_click` - The callback to execute when the button is clicked.
/// * `content` - The content to display inside the button (e.g., `Text` or `Icon`).
///
/// # Example
///
/// ```rust,ignore
/// Button(
///     Modifier::padding(8.0),
///     || println!("Clicked!"),
///     || Text("Click Me", Modifier::empty())
/// );
/// ```
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
