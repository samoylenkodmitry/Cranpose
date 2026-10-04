//! Button widget implementation

use std::{cell::RefCell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_ui_layout::{HorizontalAlignment, LinearArrangement};

use crate::{
    composable, interaction::MutableInteractionSource, layout::policies::FlexMeasurePolicy,
    modifier::Modifier, widgets::Layout,
};

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

/// Calls `on_click` after a click and places content inside a button container.
/// The modifier controls size and appearance. `spec` can supply an interaction source.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn SaveAction() {
///     Button(
///         Modifier::empty().padding(8.0),
///         ButtonSpec::default(),
///         || println!("Save requested"),
///         || {
///             Text("Save", Modifier::empty(), TextStyle::default());
///         },
///     );
/// }
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
            crate::density::density().density(),
        ),
        content,
    )
}

#[cfg(test)]
#[path = "tests/button_tests.rs"]
mod tests;
