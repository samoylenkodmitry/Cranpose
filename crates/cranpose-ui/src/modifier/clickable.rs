use std::rc::Rc;

use super::{Modifier, Point, SemanticsConfiguration, inspector_metadata};
use crate::modifier_nodes::ClickableElement;

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
            .then(
                Modifier::empty().semantics(|config: &mut SemanticsConfiguration| {
                    config.is_clickable = true;
                }),
            );
        self.then(modifier)
    }

    /// Make the component react synchronously to primary-pointer press while
    /// retaining ordinary click confirmation on release.
    pub fn clickable_on_press(
        self,
        on_press: impl Fn(Point) + 'static,
        on_click: impl Fn(Point) + 'static,
    ) -> Self {
        let modifier = Self::with_element(ClickableElement::with_handlers(
            Rc::new(on_press),
            Rc::new(on_click),
        ))
        .with_inspector_metadata(inspector_metadata("clickableOnPress", |info| {
            info.add_property("onPress", "provided");
            info.add_property("onClick", "provided");
        }))
        .then(
            Modifier::empty().semantics(|config: &mut SemanticsConfiguration| {
                config.is_clickable = true;
            }),
        );
        self.then(modifier)
    }
}
