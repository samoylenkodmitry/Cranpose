use std::fmt::Debug;

use cranpose_app_shell::AppShell;
use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::Renderer;
use cranpose_ui::{
    Announcement, CollectionInfo, LayoutBox, LiveRegionMode, ProgressBarRangeInfo, ScrollAxisRange,
    SemanticsAction, SemanticsNode, SemanticsRole, SemanticsWidgetRole,
};

#[path = "accessibility_identity.rs"]
mod identity;
pub(crate) use identity::{AccessibilityIdentityError, AccessibilitySnapshot, ReplacedSnapshot};

#[cfg(any(
    test,
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) fn opened_dialog(
    current: &[AccessibilityElement],
    next: &[AccessibilityElement],
) -> Option<NodeId> {
    next.iter()
        .find(|element| {
            element.role == AccessibilityRole::Dialog
                && !current.iter().any(|old| {
                    old.identity_key() == element.identity_key()
                        && old.role == AccessibilityRole::Dialog
                })
        })
        .map(|element| element.node_id)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct AccessibilityRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl AccessibilityRect {
    pub(crate) const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[cfg(any(
        test,
        all(feature = "android", feature = "renderer-wgpu", target_os = "android")
    ))]
    pub(crate) fn center(self) -> (f32, f32) {
        (self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    fn is_visible(self) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessibilityRole {
    Button,
    StaticText,
    TextField,
    Checkbox,
    Switch,
    RadioButton,
    Tab,
    Image,
    Header,
    Dialog,
    DropdownList,
    ValuePicker,
    Link,
    SearchField,
    ProgressBar,
    ToggleButton,
    Alert,
    Toolbar,
    Menu,
    MenuItem,
    TabBar,
    List,
    ListItem,
    RadioGroup,
}

/// The role a reader names for each role an app declares. A table rather than
/// a match, so the platforms that name roles with plain data read theirs the
/// same way; the length assertion below keeps it whole when a role is added.
const WIDGET_ROLES: [(SemanticsWidgetRole, AccessibilityRole); 22] = [
    (SemanticsWidgetRole::Button, AccessibilityRole::Button),
    (SemanticsWidgetRole::Checkbox, AccessibilityRole::Checkbox),
    (SemanticsWidgetRole::Switch, AccessibilityRole::Switch),
    (
        SemanticsWidgetRole::RadioButton,
        AccessibilityRole::RadioButton,
    ),
    (SemanticsWidgetRole::Tab, AccessibilityRole::Tab),
    (SemanticsWidgetRole::Image, AccessibilityRole::Image),
    (
        SemanticsWidgetRole::DropdownList,
        AccessibilityRole::DropdownList,
    ),
    (
        SemanticsWidgetRole::ValuePicker,
        AccessibilityRole::ValuePicker,
    ),
    (SemanticsWidgetRole::Header, AccessibilityRole::Header),
    (SemanticsWidgetRole::Dialog, AccessibilityRole::Dialog),
    (SemanticsWidgetRole::Link, AccessibilityRole::Link),
    (
        SemanticsWidgetRole::SearchField,
        AccessibilityRole::SearchField,
    ),
    (
        SemanticsWidgetRole::ProgressBar,
        AccessibilityRole::ProgressBar,
    ),
    (
        SemanticsWidgetRole::ToggleButton,
        AccessibilityRole::ToggleButton,
    ),
    (SemanticsWidgetRole::Alert, AccessibilityRole::Alert),
    (SemanticsWidgetRole::Toolbar, AccessibilityRole::Toolbar),
    (SemanticsWidgetRole::Menu, AccessibilityRole::Menu),
    (SemanticsWidgetRole::MenuItem, AccessibilityRole::MenuItem),
    (SemanticsWidgetRole::TabBar, AccessibilityRole::TabBar),
    (SemanticsWidgetRole::List, AccessibilityRole::List),
    (SemanticsWidgetRole::ListItem, AccessibilityRole::ListItem),
    (
        SemanticsWidgetRole::RadioGroup,
        AccessibilityRole::RadioGroup,
    ),
];

const _: () = assert!(WIDGET_ROLES.len() == SemanticsWidgetRole::RadioGroup as usize + 1);

/// The ARIA role of each role, for the web mirror.
#[cfg(any(
    test,
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
const ARIA_ROLES: [(AccessibilityRole, &str); 24] = [
    (AccessibilityRole::Button, "button"),
    (AccessibilityRole::StaticText, "generic"),
    (AccessibilityRole::TextField, "textbox"),
    (AccessibilityRole::Checkbox, "checkbox"),
    (AccessibilityRole::Switch, "switch"),
    (AccessibilityRole::RadioButton, "radio"),
    (AccessibilityRole::Tab, "tab"),
    (AccessibilityRole::Image, "img"),
    (AccessibilityRole::Header, "heading"),
    (AccessibilityRole::Dialog, "dialog"),
    (AccessibilityRole::DropdownList, "combobox"),
    (AccessibilityRole::ValuePicker, "spinbutton"),
    (AccessibilityRole::Link, "link"),
    (AccessibilityRole::SearchField, "searchbox"),
    (AccessibilityRole::ProgressBar, "progressbar"),
    (AccessibilityRole::ToggleButton, "button"),
    (AccessibilityRole::Alert, "alert"),
    (AccessibilityRole::Toolbar, "toolbar"),
    (AccessibilityRole::Menu, "menu"),
    (AccessibilityRole::MenuItem, "menuitem"),
    (AccessibilityRole::TabBar, "tablist"),
    (AccessibilityRole::List, "list"),
    (AccessibilityRole::ListItem, "listitem"),
    (AccessibilityRole::RadioGroup, "radiogroup"),
];

/// The number the Android host reads each role as; the host's `className()`
/// and `roleDescription()` turn it back into what TalkBack says.
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
const ANDROID_ROLE_CODES: [(AccessibilityRole, i32); 24] = [
    (AccessibilityRole::Button, 1),
    (AccessibilityRole::StaticText, 2),
    (AccessibilityRole::TextField, 3),
    (AccessibilityRole::Checkbox, 4),
    (AccessibilityRole::Switch, 5),
    (AccessibilityRole::RadioButton, 6),
    (AccessibilityRole::Tab, 7),
    (AccessibilityRole::Image, 8),
    (AccessibilityRole::Header, 9),
    (AccessibilityRole::Dialog, 10),
    (AccessibilityRole::DropdownList, 11),
    (AccessibilityRole::ValuePicker, 12),
    (AccessibilityRole::Link, 13),
    (AccessibilityRole::SearchField, 14),
    (AccessibilityRole::ProgressBar, 15),
    (AccessibilityRole::ToggleButton, 16),
    (AccessibilityRole::Alert, 17),
    (AccessibilityRole::Toolbar, 18),
    (AccessibilityRole::Menu, 19),
    (AccessibilityRole::MenuItem, 20),
    (AccessibilityRole::TabBar, 21),
    (AccessibilityRole::List, 22),
    (AccessibilityRole::ListItem, 23),
    (AccessibilityRole::RadioGroup, 24),
];

/// What a table names a role as, or the fallback for a role the table lacks.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn role_entry<T: Copy>(
    table: &[(AccessibilityRole, T)],
    role: AccessibilityRole,
    fallback: T,
) -> T {
    table
        .iter()
        .find(|(named, _)| *named == role)
        .map_or(fallback, |(_, value)| *value)
}

#[cfg(any(
    test,
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn web_role(element: &AccessibilityElement) -> &'static str {
    if element.progress.is_some()
        && element.adjustable
        && element.role != AccessibilityRole::ValuePicker
    {
        "slider"
    } else if element.pane_title.is_some() && element.role == AccessibilityRole::StaticText {
        "region"
    } else {
        element.role.aria_name()
    }
}

impl AccessibilityRole {
    /// Every role, for the tables that name a role on a platform and the
    /// tests that check none is left out.
    pub(crate) const ALL: [Self; 24] = [
        Self::Button,
        Self::StaticText,
        Self::TextField,
        Self::Checkbox,
        Self::Switch,
        Self::RadioButton,
        Self::Tab,
        Self::Image,
        Self::Header,
        Self::Dialog,
        Self::DropdownList,
        Self::ValuePicker,
        Self::Link,
        Self::SearchField,
        Self::ProgressBar,
        Self::ToggleButton,
        Self::Alert,
        Self::Toolbar,
        Self::Menu,
        Self::MenuItem,
        Self::TabBar,
        Self::List,
        Self::ListItem,
        Self::RadioGroup,
    ];

    fn from_widget_role(role: SemanticsWidgetRole) -> Self {
        WIDGET_ROLES
            .iter()
            .find(|(widget, _)| *widget == role)
            .map_or(Self::StaticText, |(_, own)| *own)
    }

    /// The ARIA role a browser reads the control as.
    #[cfg(any(
        test,
        all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
    ))]
    pub(crate) fn aria_name(self) -> &'static str {
        role_entry(&ARIA_ROLES, self, "generic")
    }

    /// The number the Android host reads the control's role as.
    #[cfg(any(
        test,
        all(feature = "android", feature = "renderer-wgpu", target_os = "android")
    ))]
    pub(crate) fn android_code(self) -> i32 {
        role_entry(&ANDROID_ROLE_CODES, self, 2)
    }

    /// Whether a reader types into the control: a plain text field or a
    /// search field.
    #[cfg(any(
        test,
        all(feature = "desktop-shell", feature = "renderer-wgpu"),
        all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
        all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
    ))]
    pub(crate) fn is_text_field(self) -> bool {
        matches!(self, Self::TextField | Self::SearchField)
    }

    /// Whether the control is a container a reader walks into rather than
    /// stops on, named by its role: a toolbar, a menu, a tab bar or a list.
    pub(crate) fn is_named_container(self) -> bool {
        matches!(
            self,
            Self::Toolbar | Self::Menu | Self::TabBar | Self::List | Self::RadioGroup
        )
    }
}

