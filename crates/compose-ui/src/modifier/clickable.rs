use super::{inspector_metadata, Modifier, Point, SemanticsConfiguration};
use crate::modifier_nodes::ClickableElement;
use std::rc::Rc;

impl Modifier {
    /// Make the component clickable.
    ///
    /// Example: `Modifier::empty().clickable(|pt| println!("Clicked at {:?}", pt))`
    pub fn clickable(self, handler: impl Fn(Point) + 'static) -> Self {
        let handler = Rc::new(handler);
        let modifier = Self::with_element(ClickableElement::with_handler(handler))
            .with_inspector_metadata(inspector_metadata("clickable", |info| {
                info.add_property("onClick", "provided");
            }))
            .then(Modifier::empty().semantics(|config: &mut SemanticsConfiguration| {
                config.is_clickable = true;
            }));
        self.then(modifier)
    }
}
