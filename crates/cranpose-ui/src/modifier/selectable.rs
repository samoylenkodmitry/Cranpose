use std::rc::Rc;

use cranpose_foundation::SemanticsWidgetRole;

use super::{Modifier, SemanticsConfiguration, inspector_metadata};

impl Modifier {
    /// Makes the component one choice among several, of which one is picked
    /// at a time: a tab, a radio row, an entry in a segmented control.
    ///
    /// This is Compose's `Modifier.selectable(selected, enabled, role,
    /// onClick)`. A screen reader reads the role, whether the choice is
    /// picked, and offers the click; a person who cannot see the screen hears
    /// "Receipts, tab, selected" rather than a bare word.
    ///
    /// `role` is what the reader announces the control **as**; a tab row
    /// passes `Some(SemanticsWidgetRole::Tab)`, a radio list
    /// `Some(SemanticsWidgetRole::RadioButton)`. The picked state is published
    /// either way, as Compose's `selected` semantics.
    pub fn selectable(
        self,
        selected: bool,
        role: Option<SemanticsWidgetRole>,
        on_click: impl Fn() + 'static,
    ) -> Self {
        let on_click = Rc::new(on_click);
        let modifier = Modifier::empty()
            .clickable(move |_point| on_click())
            .with_inspector_metadata(inspector_metadata("selectable", move |info| {
                info.add_property("selected", if selected { "true" } else { "false" });
                info.add_property("onClick", "provided");
            }))
            .then(Modifier::empty().semantics(selectable_semantics(selected, role)));
        self.then(modifier)
    }
}

fn selectable_semantics(
    selected: bool,
    role: Option<SemanticsWidgetRole>,
) -> impl Fn(&mut SemanticsConfiguration) {
    move |config| {
        config.is_clickable = true;
        config.selected = Some(selected);
        if let Some(role) = role {
            config.role = Some(role);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_tab_says_so_and_takes_a_click() {
        let mut config = SemanticsConfiguration::default();

        selectable_semantics(true, Some(SemanticsWidgetRole::Tab))(&mut config);

        assert_eq!(config.selected, Some(true));
        assert_eq!(config.role, Some(SemanticsWidgetRole::Tab));
        assert!(config.is_clickable);
    }

    #[test]
    fn a_choice_with_no_role_still_says_whether_it_is_picked() {
        let mut config = SemanticsConfiguration::default();

        selectable_semantics(false, None)(&mut config);

        assert_eq!(config.selected, Some(false));
        assert_eq!(config.role, None);
    }
}