const _: () = assert!(AccessibilityRole::ALL.len() == AccessibilityRole::RadioGroup as usize + 1);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccessibilityElement {
    pub(crate) node_id: NodeId,
    pub(crate) node_generation: u32,
    pub(crate) canvas_key: Option<u64>,
    pub(crate) label: String,
    pub(crate) state_description: Option<String>,
    pub(crate) click_label: Option<String>,
    /// What a long press on the control does, named for a reader. Present
    /// only when the control declared a long press at all.
    pub(crate) long_click_label: Option<String>,
    /// What the magic tap does, named for a reader. Present only when the
    /// control declared one.
    pub(crate) magic_tap_label: Option<String>,
    /// The short names Voice Control shows for the control.
    pub(crate) input_labels: Vec<String>,
    /// The language of the control's text, as a BCP 47 tag.
    pub(crate) language: Option<String>,
    pub(crate) value: Option<String>,
    pub(crate) bounds: AccessibilityRect,
    pub(crate) role: AccessibilityRole,
    pub(crate) clickable: bool,
    pub(crate) selected: Option<bool>,
    pub(crate) toggled: Option<bool>,
    pub(crate) enabled: bool,
    pub(crate) custom_actions: Vec<String>,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) focused: bool,
    pub(crate) live_region: Option<LiveRegionMode>,
    pub(crate) progress: Option<ProgressBarRangeInfo>,
    pub(crate) adjustable: bool,
    pub(crate) vertical_scroll: Option<ScrollAxisRange>,
    pub(crate) horizontal_scroll: Option<ScrollAxisRange>,
    /// Whether a screen reader may ask this list for the row at an index.
    pub(crate) scroll_to_index: bool,
    pub(crate) scroll_parent: Option<NodeId>,
    pub(crate) collection: Option<CollectionInfo>,
    pub(crate) collection_item: Option<CollectionItem>,
    pub(crate) pane_title: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) password: bool,
    pub(crate) expanded: Option<bool>,
    pub(crate) dismissable: bool,
    pub(crate) is_modal: bool,
    /// Where the caret of an editable field sits, or which stretch of its
    /// text is picked: the anchor and the end that moves, as byte offsets into
    /// `value`. A field that holds a secret publishes none.
    pub(crate) text_selection: Option<(usize, usize)>,
    pub(crate) multiline: bool,
}

impl AccessibilityElement {
    fn identity_key(&self) -> (NodeId, u32, Option<u64>) {
        (self.node_id, self.node_generation, self.canvas_key)
    }
}

