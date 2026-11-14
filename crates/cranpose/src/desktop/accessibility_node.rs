use accesskit::{Action, CustomAction, Invalid, Live, Node, Rect, Role, Toggled};
use cranpose_ui::{LiveRegionMode, ToggleableState};

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};

fn accesskit_element_role(element: &AccessibilityElement) -> Role {
    let details = element.details();
    let scrolls = details.vertical_scroll.is_some() || details.horizontal_scroll.is_some();
    match details.progress {
        Some(_) if details.adjustable && element.role != AccessibilityRole::ValuePicker => {
            Role::Slider
        }
        Some(_) => accesskit_role(element.role),
        None if details.pane_title.is_some() && element.role == AccessibilityRole::StaticText => {
            Role::Region
        }
        None if scrolls && element.label.is_empty() && !element.role.is_named_container() => {
            scroll_role(element)
        }
        None if details.password => Role::PasswordInput,
        None if element.role.is_text_field() && details.multiline => Role::MultilineTextInput,
        None => accesskit_role(element.role),
    }
}

pub(super) fn accesskit_node(element: &AccessibilityElement) -> Node {
    let role = accesskit_element_role(element);
    let mut node = Node::new(role);
    if role == Role::Label {
        node.set_value(element.label.as_str());
    } else {
        node.set_label(element.label.as_str());
    }
    if let Some(title) = &element.details().pane_title {
        node.set_label(title.as_str());
    }
    node.set_bounds(Rect {
        x0: element.bounds.x as f64,
        y0: element.bounds.y as f64,
        x1: (element.bounds.x + element.bounds.width) as f64,
        y1: (element.bounds.y + element.bounds.height) as f64,
    });
    apply_state(&mut node, element);
    apply_actions(&mut node, element);
    node
}

pub(super) fn accesskit_live(mode: LiveRegionMode) -> Live {
    match mode {
        LiveRegionMode::Polite => Live::Polite,
        LiveRegionMode::Assertive => Live::Assertive,
    }
}

/// A scroll container with no label of its own: a list when it says how many
/// rows it holds, a plain scroll view otherwise.
fn scroll_role(element: &AccessibilityElement) -> Role {
    if element.details().collection.is_some() {
        Role::List
    } else {
        Role::ScrollView
    }
}

/// The accesskit role of each role a reader names.
pub(super) const ACCESSKIT_ROLES: [(AccessibilityRole, Role); 24] = [
    (AccessibilityRole::Button, Role::Button),
    (AccessibilityRole::StaticText, Role::Label),
    (AccessibilityRole::TextField, Role::TextInput),
    (AccessibilityRole::Checkbox, Role::CheckBox),
    (AccessibilityRole::Switch, Role::Switch),
    (AccessibilityRole::RadioButton, Role::RadioButton),
    (AccessibilityRole::Tab, Role::Tab),
    (AccessibilityRole::Image, Role::Image),
    (AccessibilityRole::Header, Role::Heading),
    (AccessibilityRole::Dialog, Role::Dialog),
    (AccessibilityRole::DropdownList, Role::ComboBox),
    (AccessibilityRole::ValuePicker, Role::SpinButton),
    (AccessibilityRole::Link, Role::Link),
    (AccessibilityRole::SearchField, Role::SearchInput),
    (AccessibilityRole::ProgressBar, Role::ProgressIndicator),
    (AccessibilityRole::ToggleButton, Role::Button),
    (AccessibilityRole::Alert, Role::Alert),
    (AccessibilityRole::Toolbar, Role::Toolbar),
    (AccessibilityRole::Menu, Role::Menu),
    (AccessibilityRole::MenuItem, Role::MenuItem),
    (AccessibilityRole::TabBar, Role::TabList),
    (AccessibilityRole::List, Role::List),
    (AccessibilityRole::ListItem, Role::ListItem),
    (AccessibilityRole::RadioGroup, Role::RadioGroup),
];

const _: () = assert!(ACCESSKIT_ROLES.len() == AccessibilityRole::ALL.len());

/// The accesskit role a screen reader reads the control as.
pub(super) fn accesskit_role(role: AccessibilityRole) -> Role {
    accessibility::role_entry(&ACCESSKIT_ROLES, role, Role::Unknown)
}

