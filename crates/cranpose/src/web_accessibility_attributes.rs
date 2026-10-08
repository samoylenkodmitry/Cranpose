//! What each node of the web accessibility mirror carries, worked out in
//! Rust, so the mirror writes only the attributes that changed and never
//! reads them back from the page.

use std::fmt::{Display, Write};

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole, edits_text};

/// The scroll container an element sits in: its virtual id, the move one page
/// on makes, and the last row a reader may ask the container for.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct PageTarget {
    pub(crate) container: i32,
    pub(crate) dx: f32,
    pub(crate) dy: f32,
    pub(crate) last_row: Option<usize>,
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn page_targets(
    ids: &[i32],
    elements: &[AccessibilityElement],
) -> Vec<Option<PageTarget>> {
    elements
        .iter()
        .map(|element| {
            let container = accessibility::scroll_container_for(elements, element)?;
            let index = elements
                .iter()
                .position(|candidate| std::ptr::eq(candidate, container))?;
            let (dx, dy) = accessibility::page_delta(container, true);
            let rows = accessibility::row_count(container);
            Some(PageTarget {
                container: *ids.get(index)?,
                dx,
                dy,
                last_row: (rows > 0).then(|| rows - 1),
            })
        })
        .collect()
}

/// The attributes of one mirror node, by name. The attributes the mirror's
/// listeners own, the composition and the selection of a field, and the
/// placement in `style` are never among them. The values share one buffer,
/// so a set that is filled again allocates nothing once it has grown.
#[derive(Default)]
pub(crate) struct MirrorAttributes {
    entries: Vec<(&'static str, usize, usize)>,
    text: String,
}

impl MirrorAttributes {
    /// Fills the set with the attributes of the mirror node for `element`,
    /// which has the virtual id `id` and pages the container `page` names.
    pub(crate) fn describe(
        &mut self,
        id: i32,
        element: &AccessibilityElement,
        page: Option<PageTarget>,
    ) {
        self.entries.clear();
        self.text.clear();
        let details = element.details();
        if element.role != AccessibilityRole::StaticText {
            self.set("aria-label", &element.label);
        }
        if !element.enabled {
            self.set("disabled", "");
        }
        self.set_number("data-cranpose-node", id);
        if element.clickable {
            self.set("data-cranpose-clickable", "");
        }
        if let Some(key) = element.canvas_key {
            self.set_number("data-cranpose-canvas", key);
        }
        if let Some(language) = &details.language {
            self.set("lang", language);
        }
        if (element.role != AccessibilityRole::StaticText || details.pane_title.is_some())
            && !edits_text(element)
        {
            self.set("role", accessibility::web_role(element));
        }
        if let Some(title) = &details.pane_title {
            self.set("aria-label", title);
        }
        self.describe_role_extras(element);
        self.describe_progress(element);
        self.describe_aria_state(element);
        if mirror_tag(element) == "input" {
            self.set(
                "type",
                if details.password {
                    "password"
                } else if element.role == AccessibilityRole::SearchField {
                    "search"
                } else {
                    "text"
                },
            );
        }
        self.describe_page(element, page);
        self.set("tabindex", tab_index(element));
    }

    /// What a role asks for beyond its name: a heading level or the modal
    /// flag on a dialog.
    fn describe_role_extras(&mut self, element: &AccessibilityElement) {
        if is_mirror_container(element) && element.role != AccessibilityRole::Dialog {
            return;
        }
        match element.role {
            AccessibilityRole::Header => self.set("aria-level", "2"),
            AccessibilityRole::Dialog => {
                self.set_flag("aria-modal", element.details().is_modal);
            }
            _ => {}
        }
    }

    /// The value an adjustable control holds, and the stops an arrow key
    /// moves it by. A screen reader reads the value and offers its own way to
    /// change it.
    fn describe_progress(&mut self, element: &AccessibilityElement) {
        let details = element.details();
        let Some(progress) = details.progress else {
            return;
        };
        self.set_number("aria-valuenow", progress.current);
        self.set_number("aria-valuemin", progress.start.min(progress.end));
        self.set_number("aria-valuemax", progress.start.max(progress.end));
        if let Some(text) = &details.state_description {
            self.set("aria-valuetext", text);
        }
        if details.adjustable && element.enabled {
            self.set_number("data-cranpose-value", progress.current);
            self.set_number("data-cranpose-min", progress.start);
            self.set_number("data-cranpose-max", progress.end);
            self.set_number("data-cranpose-step", progress.step());
        }
    }

    /// What the control says about itself in words, the checked or selected
    /// flag, and whether the control is disabled.
    fn describe_aria_state(&mut self, element: &AccessibilityElement) {
        let details = element.details();
        if let Some(state) = accessibility::state_with_error(element) {
            self.set("aria-description", &state);
        }
        if details.password {
            self.set("aria-roledescription", "password");
        }
        if details.error.is_some() {
            self.set("aria-invalid", "true");
        }
        if let Some(item) = element.collection_item {
            self.set_number("aria-posinset", item.position);
            self.set_number("aria-setsize", item.count);
        }
        if let Some(expanded) = details.expanded {
            self.set_flag("aria-expanded", expanded);
        }
        if let Some(toggled) = element.toggled {
            let flag = if element.role == AccessibilityRole::ToggleButton {
                "aria-pressed"
            } else {
                "aria-checked"
            };
            self.set_flag(flag, toggled);
        }
        if let Some(selected) = element.selected {
            let flag = match element.role {
                AccessibilityRole::RadioButton => "aria-checked",
                AccessibilityRole::Button | AccessibilityRole::ToggleButton => "aria-pressed",
                _ => "aria-selected",
            };
            self.set_flag(flag, selected);
        }
        if !element.enabled {
            self.set("aria-disabled", "true");
        }
    }