impl Default for AccessibilityElement {
    fn default() -> Self {
        Self {
            node_id: 0,
            node_generation: 0,
            canvas_key: None,
            label: String::new(),
            state_description: None,
            click_label: None,
            long_click_label: None,
            magic_tap_label: None,
            input_labels: Vec::new(),
            language: None,
            value: None,
            bounds: AccessibilityRect::default(),
            role: AccessibilityRole::StaticText,
            clickable: false,
            selected: None,
            toggled: None,
            enabled: true,
            custom_actions: Vec::new(),
            focusable: false,
            tab_stop: true,
            focused: false,
            live_region: None,
            progress: None,
            adjustable: false,
            vertical_scroll: None,
            horizontal_scroll: None,
            scroll_to_index: false,
            scroll_parent: None,
            collection: None,
            collection_item: None,
            pane_title: None,
            error: None,
            password: false,
            expanded: None,
            dismissable: false,
            is_modal: false,
            text_selection: None,
            multiline: false,
        }
    }
}

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
pub(crate) fn snapshot_if_changed<R>(
    shell: &mut AppShell<R>,
    seen_revision: &mut Option<u64>,
) -> Option<Vec<AccessibilityElement>>
where
    R: Renderer,
    R::Error: Debug,
{
    let revision = shell.semantics_snapshot_revision();
    if *seen_revision == Some(revision) {
        return None;
    }
    let next = snapshot(shell);
    *seen_revision = Some(shell.semantics_snapshot_revision());
    Some(next)
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn snapshot<R>(shell: &mut AppShell<R>) -> Vec<AccessibilityElement>
where
    R: Renderer,
    R::Error: Debug,
{
    if !shell.semantics_active() {
        return Vec::new();
    }
    let mut bounds = HashMap::default();
    let has_layout = shell.with_layout_tree(|layout_tree| match layout_tree {
        Some(layout_tree) => {
            collect_bounds(layout_tree.root(), &mut bounds);
            true
        }
        None => false,
    });
    if !has_layout {
        return Vec::new();
    }
    let Some(semantics_tree) = shell.semantics_tree() else {
        return Vec::new();
    };
    project_semantics(semantics_tree.root(), &bounds)
}

#[cfg_attr(test, allow(dead_code))]
fn collect_bounds(root: &LayoutBox, bounds: &mut HashMap<NodeId, AccessibilityRect>) {
    bounds.insert(
        root.node_id,
        AccessibilityRect::new(root.rect.x, root.rect.y, root.rect.width, root.rect.height),
    );
    for child in &root.children {
        collect_bounds(child, bounds);
    }
}

fn project_semantics(
    root: &SemanticsNode,
    bounds: &HashMap<NodeId, AccessibilityRect>,
) -> Vec<AccessibilityElement> {
    let mut elements = Vec::new();
    project_node(root, bounds, false, None, None, &mut elements);
    elements
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn install_inspector<R: Renderer>(shell: &mut AppShell<R>, enabled: Option<bool>)
where
    R::Error: Debug,
{
    shell.set_inspector_projector(
        enabled
            .unwrap_or(cfg!(debug_assertions))
            .then_some(inspector_nodes),
    );
}

#[cfg_attr(test, allow(dead_code))]
fn inspector_nodes(
    layout: &cranpose_ui::LayoutTree,
    semantics: &cranpose_ui::SemanticsTree,
) -> Vec<cranpose_app_shell::inspector::InspectorNode> {
    let mut bounds = HashMap::default();
    collect_bounds(layout.root(), &mut bounds);
    project_semantics(semantics.root(), &bounds)
        .into_iter()
        .map(inspector_node)
        .collect()
}

fn inspector_node(element: AccessibilityElement) -> cranpose_app_shell::inspector::InspectorNode {
    let value = if element.password {
        "[protected]"
    } else {
        element.value.as_deref().unwrap_or("")
    };
    let mut actions = Vec::new();
    if element.clickable {
        actions.push("Activate".to_string());
    }
    if element.adjustable {
        actions.push("Adjust value".to_string());
    }
    if element.focusable {
        actions.push("Focus".to_string());
    }
    if matches!(
        element.role,
        AccessibilityRole::TextField | AccessibilityRole::SearchField
    ) {
        actions.push("Edit text".to_string());
    }
    actions.extend(element.custom_actions.iter().cloned());
    actions.extend(element.long_click_label.iter().cloned());
    actions.extend(element.magic_tap_label.iter().cloned());
    let details = format!(
        "Name: {}\nRole: {:?}\nValue: {}\nState: {}\nEnabled: {}  Focused: {}\nSelected: {:?}  Toggled: {:?}\nBounds: {:.1}, {:.1}  {:.1} x {:.1}\nActions: {}\nLive: {:?}\nRange: {:?}\nError: {}",
        element.label,
        element.role,
        value,
        element.state_description.as_deref().unwrap_or(""),
        element.enabled,
        element.focused,
        element.selected,
        checked_state(&element),
        element.bounds.x,
        element.bounds.y,
        element.bounds.width,
        element.bounds.height,
        actions.join(", "),
        element.live_region,
        element.progress,
        element.error.as_deref().unwrap_or("")
    );
    cranpose_app_shell::inspector::InspectorNode {
        node_id: element.node_id,
        canvas_key: element.canvas_key,
        bounds: cranpose_ui::Rect {
            x: element.bounds.x,
            y: element.bounds.y,
            width: element.bounds.width,
            height: element.bounds.height,
        },
        label: format!("{}, {:?}", element.label, element.role),
        details,
        focused: element.focused,
        issue: element.label.is_empty() && (element.clickable || element.focusable),
    }
}

fn project_node(
    node: &SemanticsNode,
    bounds: &HashMap<NodeId, AccessibilityRect>,
    suppress_static_text: bool,
    inherited_live_region: Option<LiveRegionMode>,
    inherited_scroll: Option<NodeId>,
    elements: &mut Vec<AccessibilityElement>,
) {
    if node.hidden {
        return;
    }
    let live_region = node.live_region.or(inherited_live_region);
    let first_new = elements.len();
    let clickable = node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }));
    let actionable = clickable || node.editable_text;
    let merges = node.merges_accessibility_descendants();
    let boundary = node.is_accessibility_boundary();
    let label = node.accessibility_label();
    let rect = bounds.get(&node.node_id).copied().unwrap_or_default();

    let container = is_container(node);
    if let Some(label) = label
        && rect.is_visible()
        && (boundary || !suppress_static_text)
    {
        elements.push(element_for_node(
            node,
            rect,
            label.into_owned(),
            clickable,
            live_region,
        ));
    } else if container && rect.is_visible() {
        elements.push(element_for_node(
            node,
            rect,
            String::new(),
            clickable,
            live_region,
        ));
    } else if actionable && rect.is_visible() {
        warn_unlabeled(node.node_id);
    }

    project_canvas_children(node, rect, live_region, elements);
    for element in &mut elements[first_new..] {
        element.node_generation = node.node_generation;
        element.scroll_parent = inherited_scroll;
    }

    let suppress_children = merges || (suppress_static_text && !boundary);
    let scroll_for_children = if container {
        Some(node.node_id)
    } else {
        inherited_scroll
    };
    project_children(
        node,
        bounds,
        suppress_children,
        live_region,
        scroll_for_children,
        elements,
    );
}

/// A node a reader walks into rather than stops on: a list, a scroll view, or
/// a group of tabs or radio buttons.
fn is_container(node: &SemanticsNode) -> bool {
    node.vertical_scroll.is_some()
        || node.horizontal_scroll.is_some()
        || node.selectable_group
        || node.is_modal
        || node.pane_title.is_some()
        || node.widget_role == Some(SemanticsWidgetRole::Dialog)
        || node
            .widget_role
            .is_some_and(|role| AccessibilityRole::from_widget_role(role).is_named_container())
}

pub(crate) fn checked_state(element: &AccessibilityElement) -> Option<bool> {
    if element.role == AccessibilityRole::RadioButton {
        element.selected.or(element.toggled)
    } else {
        element.toggled
    }
}