/// What the control says about itself beyond its name: its value, the state
/// description, and whether it is selected, toggled or disabled.
fn apply_state(node: &mut Node, element: &AccessibilityElement) {
    let details = element.details();
    if details.is_modal {
        node.set_modal();
    }
    if let Some(value) = &element.value {
        node.set_value(value.as_str());
    }
    if let Some(description) = accessibility::description_text(element) {
        node.set_description(description.as_str());
    }
    if let Some(language) = &details.language {
        node.set_language(language.as_str());
    }
    if details.error.is_some() {
        node.set_invalid(Invalid::True);
    }
    if let Some(item) = element.collection_item {
        node.set_position_in_set(item.position.saturating_sub(1));
        node.set_size_of_set(item.count);
    }
    if let Some(selected) = selected_state(element) {
        node.set_selected(selected);
    }
    if let Some(toggled) = toggled_state(element) {
        node.set_toggled(match toggled {
            ToggleableState::Indeterminate => Toggled::Mixed,
            ToggleableState::On => Toggled::True,
            ToggleableState::Off => Toggled::False,
        });
    }
    if !element.enabled {
        node.set_disabled();
    }
    if let Some(expanded) = details.expanded {
        node.set_expanded(expanded);
    }
    if let Some(mode) = details.live_region {
        node.set_live(accesskit_live(mode));
    }
    if let Some(progress) = details.progress {
        node.set_numeric_value(progress.current as f64);
        node.set_min_numeric_value(progress.start as f64);
        node.set_max_numeric_value(progress.end as f64);
        node.set_numeric_value_step(progress.step() as f64);
    }
    if let Some(range) = details.vertical_scroll {
        node.set_scroll_y(range.value as f64);
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(range.max_value as f64);
    }
    if let Some(range) = details.horizontal_scroll {
        node.set_scroll_x(range.value as f64);
        node.set_scroll_x_min(0.0);
        node.set_scroll_x_max(range.max_value as f64);
    }
}

fn reads_selection_as_pressed(role: AccessibilityRole) -> bool {
    matches!(
        role,
        AccessibilityRole::Button | AccessibilityRole::ToggleButton
    )
}

fn selected_state(element: &AccessibilityElement) -> Option<bool> {
    element.selected.filter(|_| {
        element.role != AccessibilityRole::RadioButton && !reads_selection_as_pressed(element.role)
    })
}

fn toggled_state(element: &AccessibilityElement) -> Option<ToggleableState> {
    if reads_selection_as_pressed(element.role) {
        element
            .toggled
            .or_else(|| element.selected.map(ToggleableState::from))
    } else {
        accessibility::checked_state(element)
    }
}

/// What a screen reader can do with the control: activate it, run one of the
/// actions it lists, or put focus on it. accesskit has no long press of its
/// own, so a long press is the last action in that list.
fn apply_actions(node: &mut Node, element: &AccessibilityElement) {
    let details = element.details();
    if !element.enabled {
        return;
    }
    if element.clickable {
        node.add_action(Action::Click);
    }
    let listed = accessibility::listed_actions(element);
    if !listed.is_empty() {
        node.add_action(Action::CustomAction);
        node.set_custom_actions(
            listed
                .into_iter()
                .enumerate()
                .map(|(index, label)| CustomAction {
                    id: index as i32,
                    description: label,
                })
                .collect::<Vec<_>>(),
        );
    }
    if let Some(expanded) = details.expanded {
        node.add_action(if expanded {
            Action::Collapse
        } else {
            Action::Expand
        });
    }
    if element.focusable {
        node.add_action(Action::Focus);
    }
    if element.role.is_text_field() {
        node.add_action(Action::SetValue);
    }
    if details.text_selection.is_some() {
        node.add_action(Action::SetTextSelection);
    }
    if details.adjustable {
        node.add_action(Action::SetValue);
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
    }
    if let Some(range) = details.vertical_scroll {
        if range.can_scroll_forward() {
            node.add_action(Action::ScrollDown);
        }
        if range.can_scroll_backward() {
            node.add_action(Action::ScrollUp);
        }
    }
    if let Some(range) = details.horizontal_scroll {
        if range.can_scroll_forward() {
            node.add_action(Action::ScrollRight);
        }
        if range.can_scroll_backward() {
            node.add_action(Action::ScrollLeft);
        }
    }
    apply_row_offset(node, element);
}

/// The rows a screen reader may ask a list for. accesskit 0.24.1 has no
/// scroll-to-index action, so the list carries its rows as the scroll offset
/// of the axis it runs along: the reader reads the range, sets an offset in
/// rows, and the list puts that row at the top.
fn apply_row_offset(node: &mut Node, element: &AccessibilityElement) {
    let details = element.details();
    let count = accessibility::row_count(element);
    if count == 0 {
        return;
    }
    let last = (count - 1) as f64;
    let first_visible = details
        .vertical_scroll
        .or(details.horizontal_scroll)
        .map_or(0.0, |range| f64::from(range.value).clamp(0.0, last));
    if details.vertical_scroll.is_some() {
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(last);
        node.set_scroll_y(first_visible);
    } else {
        node.set_scroll_x_min(0.0);
        node.set_scroll_x_max(last);
        node.set_scroll_x(first_visible);
    }
    node.add_action(Action::SetScrollOffset);
}
