//! Button widget implementation

#![allow(non_snake_case)]

use crate::composable;
use crate::interaction::MutableInteractionSource;
use crate::layout::policies::FlexMeasurePolicy;
use crate::modifier::Modifier;
use crate::widgets::Layout;
use cranpose_core::NodeId;
use cranpose_ui_layout::{HorizontalAlignment, LinearArrangement};
use std::cell::RefCell;
use std::rc::Rc;

fn button_modifier<F>(
    modifier: Modifier,
    interaction_source: Option<MutableInteractionSource>,
    on_click: F,
) -> Modifier
where
    F: FnMut() + 'static,
{
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));
    let modifier = if let Some(interaction_source) = interaction_source {
        modifier.press_interaction_source(interaction_source)
    } else {
        modifier
    };

    modifier.clickable(move |_point| {
        (on_click_rc.borrow_mut())();
    })
}

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
    Layout(
        button_modifier(modifier, None, on_click),
        FlexMeasurePolicy::column(
            LinearArrangement::Center,
            HorizontalAlignment::CenterHorizontally,
        ),
        content,
    )
}

#[composable]
pub fn ButtonWithInteractionSource<F, G>(
    modifier: Modifier,
    interaction_source: MutableInteractionSource,
    on_click: F,
    content: G,
) -> NodeId
where
    F: FnMut() + 'static,
    G: FnMut() + 'static,
{
    Layout(
        button_modifier(modifier, Some(interaction_source), on_click),
        FlexMeasurePolicy::column(
            LinearArrangement::Center,
            HorizontalAlignment::CenterHorizontally,
        ),
        content,
    )
}