/// Projects the nodes under a container, then numbers the selectable controls
/// of a group so a reader hears which of how many each one is.
fn project_children(
    node: &SemanticsNode,
    bounds: &HashMap<NodeId, AccessibilityRect>,
    suppress_static_text: bool,
    live_region: Option<LiveRegionMode>,
    scroll_for_children: Option<NodeId>,
    elements: &mut Vec<AccessibilityElement>,
) {
    let first_child = elements.len();
    for child in node.accessibility_children() {
        project_node(
            child,
            bounds,
            suppress_static_text,
            live_region,
            scroll_for_children,
            elements,
        );
    }
    let selectable_group = node.selectable_group
        || matches!(
            node.widget_role,
            Some(SemanticsWidgetRole::RadioGroup | SemanticsWidgetRole::TabBar)
        );
    if selectable_group {
        number_group(node.node_id, first_child, elements);
    }
    if selectable_group || node.widget_role == Some(SemanticsWidgetRole::Menu) {
        mark_group_tab_stop(node.node_id, first_child, elements);
    }
}

fn mark_group_tab_stop(group: NodeId, first_child: usize, elements: &mut [AccessibilityElement]) {
    let members: Vec<_> = (first_child..elements.len())
        .filter(|index| {
            let element = &elements[*index];
            element.scroll_parent == Some(group)
                && matches!(
                    element.role,
                    AccessibilityRole::RadioButton
                        | AccessibilityRole::Tab
                        | AccessibilityRole::MenuItem
                )
        })
        .collect();
    let stop = members
        .iter()
        .copied()
        .filter(|index| elements[*index].enabled)
        .max_by_key(|index| {
            (
                elements[*index].focused,
                elements[*index].selected == Some(true),
                std::cmp::Reverse(*index),
            )
        });
    for index in members {
        elements[index].tab_stop = Some(index) == stop;
    }
}

/// Gives each selectable control under a group its place and the group's
/// size, and tells the group's own element how many it holds and which way
/// it runs.
fn number_group(group: NodeId, first_child: usize, elements: &mut [AccessibilityElement]) {
    let members: Vec<usize> = (first_child..elements.len())
        .filter(|index| {
            elements[*index].selected.is_some() && elements[*index].scroll_parent == Some(group)
        })
        .collect();
    let count = members.len();
    let Some(first) = members.first() else {
        return;
    };
    let horizontal = members.get(1).is_none_or(|second| {
        let (a, b) = (elements[*first].bounds, elements[*second].bounds);
        (b.x - a.x).abs() >= (b.y - a.y).abs()
    });
    for (index, position) in members.iter().zip(1..) {
        elements[*index].collection_item = Some(CollectionItem {
            position,
            count,
            horizontal,
        });
    }
    let (rows, columns) = if horizontal { (1, count) } else { (count, 1) };
    let radio_group = members
        .iter()
        .all(|index| elements[*index].role == AccessibilityRole::RadioButton);
    if let Some(element) = elements
        .iter_mut()
        .find(|element| element.node_id == group && element.canvas_key.is_none())
    {
        element.collection = Some(CollectionInfo { rows, columns });
        if element.role == AccessibilityRole::StaticText && radio_group {
            element.role = AccessibilityRole::RadioGroup;
        }
    }
}

/// Whether a control reads as open or as closed: one that says what closing
/// it does is open now, and one that says what opening it does is closed.
/// A control that says neither is not a thing a reader opens at all.
fn expansion(node: &SemanticsNode) -> Option<bool> {
    node.collapse
        .is_some()
        .then_some(true)
        .or_else(|| node.expand.is_some().then_some(false))
}

/// What a reader reads out for a control's long press: the verb phrase the
/// app gave, and for a control that declared the action with no phrase the
/// plain words for what it is. A control with no long press gets nothing.
fn long_click_label(node: &SemanticsNode) -> Option<String> {
    node.on_long_click.as_ref()?;
    let named = node
        .on_long_click_label
        .clone()
        .filter(|label| !label.trim().is_empty());
    Some(named.unwrap_or_else(|| "long press".to_owned()))
}

/// What a reader lists for a control's magic tap: the verb phrase the app
/// gave, or the plain words for the gesture. A control with no magic tap
/// gets nothing.
fn magic_tap_label(node: &SemanticsNode) -> Option<String> {
    node.on_magic_tap.as_ref()?;
    let named = node
        .on_magic_tap_label
        .clone()
        .filter(|label| !label.trim().is_empty());
    Some(named.unwrap_or_else(|| "magic tap".to_owned()))
}

#[cfg(debug_assertions)]
fn warn_unlabeled(node_id: NodeId) {
    thread_local! {
        static WARNED: std::cell::RefCell<std::collections::HashSet<NodeId>> =
            std::cell::RefCell::new(std::collections::HashSet::new());
    }
    if WARNED.with(|warned| warned.borrow_mut().insert(node_id)) {
        log::warn!(
            "accessibility: control {node_id} takes a click or text but has no label, so a screen reader has nothing to read for it; give it Modifier::content_description or text inside"
        );
    }
}

#[cfg(not(debug_assertions))]
fn warn_unlabeled(_node_id: NodeId) {}

/// Where a selectable control sits inside its group: its place counted from
/// one, how many the group holds, and whether the group runs left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CollectionItem {
    pub(crate) position: usize,
    pub(crate) count: usize,
    pub(crate) horizontal: bool,
}

/// One control as the platforms see it. A scroll container with no label of
/// its own comes through with an empty label: a reader never lands on it, but
/// it is the node the reader pages through.
fn element_for_node(
    node: &SemanticsNode,
    rect: AccessibilityRect,
    label: String,
    clickable: bool,
    live_region: Option<LiveRegionMode>,
) -> AccessibilityElement {
    let role = if node.is_modal {
        AccessibilityRole::Dialog
    } else if let Some(role) = node.widget_role {
        AccessibilityRole::from_widget_role(role)
    } else if node.editable_text {
        AccessibilityRole::TextField
    } else if node.progress.is_some() {
        AccessibilityRole::ProgressBar
    } else if clickable || matches!(node.role, SemanticsRole::Button) {
        AccessibilityRole::Button
    } else {
        AccessibilityRole::StaticText
    };
    AccessibilityElement {
        node_id: node.node_id,
        node_generation: node.node_generation,
        canvas_key: None,
        value: node
            .text
            .clone()
            .or_else(|| node.editable_text.then(|| label.clone()))
            .filter(|_| !node.password)
            .filter(|value| node.editable_text || !value.is_empty()),
        label,
        state_description: node.state_description.clone(),
        click_label: node.on_click_label.clone(),
        long_click_label: long_click_label(node),
        magic_tap_label: magic_tap_label(node),
        input_labels: node.input_labels.clone(),
        language: node.language.clone(),
        bounds: rect,
        role,
        clickable: clickable && node.enabled,
        selected: node.selected,
        toggled: node.toggled,
        enabled: node.enabled,
        custom_actions: node
            .custom_actions
            .iter()
            .map(|action| action.label.clone())
            .collect(),
        focusable: node.focusable,
        tab_stop: true,
        focused: node.focused,
        live_region: live_region.or_else(|| {
            (node.widget_role == Some(SemanticsWidgetRole::Alert))
                .then_some(LiveRegionMode::Assertive)
        }),
        progress: node.progress,
        adjustable: node.set_progress.is_some(),
        vertical_scroll: node.vertical_scroll,
        horizontal_scroll: node.horizontal_scroll,
        scroll_to_index: node.scroll_to_index.is_some(),
        scroll_parent: None,
        collection: node.collection,
        collection_item: None,
        pane_title: node.pane_title.clone(),
        error: node.error.clone(),
        password: node.password,
        expanded: expansion(node),
        dismissable: node.dismiss.is_some(),
        is_modal: node.is_modal,
        text_selection: node
            .text_selection
            .filter(|_| node.editable_text && !node.password)
            .map(|range| (range.start, range.end)),
        multiline: node.multiline,
    }
}

