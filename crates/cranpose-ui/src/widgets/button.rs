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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ButtonSpec {
    pub interaction_source: Option<MutableInteractionSource>,
}

impl ButtonSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn interaction_source(mut self, interaction_source: MutableInteractionSource) -> Self {
        self.interaction_source = Some(interaction_source);
        self
    }
}

fn button_modifier<F>(modifier: Modifier, spec: ButtonSpec, on_click: F) -> Modifier
where
    F: FnMut() + 'static,
{
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));
    let modifier = if let Some(interaction_source) = spec.interaction_source {
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
/// * `modifier` - Modifiers to apply to the button container.
/// * `spec` - Configuration for button behavior.
/// * `on_click` - The callback to execute when the button is clicked.
/// * `content` - The content to display inside the button (e.g., `Text` or `Icon`).
///
/// # Example
///
/// ```rust,ignore
/// Button(
///     Modifier::padding(8.0),
///     ButtonSpec::default(),
///     || println!("Clicked!"),
///     || Text("Click Me", Modifier::empty())
/// );
/// ```
#[composable]
pub fn Button<F, G>(modifier: Modifier, spec: ButtonSpec, on_click: F, content: G) -> NodeId
where
    F: FnMut() + 'static,
    G: FnMut() + 'static,
{
    Layout(
        button_modifier(modifier, spec, on_click),
        FlexMeasurePolicy::column(
            LinearArrangement::Center,
            HorizontalAlignment::CenterHorizontally,
        ),
        content,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_core::{Composition, MemoryApplier};

    #[test]
    fn default_button_spec_has_no_interaction_source() {
        let spec = ButtonSpec::default();

        assert!(spec.interaction_source.is_none());
    }

    #[test]
    fn button_spec_builder_preserves_interaction_source() {
        let composition = Composition::new(MemoryApplier::new());
        let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
        let spec = ButtonSpec::new().interaction_source(source.clone());

        assert_eq!(spec.interaction_source, Some(source));
    }
}
