use std::rc::Rc;

use cranpose_foundation::{SemanticsWidgetRole, ToggleableState};

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
        self.tri_state_toggleable(value.into(), description, role, move || {
            on_value_change(!value);
        })
    }

    /// Makes the component a control that is on, off or partly on, such as
    /// a check box over a list whose items are partly checked. A click calls
    /// `on_click`, which decides the next state.
    ///
    /// This is Compose's `Modifier.triStateToggleable(state, enabled, role,
    /// onClick)`. Readers hear the state: a partly checked box reads as
    /// "mixed" or "partially checked".
    pub fn tri_state_toggleable(
        self,
        state: ToggleableState,
        description: Option<String>,
        role: Option<SemanticsWidgetRole>,
        on_click: impl Fn() + 'static,
    ) -> Self {
        let on_click = Rc::new(on_click);
        let modifier = Modifier::empty()
            .clickable(move |_point| on_click())
            .with_inspector_metadata(inspector_metadata("toggleable", move |info| {
                info.add_property("state", format!("{state:?}"));
                info.add_property("onClick", "provided");
            }))
            .then(Modifier::empty().stable_semantics(
                move |config: &mut SemanticsConfiguration| {
                    config.is_clickable = true;
                    config.toggled = Some(state);
                    if let Some(description) = &description {
                        config.content_description = Some(description.clone());
                    }
                    if let Some(role) = role {
                        config.role = Some(role);
                    }
                },
            ));
        self.then(modifier)
    }
}

#[cfg(test)]
#[path = "tests/toggleable_tests.rs"]
mod tests;