/// Pages a scroll container for a screen reader that asked for the next or
/// the previous page. The deltas are in layout pixels, positive toward the
/// end of the content. Answers whether the container moved.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn scroll_by(root: &SemanticsNode, node_id: NodeId, dx: f32, dy: f32) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    match &node.scroll_by {
        Some(action) => action.invoke(dx, dy),
        None => false,
    }
}

/// Puts the row at `index` in view for a screen reader that asked for it by
/// number, rather than paging until the row shows up. The index counts rows
/// from zero. Answers whether the list moved.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn scroll_to_index(root: &SemanticsNode, node_id: NodeId, index: usize) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    match &node.scroll_to_index {
        Some(action) => action.invoke(index),
        None => false,
    }
}

/// How many rows a list holds, for a platform that has to name the last one.
/// A list that runs down the screen holds its rows in one column, and one that
/// runs across holds them in one row.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn row_count(element: &AccessibilityElement) -> usize {
    if !element.scroll_to_index {
        return 0;
    }
    element
        .collection
        .map_or(0, |collection| collection.rows.max(collection.columns))
}

/// How far one reader page moves a container: most of what it shows, so the
/// last row of one page is still on the next.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn page_delta(element: &AccessibilityElement, forward: bool) -> (f32, f32) {
    let sign = if forward { 1.0 } else { -1.0 };
    if element.vertical_scroll.is_some() {
        (0.0, sign * element.bounds.height * 0.9)
    } else {
        (sign * element.bounds.width * 0.9, 0.0)
    }
}

/// Runs one reader action against the live semantics tree inside the app
/// context and a mutable snapshot, the way the shell runs a click or a key,
/// so the handler may write state and invalidate layout. Marks the shell
/// dirty when the action reports a change, and answers that report.
#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn run_reader_action<R>(
    shell: &mut AppShell<R>,
    act: impl FnOnce(&SemanticsNode) -> bool,
) -> bool
where
    R: Renderer,
    R::Error: Debug,
{
    let context = std::rc::Rc::clone(shell.app_context());
    let changed = context.enter(|| {
        cranpose_core::run_in_mutable_snapshot(|| {
            shell.semantics_tree().is_some_and(|tree| act(tree.root()))
        })
        .unwrap_or(false)
    });
    if changed {
        shell.mark_dirty();
    }
    changed
}

/// Whether a reader's escape gesture has somewhere to go: a dialog or popup
/// on top, or a back handler the app registered.
#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
pub(crate) fn escape_has_a_taker() -> bool {
    cranpose_ui::modal_depth() > 0 || cranpose_services::back_interception_enabled()
}

/// Hands a reader's escape to the app's back handler, the way the platform
/// back gesture reaches it. Answers whether a handler was there to take it.
#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
pub(crate) fn request_back() -> bool {
    if !cranpose_services::back_interception_enabled() {
        return false;
    }
    cranpose_services::push_back_request();
    true
}

/// The nearest scroll container around an element: the scrollable node
/// closest above it in the semantics tree, which holds it even after a page
/// has moved it out of view.
#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn scroll_container_for<'a>(
    elements: &'a [AccessibilityElement],
    element: &AccessibilityElement,
) -> Option<&'a AccessibilityElement> {
    let mut parent = element.scroll_parent;
    while let Some(id) = parent {
        let candidate = elements
            .iter()
            .find(|candidate| candidate.node_id == id && candidate.canvas_key.is_none())?;
        if candidate.vertical_scroll.is_some() || candidate.horizontal_scroll.is_some() {
            return Some(candidate);
        }
        parent = candidate.scroll_parent;
    }
    None
}

fn project_canvas_children(
    node: &SemanticsNode,
    owner: AccessibilityRect,
    live_region: Option<LiveRegionMode>,
    elements: &mut Vec<AccessibilityElement>,
) {
    for child in &node.canvas_children {
        let rect = AccessibilityRect::new(
            owner.x + child.bounds.x,
            owner.y + child.bounds.y,
            child.bounds.width,
            child.bounds.height,
        );
        if !rect.is_visible() || child.label.trim().is_empty() {
            continue;
        }
        let role = match child.role {
            Some(role) => AccessibilityRole::from_widget_role(role),
            None if child.clickable => AccessibilityRole::Button,
            None => AccessibilityRole::StaticText,
        };
        elements.push(AccessibilityElement {
            node_id: node.node_id,
            canvas_key: Some(child.key),
            label: child.label.clone(),
            state_description: child.state_description.clone(),
            click_label: child.on_click_label.clone(),
            bounds: rect,
            role,
            clickable: child.clickable && node.enabled && child.enabled,
            selected: child.selected,
            toggled: child.toggled,
            enabled: node.enabled && child.enabled,
            custom_actions: child
                .custom_actions
                .iter()
                .map(|action| action.label.clone())
                .collect(),
            live_region,
            ..AccessibilityElement::default()
        });
    }
}

#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn perform_custom_action(
    root: &SemanticsNode,
    node_id: NodeId,
    canvas_key: Option<u64>,
    action_index: usize,
) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    let actions = match canvas_key {
        Some(key) => match node.canvas_children.iter().find(|child| child.key == key) {
            Some(child) if child.enabled => &child.custom_actions,
            Some(_) => return false,
            None => return false,
        },
        None => &node.custom_actions,
    };
    match actions.get(action_index) {
        Some(action) => {
            action.invoke();
            true
        }
        None if canvas_key.is_none() => {
            let after = action_index - actions.len();
            match (after, &node.on_long_click, &node.on_magic_tap) {
                (0, Some(long_click), _) => long_click.invoke(),
                (0, None, Some(tap)) | (1, Some(_), Some(tap)) => tap.invoke(),
                _ => false,
            }
        }
        None => false,
    }
}