    /// The paging data of a control inside a scroll container.
    fn describe_page(&mut self, element: &AccessibilityElement, page: Option<PageTarget>) {
        let Some(page) = page else {
            return;
        };
        self.set_number("data-cranpose-page", page.container);
        self.set_number("data-cranpose-page-dx", page.dx);
        self.set_number("data-cranpose-page-dy", page.dy);
        if let Some(last_row) = page.last_row.filter(|_| takes_home_and_end(element)) {
            self.set_number("data-cranpose-last-row", last_row);
        }
    }

    fn set(&mut self, name: &'static str, value: &str) {
        self.set_with(name, |text| text.push_str(value));
    }

    fn set_flag(&mut self, name: &'static str, value: bool) {
        self.set(name, if value { "true" } else { "false" });
    }

    fn set_number(&mut self, name: &'static str, value: impl Display) {
        // Writing into a string cannot fail.
        self.set_with(name, |text| {
            let _ = write!(text, "{value}");
        });
    }

    /// Gives `name` the value `write` puts into the buffer. A later value of
    /// a name replaces the earlier one, as a second write of an attribute
    /// does.
    fn set_with(&mut self, name: &'static str, write: impl FnOnce(&mut String)) {
        let start = self.text.len();
        write(&mut self.text);
        let end = self.text.len();
        match self.entries.iter_mut().find(|(known, ..)| *known == name) {
            Some(entry) => *entry = (name, start, end),
            None => self.entries.push((name, start, end)),
        }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(known, ..)| *known == name)
            .and_then(|(_, start, end)| self.text.get(*start..*end))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&'static str, &str)> {
        self.entries
            .iter()
            .filter_map(|(name, start, end)| Some((*name, self.text.get(*start..*end)?)))
    }

    /// The attributes a node that carries `shown` must write to carry this
    /// set: the ones `shown` lacks or holds with another value.
    pub(crate) fn written_over<'a>(
        &'a self,
        shown: &'a Self,
    ) -> impl Iterator<Item = (&'static str, &'a str)> {
        self.iter()
            .filter(move |(name, value)| shown.get(name) != Some(*value))
    }

    /// The attributes a node that carries `shown` must remove to carry this
    /// set.
    pub(crate) fn dropped_from<'a>(
        &'a self,
        shown: &'a Self,
    ) -> impl Iterator<Item = &'static str> {
        shown
            .entries
            .iter()
            .map(|(name, ..)| *name)
            .filter(move |name| self.get(name).is_none())
    }
}

/// The text a mirror node holds as its content: the words of a heading or
/// of plain text, and the value of a text field a reader does not edit. A
/// container holds the nodes inside it instead, and a field a reader edits
/// holds its text as its value.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn mirror_text(element: &AccessibilityElement) -> Option<&str> {
    if is_mirror_container(element) {
        return None;
    }
    match element.role {
        AccessibilityRole::StaticText | AccessibilityRole::Header => Some(&element.label),
        AccessibilityRole::TextField | AccessibilityRole::SearchField if !edits_text(element) => {
            element.value.as_deref()
        }
        _ => None,
    }
}

fn is_mirror_container(element: &AccessibilityElement) -> bool {
    let details = element.details();
    element.role.is_named_container()
        || element.role == AccessibilityRole::Dialog
        || details.vertical_scroll.is_some()
        || details.horizontal_scroll.is_some()
        || details.pane_title.is_some()
}

/// The element a control is mirrored as: a text field is an input or a text
/// area, so a reader walks and edits its text the way it does any form
/// field; a control a click reaches is a button; anything else is a span.
pub(crate) fn mirror_tag(element: &AccessibilityElement) -> &'static str {
    if edits_text(element) {
        if element.details().multiline && !element.details().password {
            "textarea"
        } else {
            "input"
        }
    } else if is_mirror_container(element) {
        "div"
    } else if element.clickable {
        "button"
    } else {
        "span"
    }
}

/// Whether Home and End on this mirror node may reach the list around it. A
/// text field moves its caret with those two keys and a slider moves its
/// value, so inside a list those two keep them.
fn takes_home_and_end(element: &AccessibilityElement) -> bool {
    !element.role.is_text_field() && !element.details().adjustable
}

/// Where the mirrored control sits in the Tab order: a focus target or an
/// adjustable control takes Tab, a plain button keeps the browser's default,
/// and text stays out of the way.
fn tab_index(element: &AccessibilityElement) -> &'static str {
    if element.enabled
        && element.tab_stop
        && (element.focusable || element.details().adjustable || element.clickable)
    {
        "0"
    } else {
        "-1"
    }
}

#[cfg(test)]
#[path = "tests/web_accessibility_attributes_tests.rs"]
mod tests;
