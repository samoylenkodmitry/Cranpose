use std::rc::Rc;

use cranpose_foundation::SemanticsWidgetRole;

use super::{Modifier, SemanticsConfiguration, inspector_metadata};

impl Modifier {
    /// Make the component a two-state control.
    ///
    /// This is Compose's `Modifier.toggleable(value, enabled, role,
    /// onValueChange)`: a click hands the callback the **new** value, so a
    /// caller writes `.toggleable(checked, None, None, move |next|
    /// state.set(next))` and never has to read the old one back out of its own
    /// state to invert it.
    ///
    /// `role` is what a screen reader announces the control **as** — "switch",
    /// "checkbox" — before it is acted on, and it is genuinely optional: a
    /// toggleable row could be either, so Compose's parameter defaults to
    /// `null` and so does passing `None` here. Wear's own `SwitchButton` leaves
    /// it unset on the row and puts `Role.Switch` on the `Switch` control
    /// inside, which merges up into the same node; naming it on the row reaches
    /// the same announcement without leaning on a merge rule.
    ///
    /// A toggleable control with no role is not silent — the description and
    /// the state below still speak — but it is announced as an unnamed
    /// something the reader cannot say is toggleable, which is the whole of the
    /// difference.
    ///
    /// The **state** it publishes regardless: `toggled`, Compose's
    /// `toggleableState`, so a reader landing on the row says whether it is on
    /// without the caller spelling it into the description. `description` stays
    /// because a row still needs a name, and a caller that wants the state
    /// spoken a particular way ("Haptics, on") can still say it there.
    pub fn toggleable(
        self,
        value: bool,
        description: Option<String>,
        role: Option<SemanticsWidgetRole>,
        on_value_change: impl Fn(bool) + 'static,
    ) -> Self {
        let on_value_change = Rc::new(on_value_change);
        let toggled = value;
        let modifier = Modifier::empty()
            .clickable(move |_point| on_value_change(!toggled))
            .with_inspector_metadata(inspector_metadata("toggleable", move |info| {
                info.add_property("value", if toggled { "true" } else { "false" });
                info.add_property("onValueChange", "provided");
            }))
            .then(
                Modifier::empty().semantics(move |config: &mut SemanticsConfiguration| {
                    config.is_clickable = true;
                    config.toggled = Some(toggled);
                    if let Some(description) = &description {
                        config.content_description = Some(description.clone());
                    }
                    if let Some(role) = role {
                        config.role = Some(role);
                    }
                }),
            );
        self.then(modifier)
    }
}

#[cfg(test)]
#[path = "tests/toggleable_tests.rs"]
mod tests;