/// Runs the magic tap a VoiceOver user made on a control, and answers
/// whether the control took it.
#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) fn magic_tap(root: &SemanticsNode, node_id: NodeId) -> bool {
    find_semantics_node(root, node_id).is_some_and(|node| {
        node.on_magic_tap
            .as_ref()
            .is_some_and(cranpose_foundation::SemanticsMagicTap::invoke)
    })
}

/// What a screen reader lists for a node, in the order the platforms number
/// them: the custom actions the app gave, and after them the long press on
/// the three platforms that have no long press of their own. The index a
/// platform hands back is a place in this list, which is what
/// [`perform_custom_action`] reads.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn reader_actions(element: &AccessibilityElement) -> Vec<String> {
    if !element.enabled {
        return Vec::new();
    }
    element
        .custom_actions
        .iter()
        .cloned()
        .chain(element.long_click_label.clone())
        .chain(element.magic_tap_label.clone())
        .collect()
}

/// Moves the value of an adjustable control, for a screen reader that offers
/// its own way to change one: a VoiceOver swipe up, TalkBack's set-progress
/// action, an accesskit value, or an arrow key on the web mirror. `value` is in
/// the control's own range. Answers whether the control took it.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn set_progress(root: &SemanticsNode, node_id: NodeId, value: f32) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    match &node.set_progress {
        Some(action) => action.invoke(value),
        None => false,
    }
}

/// Hands a text field the text a screen reader or a voice tool dictated, and
/// answers whether the field took it.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn set_text(root: &SemanticsNode, node_id: NodeId, text: &str) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    match &node.set_text {
        Some(action) => action.invoke(text),
        None => false,
    }
}

/// Moves the caret of a field, or picks a stretch of its text, for a screen
/// reader. The ends are byte offsets into the field's text, the anchor first
/// and the end that moves second. Answers whether the field took the selection.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn set_text_selection(
    root: &SemanticsNode,
    node_id: NodeId,
    anchor: usize,
    focus: usize,
) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    match &node.set_selection {
        Some(action) => action.invoke(anchor, focus),
        None => false,
    }
}

/// The same move with the ends counted in UTF-16 units, which is how Android
/// and a browser count text.
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
pub(crate) fn set_text_selection_utf16(
    root: &SemanticsNode,
    node_id: NodeId,
    anchor: usize,
    focus: usize,
) -> bool {
    set_text_selection_counted(root, node_id, anchor, focus, byte_offset_for_utf16)
}

/// The same move with the ends counted in characters, which is how accesskit
/// counts text.
#[cfg(any(test, all(feature = "desktop-shell", feature = "renderer-wgpu")))]
pub(crate) fn set_text_selection_chars(
    root: &SemanticsNode,
    node_id: NodeId,
    anchor: usize,
    focus: usize,
) -> bool {
    set_text_selection_counted(root, node_id, anchor, focus, byte_offset_for_chars)
}

/// Moves the selection of a field with its ends counted in some unit of the
/// field's own text, turned into bytes by `byte_offset`.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
fn set_text_selection_counted(
    root: &SemanticsNode,
    node_id: NodeId,
    anchor: usize,
    focus: usize,
    byte_offset: fn(&str, usize) -> usize,
) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    let text = node.text.as_deref().unwrap_or("");
    set_text_selection(
        root,
        node_id,
        byte_offset(text, anchor),
        byte_offset(text, focus),
    )
}

/// The byte offset at or before `byte` that starts a character.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
fn floor_char_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

/// How many UTF-16 units the text holds before the byte offset.
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn utf16_offset(text: &str, byte: usize) -> usize {
    text[..floor_char_boundary(text, byte)]
        .encode_utf16()
        .count()
}

/// The byte offset where the character at a count of UTF-16 units starts. A
/// count past the end, or one that falls inside a surrogate pair, lands on
/// the end of the text or on the next character.
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn byte_offset_for_utf16(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (byte, character) in text.char_indices() {
        if seen >= units {
            return byte;
        }
        seen += character.len_utf16();
    }
    text.len()
}

/// How many characters the text holds before the byte offset.
#[cfg(any(test, all(feature = "desktop-shell", feature = "renderer-wgpu")))]
pub(crate) fn char_offset(text: &str, byte: usize) -> usize {
    text[..floor_char_boundary(text, byte)].chars().count()
}

/// The byte offset where the character at an index starts, or the end of the
/// text for an index past the last character.
#[cfg(any(test, all(feature = "desktop-shell", feature = "renderer-wgpu")))]
pub(crate) fn byte_offset_for_chars(text: &str, characters: usize) -> usize {
    text.char_indices()
        .nth(characters)
        .map_or(text.len(), |(byte, _)| byte)
}

/// Opens or closes a control a screen reader asked to open or to close.
/// Answers whether the control took the ask.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
pub(crate) fn set_expanded(root: &SemanticsNode, node_id: NodeId, open: bool) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    let action = if open { &node.expand } else { &node.collapse };
    match action {
        Some(action) => action.invoke(),
        None => false,
    }
}

/// Sends away the control a screen reader asked to send away. Answers whether
/// the control took the ask.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn dismiss(root: &SemanticsNode, node_id: NodeId) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    match &node.dismiss {
        Some(action) => action.invoke(),
        None => false,
    }
}

/// The name a reader lists the way out under, on the platforms that have no
/// dismiss action of their own and offer it beside the app's own actions.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) const DISMISS_LABEL: &str = "Dismiss";

/// The actions a reader lists for a control: the ones
/// [`reader_actions`] names, and the way out after them when the control
/// declares one. accesskit 0.24 and ARIA carry no dismiss action, so both
/// reach it as one more named action.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn listed_actions(element: &AccessibilityElement) -> Vec<String> {
    reader_actions(element)
        .into_iter()
        .chain((element.enabled && element.dismissable).then(|| DISMISS_LABEL.to_owned()))
        .collect()
}

/// Runs the action a reader picked out of a control's listed actions: one the
/// app named, or the way out that sits after them.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn perform_listed_action(
    root: &SemanticsNode,
    node_id: NodeId,
    canvas_key: Option<u64>,
    named: usize,
    index: usize,
) -> bool {
    if index == named {
        return dismiss(root, node_id);
    }
    perform_custom_action(root, node_id, canvas_key, index)
}

/// Runs the long press a screen reader asked a control for, and answers
/// whether the control took the ask. Compose's `onLongClick` action.
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
pub(crate) fn long_click(root: &SemanticsNode, node_id: NodeId) -> bool {
    let Some(node) = find_semantics_node(root, node_id) else {
        return false;
    };
    match &node.on_long_click {
        Some(action) => action.invoke(),
        None => false,
    }
}

