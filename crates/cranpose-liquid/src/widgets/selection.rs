use std::rc::Rc;

use cranpose_foundation::{SemanticsConfiguration, SemanticsCustomAction, SemanticsWidgetRole};

pub(super) fn selection_semantics(
    label: String,
    role: SemanticsWidgetRole,
    index: usize,
    selected: usize,
    on_select: Rc<dyn Fn(usize)>,
) -> impl Fn(&mut SemanticsConfiguration) {
    let action = SemanticsCustomAction::new("", move || on_select(index));
    move |config| {
        config.role = Some(role);
        config.on_click = Some(action.clone());
        config.selected = Some(index == selected);
        config.content_description = Some(label.clone());
    }
}
