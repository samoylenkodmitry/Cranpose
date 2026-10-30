use std::rc::Rc;

use cranpose_foundation::{SemanticsConfiguration, SemanticsCustomAction, SemanticsWidgetRole};

// Whether an element of a selectable group is the selected one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectionState {
    Selected,
    NotSelected,
}

impl SelectionState {
    pub(super) fn of(index: usize, selected: usize) -> Self {
        if index == selected {
            Self::Selected
        } else {
            Self::NotSelected
        }
    }
}

pub(super) fn selection_semantics(
    label: String,
    role: SemanticsWidgetRole,
    index: usize,
    selected: SelectionState,
    on_select: Rc<dyn Fn(usize)>,
) -> impl Fn(&mut SemanticsConfiguration) {
    let action = SemanticsCustomAction::new("", move || on_select(index));
    move |config| {
        config.role = Some(role);
        config.on_click = Some(action.clone());
        config.selected = Some(selected == SelectionState::Selected);
        config.content_description = Some(label.clone());
    }
}