/// The word a reader that carries no open-or-closed flag of its own says
/// about a control it can open.
#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) fn expansion_word(element: &AccessibilityElement) -> Option<&'static str> {
    element
        .expanded
        .map(|open| if open { "expanded" } else { "collapsed" })
}

/// The value one screen reader step away from the one the control holds now,
/// for the readers that offer a step up and a step down rather than a value.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) fn stepped_value(progress: &ProgressBarRangeInfo, up: bool) -> f32 {
    let step = progress.step();
    let next = if up {
        progress.current + step
    } else {
        progress.current - step
    };
    let low = progress.start.min(progress.end);
    let high = progress.start.max(progress.end);
    next.clamp(low, high)
}

#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn focus_node(root: &SemanticsNode, node_id: NodeId) -> bool {
    find_semantics_node(root, node_id)
        .is_some_and(|node| node.focusable && cranpose_ui::request_focus_from_platform(node_id))
}

/// Text the app asked a screen reader to read out, through
/// [`cranpose_ui::Announcer`]. Every platform bridge takes this queue once a
/// frame.
/// Installs the display options the system reports and asks for a root
/// render when they changed, so the theme, the glass and the animations read
/// them. A backend that scales text calls `set_font_scale` on the shell
/// beside this.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn apply_accessibility_options<R>(
    shell: &mut AppShell<R>,
    options: cranpose_services::AccessibilityOptions,
) -> bool
where
    R: Renderer,
    R::Error: Debug,
{
    let changed = cranpose_services::set_platform_accessibility_options(options);
    if changed {
        shell.request_root_render();
    }
    changed
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn drain_app_announcements() -> Vec<Announcement> {
    cranpose_ui::drain_announcements()
}

/// Text that changed inside a live region, for the two platforms with no live
/// region of their own: iOS has no such notion, and Android ignores a live
/// region set on a virtual view. On desktop the accesskit `live` field carries
/// it, and on web `aria-live` does, so those two bridges leave this alone and
/// let the screen reader do the reading.
///
/// An element that was not in `previous` counts as changed, so an error text
/// that appears next to a field is read out. A first snapshot reads nothing:
/// every element is new then, and a blind user would hear the whole screen
/// twice.
#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn live_region_announcements(
    previous: &[AccessibilityElement],
    current: &[AccessibilityElement],
) -> Vec<Announcement> {
    if previous.is_empty() {
        return Vec::new();
    }
    let mut announcements = Vec::new();
    for element in current {
        let Some(mode) = element.live_region else {
            continue;
        };
        let text = spoken_text(element);
        if text.trim().is_empty() {
            continue;
        }
        let was = previous
            .iter()
            .find(|other| other.identity_key() == element.identity_key())
            .map(spoken_text);
        if was.as_deref() != Some(text.as_str()) {
            announcements.push(Announcement { text, mode });
        }
    }
    announcements
}

/// The title of each pane that opened or changed since the last publish, so a
/// reader hears where it is when the app moves on. Nothing on the first
/// publish, which would read the first screen's title over its content.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn pane_title_announcements(
    previous: &[AccessibilityElement],
    current: &[AccessibilityElement],
) -> Vec<Announcement> {
    if previous.is_empty() {
        return Vec::new();
    }
    current
        .iter()
        .filter_map(|element| {
            let title = element
                .pane_title
                .as_deref()
                .filter(|title| !title.trim().is_empty())?;
            let was = previous
                .iter()
                .find(|other| other.identity_key() == element.identity_key())
                .and_then(|other| other.pane_title.as_deref());
            (was != Some(title)).then(|| Announcement {
                text: title.to_owned(),
                mode: LiveRegionMode::Polite,
            })
        })
        .collect()
}

#[cfg(any(
    test,
    feature = "robot",
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
fn spoken_text(element: &AccessibilityElement) -> String {
    let mut parts = vec![element.label.clone()];
    if let Some(value) = &element.value
        && value != &element.label
    {
        parts.push(value.clone());
    }
    if let Some(state) = &element.state_description {
        parts.push(state.clone());
    }
    parts.extend(error_text(element));
    parts.retain(|part| !part.trim().is_empty());
    parts.join(", ")
}

/// What a reader hears about a control whose content is wrong: "invalid" and
/// the reason the app gave.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn error_text(element: &AccessibilityElement) -> Option<String> {
    element
        .error
        .as_deref()
        .filter(|error| !error.trim().is_empty())
        .map(|error| format!("invalid, {error}"))
}

/// The state a reader hears for a control, with the reason its content is
/// wrong after it, for the platforms that carry both in one description.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn state_with_error(element: &AccessibilityElement) -> Option<String> {
    let parts: Vec<String> = element
        .state_description
        .clone()
        .into_iter()
        .chain(error_text(element))
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Whether two publications of one control read the same: the words, a
/// toggle, a pick and a value.
#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
fn speaks_the_same(was: &AccessibilityElement, now: &AccessibilityElement) -> bool {
    let same_sources = was.label == now.label
        && was.value == now.value
        && was.state_description == now.state_description
        && was.error == now.error;
    (same_sources || spoken_text(was) == spoken_text(now))
        && was.toggled == now.toggled
        && was.selected == now.selected
        && was.progress == now.progress
}

/// For each element of `current`, whether it was published before and now
/// says something else: a toggle that flipped, a counter that moved on, a
/// value a reader just set. A reader speaks the one under its cursor again.
#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
pub(crate) fn spoken_changes(
    previous: &[AccessibilityElement],
    current: &[AccessibilityElement],
) -> Vec<bool> {
    let mut published: HashMap<_, &AccessibilityElement> = HashMap::default();
    for element in previous {
        published.entry(element.identity_key()).or_insert(element);
    }
    current
        .iter()
        .map(|element| {
            published
                .get(&element.identity_key())
                .is_some_and(|was| !speaks_the_same(was, element))
        })
        .collect()
}

#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn find_semantics_node(node: &SemanticsNode, node_id: NodeId) -> Option<&SemanticsNode> {
    if node.hidden {
        return None;
    }
    if node.node_id == node_id {
        return node.enabled.then_some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_semantics_node(child, node_id))
}

#[cfg(test)]
pub(crate) fn element_with(node_id: NodeId, canvas_key: Option<u64>) -> AccessibilityElement {
    AccessibilityElement {
        node_id,
        canvas_key,
        label: "Row".into(),
        bounds: AccessibilityRect::new(0.0, 0.0, 10.0, 10.0),
        ..AccessibilityElement::default()
    }
}

