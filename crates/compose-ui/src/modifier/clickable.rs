use super::{inspector_metadata, Modifier, Point, SemanticsConfiguration};
use crate::modifier_nodes::ClickableElement;
use std::rc::Rc;

impl Modifier {
    pub fn clickable(handler: impl Fn(Point) + 'static) -> Self {
        let handler = Rc::new(handler);
        Self::with_element(ClickableElement::with_handler(handler))
            .with_inspector_metadata(inspector_metadata("clickable", |info| {
                info.add_property("onClick", "provided");
            }))
            .then(
                Modifier::empty().semantics(|config: &mut SemanticsConfiguration| {
                    config.is_clickable = true;
                }),
            )
    }
}
