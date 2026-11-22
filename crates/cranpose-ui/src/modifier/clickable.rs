use std::rc::Rc;

use super::{Modifier, Point, SemanticsConfiguration, inspector_metadata};
use crate::{indication::current_indication, modifier_nodes::ClickableElement};

impl Modifier {
    /// Make the component clickable.
    ///
    /// The component shows the [`Indication`](crate::Indication) of
    /// [`local_indication`](crate::local_indication) where the modifier is made:
    /// while a pointer rests on it and while one presses it. A press that travels
    /// past the drag threshold stops showing at once and does not click.
    ///
    /// Example: `Modifier::empty().clickable(|pt| println!("Clicked at {:?}", pt))`
    pub fn clickable(self, handler: impl Fn(Point) + 'static) -> Self {
        let handler = Rc::new(handler);
        let element = ClickableElement::with_handler(handler).indication(current_indication());
        let modifier = Self::with_element(element).with_inspector_metadata(inspector_metadata(
            "clickable",
            |info| {
                info.add_property("onClick", "provided");
            },
        ));
        self.then(pressable(modifier))
    }

    /// Make the component react synchronously to primary-pointer press while
    /// retaining ordinary click confirmation on release. It shows its
    /// indication as [`clickable`](Self::clickable) does.
    pub fn clickable_on_press(
        self,
        on_press: impl Fn(Point) + 'static,
        on_click: impl Fn(Point) + 'static,
    ) -> Self {
        let element = ClickableElement::with_handlers(Rc::new(on_press), Rc::new(on_click))
            .indication(current_indication());
        let modifier = Self::with_element(element).with_inspector_metadata(inspector_metadata(
            "clickableOnPress",
            |info| {
                info.add_property("onPress", "provided");
                info.add_property("onClick", "provided");
            },
        ));
        self.then(pressable(modifier))
    }
}

/// What every control a pointer presses also gets: a reader's click, and a
/// focus target with the keyboard ring, so Tab reaches it and Enter presses
/// it.
fn pressable(modifier: Modifier) -> Modifier {
    modifier
        .then(
            Modifier::empty().stable_semantics(|config: &mut SemanticsConfiguration| {
                config.is_clickable = true;
            }),
        )
        .focusable()
}
