use std::rc::Rc;

use cranpose_foundation::SemanticsWidgetRole;

use super::{inspector_metadata, Modifier, SemanticsConfiguration};

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
mod tests {
    use std::cell::Cell;

    use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};

    use super::*;
    use crate::modifier::{collect_semantics_from_modifier, collect_slices_from_modifier, Point};

    /// A click on a real chain is a pointer down followed by a pointer up: the
    /// click fires on the release, and only then.
    fn tap(modifier: &Modifier) {
        let slices = collect_slices_from_modifier(modifier);
        let handlers = slices.pointer_inputs();
        assert_eq!(handlers.len(), 1, "toggleable takes pointer input once");
        let at = Point { x: 4.0, y: 4.0 };
        for kind in [PointerEventKind::Down, PointerEventKind::Up] {
            let mut event = PointerEvent::new(kind, at, at);
            event.buttons = PointerButtons::new().with(PointerButton::Primary);
            handlers[0](event);
        }
    }

    #[test]
    fn a_click_reports_the_new_value_not_the_old_one() {
        let _app_context = crate::render_state::app_context_test_scope();
        for start in [false, true] {
            let seen: Rc<Cell<Option<bool>>> = Rc::new(Cell::new(None));
            let sink = seen.clone();
            let modifier =
                Modifier::empty().toggleable(start, None, None, move |next| sink.set(Some(next)));
            tap(&modifier);
            assert_eq!(
                seen.get(),
                Some(!start),
                "a toggle hands over the value it is moving to"
            );
        }
    }

    #[test]
    fn a_toggleable_row_reads_as_clickable_and_carries_its_description() {
        let modifier =
            Modifier::empty().toggleable(true, Some("Haptics, on".to_string()), None, |_| {});
        let semantics = collect_semantics_from_modifier(&modifier)
            .expect("a toggleable row publishes semantics");
        assert!(semantics.is_clickable);
        assert_eq!(
            semantics.content_description.as_deref(),
            Some("Haptics, on")
        );
        // The state is published, not left for the caller to spell into the
        // description: a reader landing here can say the row is on.
        assert_eq!(semantics.toggled, Some(true));
        // And no role unless one is asked for: a toggleable row could be a
        // checkbox or a switch, which is why Compose's parameter is nullable.
        assert_eq!(semantics.role, None);
    }

    #[test]
    fn a_role_reaches_the_semantics_so_a_reader_can_say_what_the_control_is() {
        let modifier = Modifier::empty().toggleable(
            false,
            Some("Haptics, off".to_string()),
            Some(SemanticsWidgetRole::Switch),
            |_| {},
        );
        let semantics = collect_semantics_from_modifier(&modifier).expect("semantics");
        assert_eq!(semantics.role, Some(SemanticsWidgetRole::Switch));
        // And it does not displace anything the row already published.
        assert!(semantics.is_clickable);
        assert_eq!(semantics.toggled, Some(false));
        assert_eq!(
            semantics.content_description.as_deref(),
            Some("Haptics, off")
        );
    }
}
