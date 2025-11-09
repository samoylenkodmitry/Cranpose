use super::{inspector_metadata, Modifier, Point};
use crate::modifier_nodes::ClickableElement;
use std::rc::Rc;

impl Modifier {
    pub fn clickable(handler: impl Fn(Point) + 'static) -> Self {
        let handler = Rc::new(handler);
        Self::with_element(
            ClickableElement::with_handler(handler.clone()),
            move |state| {
                state.click_handler = Some(handler.clone());
            },
        )
        .with_inspector_metadata(inspector_metadata("clickable", |info| {
            info.add_property("onClick", "provided");
        }))
    }
}