/// The word a reader says for each role, for the robot's spoken tree. Plain
/// text has no word: its name is the whole of what a reader says.
#[cfg(any(test, feature = "robot", target_os = "ios"))]
const SPOKEN_ROLES: [(AccessibilityRole, &str); 24] = [
    (AccessibilityRole::Button, "button"),
    (AccessibilityRole::StaticText, ""),
    (AccessibilityRole::TextField, "text field"),
    (AccessibilityRole::Checkbox, "checkbox"),
    (AccessibilityRole::Switch, "switch"),
    (AccessibilityRole::RadioButton, "radio button"),
    (AccessibilityRole::Tab, "tab"),
    (AccessibilityRole::Image, "image"),
    (AccessibilityRole::Header, "heading"),
    (AccessibilityRole::Dialog, "dialog"),
    (AccessibilityRole::DropdownList, "pop up button"),
    (AccessibilityRole::ValuePicker, "picker"),
    (AccessibilityRole::Link, "link"),
    (AccessibilityRole::SearchField, "search field"),
    (AccessibilityRole::ProgressBar, "progress bar"),
    (AccessibilityRole::ToggleButton, "toggle button"),
    (AccessibilityRole::Alert, "alert"),
    (AccessibilityRole::Toolbar, "toolbar"),
    (AccessibilityRole::Menu, "menu"),
    (AccessibilityRole::MenuItem, "menu item"),
    (AccessibilityRole::TabBar, "tab bar"),
    (AccessibilityRole::List, "list"),
    (AccessibilityRole::ListItem, "list item"),
    (AccessibilityRole::RadioGroup, "radio group"),
];

/// One control the way a reader speaks it: the name, the role, the state,
/// the value and the actions it offers, in the order VoiceOver says them.
#[cfg(any(test, feature = "robot", target_os = "ios"))]
pub(crate) fn spoken_line(element: &AccessibilityElement) -> String {
    let role_word = SPOKEN_ROLES
        .iter()
        .find(|(role, _)| *role == element.role)
        .map_or("", |(_, word)| *word);
    let name = match (&element.pane_title, element.label.is_empty()) {
        (Some(title), true) => format!("{title}, pane"),
        _ => spoken_text(element),
    };
    let toggle_words = if element.role == AccessibilityRole::Switch {
        ("on", "off")
    } else {
        ("checked", "not checked")
    };
    let actions: Vec<&str> = element
        .custom_actions
        .iter()
        .map(String::as_str)
        .chain(element.long_click_label.as_deref())
        .chain(element.magic_tap_label.as_deref())
        .collect();
    let mut parts: Vec<String> = vec![name, role_word.to_string()];
    parts.extend(
        checked_state(element)
            .map(|on| if on { toggle_words.0 } else { toggle_words.1 }.to_string()),
    );
    parts.extend(
        element
            .selected
            .filter(|picked| *picked && element.role != AccessibilityRole::RadioButton)
            .map(|_| "selected".to_string()),
    );
    parts.extend(element.expanded.map(|open| {
        if open {
            "expanded".to_string()
        } else {
            "collapsed".to_string()
        }
    }));
    if element.state_description.is_none() {
        parts.extend(element.progress.as_ref().and_then(spoken_percent));
    }
    parts.extend((!element.enabled).then(|| "dimmed".to_string()));
    parts.extend(element.focused.then(|| "focused".to_string()));
    parts.extend((!actions.is_empty()).then(|| format!("actions: {}", actions.join(", "))));
    parts.retain(|part| !part.is_empty());
    parts.join(", ")
}

/// Writes every control the way a reader speaks it to the log, one line
/// each under the target `cranpose::spoken_tree`, when that target is on at
/// debug level: `RUST_LOG=cranpose::spoken_tree=debug`. A platform bridge
/// calls it on every change, so a person reads a device's screen from the
/// console the way the robot's `spoken_tree` prints it.
#[cfg(target_os = "ios")]
pub(crate) fn log_spoken_tree(elements: &[AccessibilityElement]) {
    const TARGET: &str = "cranpose::spoken_tree";
    if !log::log_enabled!(target: TARGET, log::Level::Debug) {
        return;
    }
    log::debug!(target: TARGET, "--- {} controls ---", elements.len());
    for line in elements
        .iter()
        .map(spoken_line)
        .filter(|line| !line.is_empty())
    {
        log::debug!(target: TARGET, "{line}");
    }
}

#[cfg(any(test, feature = "robot", target_os = "ios"))]
fn spoken_percent(progress: &ProgressBarRangeInfo) -> Option<String> {
    let span = progress.end - progress.start;
    (span > 0.0).then(|| {
        let percent = ((progress.current - progress.start) / span * 100.0).round();
        format!("{percent} percent")
    })
}

/// Every control on the screen the way a reader speaks it, one per line, in
/// reading order: what the robot's `spoken_tree` prints.
#[cfg(feature = "robot")]
pub(crate) fn spoken_tree<R>(shell: &mut AppShell<R>) -> String
where
    R: Renderer,
    R::Error: Debug,
{
    snapshot(shell)
        .iter()
        .map(spoken_line)
        .filter(|line| !line.is_empty())
        .fold(String::new(), |mut tree, line| {
            tree.push_str(&line);
            tree.push('\n');
            tree
        })
}

#[cfg(test)]
#[path = "tests/accessibility.rs"]
mod tests;

#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) fn voiceover_same_structure(
    current: &[AccessibilityElement],
    next: &[AccessibilityElement],
) -> bool {
    current.len() == next.len()
        && current.iter().zip(next).all(|(current, next)| {
            current.identity_key() == next.identity_key()
                && current.label.is_empty() == next.label.is_empty()
                && current.role == next.role
                && current.clickable == next.clickable
                && (!current.role.is_text_field() || current.focused == next.focused)
        })
}

#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) fn voiceover_replacement_focus(
    elements: &[AccessibilityElement],
    ids: &[i32],
    cursor: Option<i32>,
) -> Option<i32> {
    let cursor = cursor?;
    if ids.contains(&cursor) {
        return None;
    }
    elements
        .iter()
        .zip(ids)
        .find(|(element, _)| !element.label.trim().is_empty() && element.bounds.is_visible())
        .map(|(_, id)| *id)
}

#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) fn voiceover_value(element: &AccessibilityElement) -> Option<String> {
    let mut parts = Vec::new();
    if element.password {
        parts.push("password".to_owned());
    } else if let Some(value) = &element.value
        && (element.role.is_text_field() || value != &element.label)
        && !value.trim().is_empty()
    {
        parts.push(value.clone());
    }
    if element
        .state_description
        .as_deref()
        .is_none_or(|state| state.trim().is_empty())
    {
        if let Some(checked) = checked_state(element) {
            let word = match (element.role, checked) {
                (AccessibilityRole::Switch, true) => "on",
                (AccessibilityRole::Switch, false) => "off",
                (_, true) => "checked",
                (_, false) => "not checked",
            };
            parts.push(word.to_owned());
        }
        if let Some(progress) = element.progress {
            parts.push(progress.current.to_string());
        }
    }
    parts.extend(expansion_word(element).map(str::to_owned));
    parts.extend(state_with_error(element));
    if let Some(item) = element.collection_item {
        parts.push(format!("{} of {}", item.position, item.count));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}
