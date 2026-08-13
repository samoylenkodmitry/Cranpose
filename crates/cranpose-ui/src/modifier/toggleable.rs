use super::{inspector_metadata, Modifier, SemanticsConfiguration};
use std::rc::Rc;

impl Modifier {
    /// Make the component a two-state control.
    ///
    /// This is Compose's `Modifier.toggleable(value, onValueChange)`: a click
    /// hands the callback the **new** value, so a caller writes
    /// `.toggleable(checked, move |next| state.set(next))` and never has to
    /// read the old one back out of its own state to invert it.
    ///
    /// Compose deliberately sets no role here — a toggleable row could be a
    /// checkbox or a switch, and only the control inside it knows which — and
    /// this does the same.
    ///
    /// What it cannot publish yet is the **state**.
    /// [`SemanticsConfiguration`] carries a description and four booleans; it
    /// has no `toggled`, no `role` and no `state_description`, so a screen
    /// reader landing on a switch row hears the label and that the row is
    /// clickable, but not whether it is on. Passing a `description` is the only
    /// way to say so today, and it is why the parameter is here.
    pub fn toggleable(
        self,
        value: bool,
        description: Option<String>,
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
                    if let Some(description) = &description {
                        config.content_description = Some(description.clone());
                    }
                }),
            );
        self.then(modifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier::{collect_semantics_from_modifier, collect_slices_from_modifier, Point};
    use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
    use std::cell::Cell;

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
                Modifier::empty().toggleable(start, None, move |next| sink.set(Some(next)));
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
        let modifier = Modifier::empty().toggleable(true, Some("Haptics, on".to_string()), |_| {});
        let semantics = collect_semantics_from_modifier(&modifier)
            .expect("a toggleable row publishes semantics");
        assert!(semantics.is_clickable);
        assert_eq!(
            semantics.content_description.as_deref(),
            Some("Haptics, on")
        );
        // And no role: a toggleable row could be a checkbox or a switch, and
        // Compose leaves that to the control inside it.
        assert!(!semantics.is_button);
    }
}
