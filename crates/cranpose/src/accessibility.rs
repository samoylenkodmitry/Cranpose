use std::{borrow::Cow, fmt::Debug};

use cranpose_app_shell::AppShell;
use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::Renderer;
use cranpose_ui::{
    Announcement, CollectionInfo, LayoutBox, LiveRegionMode, ProgressBarRangeInfo, ScrollAxisRange,
    SemanticsAction, SemanticsNode, SemanticsRole, SemanticsWidgetRole,
};

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
}

/// The role a reader names for each role an app declares. A table rather than
/// a match, so the platforms that name roles with plain data read theirs the
/// same way; the length assertion below keeps it whole when a role is added.
const WIDGET_ROLES: [(SemanticsWidgetRole, AccessibilityRole); 21] = [
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
];

const _: () = assert!(WIDGET_ROLES.len() == SemanticsWidgetRole::ListItem as usize + 1);

/// The ARIA role of each role, for the web mirror.
#[cfg(any(
    test,
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
const ARIA_ROLES: [(AccessibilityRole, &str); 23] = [
    (AccessibilityRole::Button, "button"),
    (AccessibilityRole::StaticText, "text"),
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
];

/// The number the Android host reads each role as; the host's `className()`
/// and `roleDescription()` turn it back into what TalkBack says.
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
const ANDROID_ROLE_CODES: [(AccessibilityRole, i32); 23] = [
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

impl AccessibilityRole {
    /// Every role, for the tables that name a role on a platform and the
    /// tests that check none is left out.
    pub(crate) const ALL: [Self; 23] = [
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
        role_entry(&ARIA_ROLES, self, "text")
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
        matches!(self, Self::Toolbar | Self::Menu | Self::TabBar | Self::List)
    }
}

const _: () = assert!(AccessibilityRole::ALL.len() == AccessibilityRole::ListItem as usize + 1);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccessibilityElement {
    pub(crate) node_id: NodeId,
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
    /// Where the caret of an editable field sits, or which stretch of its
    /// text is picked: the anchor and the end that moves, as byte offsets into
    /// `value`. A field that holds a secret publishes none.
    pub(crate) text_selection: Option<(usize, usize)>,
}

impl Default for AccessibilityElement {
    fn default() -> Self {
        Self {
            node_id: 0,
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
            text_selection: None,
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
    *seen_revision = Some(revision);
    Some(snapshot(shell))
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
    let mut bounds = HashMap::new();
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

pub(crate) fn element_ids(elements: &[AccessibilityElement]) -> Vec<i32> {
    let mut assigned: Vec<i32> = Vec::with_capacity(elements.len());
    for element in elements {
        let mut id = element_id(element.node_id, element.canvas_key);
        while assigned.contains(&id) {
            id = if id == i32::MAX { 1 } else { id + 1 };
        }
        assigned.push(id);
    }
    assigned
}

fn element_id(node_id: NodeId, canvas_key: Option<u64>) -> i32 {
    let mixed = match canvas_key {
        None => node_id as u64,
        Some(key) => {
            (node_id as u64)
                .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                .rotate_left(17)
                ^ key.wrapping_mul(0xd6e8_feb8_6659_fd93)
        }
    };
    ((mixed & 0x7fff_ffff) as i32).max(1)
}

#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
pub(crate) fn resolve_element_id(
    elements: &[AccessibilityElement],
    id: i32,
) -> Option<(NodeId, Option<u64>)> {
    element_ids(elements)
        .into_iter()
        .zip(elements)
        .find(|(assigned, _)| *assigned == id)
        .map(|(_, element)| (element.node_id, element.canvas_key))
}

fn project_semantics(
    root: &SemanticsNode,
    bounds: &HashMap<NodeId, AccessibilityRect>,
) -> Vec<AccessibilityElement> {
    let mut elements = Vec::new();
    project_node(root, bounds, false, None, None, &mut elements);
    elements
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
    let merges = actionable || node.merge_descendants;
    let label = published_label(node, merges);
    let rect = bounds.get(&node.node_id).copied().unwrap_or_default();

    let container = is_container(node);
    if let Some(label) = label
        && rect.is_visible()
        && (merges || !suppress_static_text)
    {
        elements.push(element_for_node(
            node,
            rect,
            label.into_owned(),
            clickable,
            live_region,
        ));
    } else if publishes_unlabeled(node) && rect.is_visible() {
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
        element.scroll_parent = inherited_scroll;
    }

    let suppress_children = suppress_static_text || merges;
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
        || node
            .widget_role
            .is_some_and(|role| AccessibilityRole::from_widget_role(role).is_named_container())
}

/// A node published with no label of its own: a container, or the root of a
/// pane whose title a reader hears.
fn publishes_unlabeled(node: &SemanticsNode) -> bool {
    is_container(node) || node.pane_title.is_some()
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
    for child in reading_order(node) {
        project_node(
            child,
            bounds,
            suppress_static_text,
            live_region,
            scroll_for_children,
            elements,
        );
    }
    if node.selectable_group {
        number_group(node.node_id, first_child, elements);
    }
}

/// The order a screen reader visits the nodes under a container: the order
/// the app laid them out, with any node the app gave a traversal index moved
/// to where that index puts it. The sort keeps the laid-out order of nodes
/// that share an index. Compose's `traversalIndex`.
fn reading_order(node: &SemanticsNode) -> Vec<&SemanticsNode> {
    let mut order: Vec<&SemanticsNode> = node.children.iter().collect();
    if order.iter().any(|child| child.traversal_index != 0.0) {
        order.sort_by(|left, right| left.traversal_index.total_cmp(&right.traversal_index));
    }
    order
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
    if let Some(element) = elements
        .iter_mut()
        .find(|element| element.node_id == group && element.canvas_key.is_none())
    {
        element.collection = Some(CollectionInfo { rows, columns });
    }
}

/// Names, once per node and only in a debug build, a control that takes a
/// click or text but reaches no reader: it has no label and no text inside,
/// so a screen reader has nothing to say for it.
/// The label a reader hears for a node: its own, or for a control or a merged
/// row the text under it; an editable field with nothing to read still gets
/// an empty one.
fn published_label(node: &SemanticsNode, merges: bool) -> Option<Cow<'_, str>> {
    if node.password {
        return password_label(node);
    }
    let own_label = node_label(node).map(Cow::Borrowed);
    let label = if merges {
        own_label.or_else(|| descendant_label(node).map(Cow::Owned))
    } else {
        own_label
    };
    label
        .filter(|label| !label.trim().is_empty())
        .or_else(|| unnamed_field_label(node))
}

/// A field that holds a secret never reads its text out: the name the app
/// gave it stands, and with no name a reader hears "password" rather than
/// the text the field put in as a stand-in for a name.
fn password_label(node: &SemanticsNode) -> Option<Cow<'_, str>> {
    let named = node_label(node)
        .filter(|name| !name.trim().is_empty())
        .filter(|name| Some(*name) != node.text.as_deref());
    Some(named.map_or(Cow::Borrowed("password"), Cow::Borrowed))
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

/// An editable field with no name and no text is still a stop for a reader,
/// which hears "text field" and nothing else; a debug build says so.
fn unnamed_field_label(node: &SemanticsNode) -> Option<Cow<'_, str>> {
    if !node.editable_text {
        return None;
    }
    warn_unlabeled(node.node_id);
    Some(Cow::Borrowed(""))
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
    let role = if let Some(role) = node.widget_role {
        AccessibilityRole::from_widget_role(role)
    } else if node.editable_text {
        AccessibilityRole::TextField
    } else if clickable || matches!(node.role, SemanticsRole::Button) {
        AccessibilityRole::Button
    } else {
        AccessibilityRole::StaticText
    };
    AccessibilityElement {
        node_id: node.node_id,
        canvas_key: None,
        value: node
            .text
            .clone()
            .or_else(|| node.editable_text.then(|| label.clone()))
            .filter(|_| !node.password),
        label,
        state_description: node.state_description.clone(),
        click_label: node.on_click_label.clone(),
        long_click_label: long_click_label(node),
        magic_tap_label: magic_tap_label(node),
        input_labels: node.input_labels.clone(),
        language: node.language.clone(),
        bounds: rect,
        role,
        clickable,
        selected: node.selected,
        toggled: node.toggled,
        enabled: node.enabled,
        custom_actions: node
            .custom_actions
            .iter()
            .map(|action| action.label.clone())
            .collect(),
        focusable: node.focusable,
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
        text_selection: node
            .text_selection
            .filter(|_| node.editable_text && !node.password)
            .map(|range| (range.start, range.end)),
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
            clickable: child.clickable,
            selected: child.selected,
            toggled: child.toggled,
            enabled: child.enabled,
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
            Some(child) => &child.custom_actions,
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
    find_semantics_node(root, node_id)
        .is_some_and(|node| node.on_magic_tap.as_ref().is_some_and(|tap| tap.invoke()))
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
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
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
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
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
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
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
        .chain(element.dismissable.then(|| DISMISS_LABEL.to_owned()))
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

/// Moves app focus onto the node a platform's accessibility layer asked for,
/// so a screen reader and the app agree on what holds focus. Answers whether
/// focus moved.
pub(crate) fn focus_node(node_id: NodeId) -> bool {
    cranpose_ui::request_focus_from_platform(node_id)
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
            .find(|other| {
                other.node_id == element.node_id && other.canvas_key == element.canvas_key
            })
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
                .find(|other| other.node_id == element.node_id)
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
    spoken_text(was) == spoken_text(now)
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
    current
        .iter()
        .map(|element| {
            previous
                .iter()
                .find(|other| {
                    other.node_id == element.node_id && other.canvas_key == element.canvas_key
                })
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
fn find_semantics_node(node: &SemanticsNode, node_id: NodeId) -> Option<&SemanticsNode> {
    if node.node_id == node_id {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_semantics_node(child, node_id))
}

fn node_label(node: &SemanticsNode) -> Option<&str> {
    node.description.as_deref().or(match &node.role {
        SemanticsRole::Text { value } => Some(value.as_str()),
        _ => None,
    })
}

fn descendant_label(node: &SemanticsNode) -> Option<String> {
    let mut labels = Vec::new();
    collect_descendant_labels(node, &mut labels);
    (!labels.is_empty()).then(|| labels.join(", "))
}

fn collect_descendant_labels<'a>(node: &'a SemanticsNode, labels: &mut Vec<&'a str>) {
    for child in node.children.iter().filter(|child| !child.hidden) {
        if let Some(label) = node_label(child) {
            if !label.trim().is_empty() && !labels.contains(&label) {
                labels.push(label);
            }
        } else {
            collect_descendant_labels(child, labels);
        }
    }
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
#[cfg(any(test, feature = "robot"))]
const SPOKEN_ROLES: [(AccessibilityRole, &str); 23] = [
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
];

/// One control the way a reader speaks it: the name, the role, the state,
/// the value and the actions it offers, in the order VoiceOver says them.
#[cfg(any(test, feature = "robot"))]
pub(crate) fn spoken_line(element: &AccessibilityElement) -> String {
    let role_word = SPOKEN_ROLES
        .iter()
        .find(|(role, _)| *role == element.role)
        .map(|(_, word)| *word)
        .unwrap_or("");
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
        element
            .toggled
            .map(|on| if on { toggle_words.0 } else { toggle_words.1 }.to_string()),
    );
    parts.extend(
        element
            .selected
            .filter(|picked| *picked)
            .map(|_| "selected".to_string()),
    );
    parts.extend(element.expanded.map(|open| {
        if open {
            "expanded".to_string()
        } else {
            "collapsed".to_string()
        }
    }));
    parts.extend(element.progress.as_ref().and_then(spoken_percent));
    parts.extend((!element.enabled).then(|| "dimmed".to_string()));
    parts.extend(element.focused.then(|| "focused".to_string()));
    parts.extend((!actions.is_empty()).then(|| format!("actions: {}", actions.join(", "))));
    parts.retain(|part| !part.is_empty());
    parts.join(", ")
}

#[cfg(any(test, feature = "robot"))]
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
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use cranpose_core::NodeId;
    use cranpose_ui::{
        CanvasSemanticsNode, SemanticsAction, SemanticsCallback, SemanticsCustomAction,
        SemanticsNode, SemanticsRole, SemanticsWidgetRole,
    };

    use super::*;

    fn node(
        node_id: NodeId,
        role: SemanticsRole,
        actions: Vec<SemanticsAction>,
        description: Option<&str>,
        children: Vec<SemanticsNode>,
    ) -> SemanticsNode {
        SemanticsNode {
            node_id,
            role,
            actions,
            children,
            description: description.map(str::to_owned),
            ..SemanticsNode::default()
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> cranpose_ui::Rect {
        cranpose_ui::Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn drawn_controls_get_distinct_ids_that_do_not_move_with_list_position() {
        let rows: Vec<_> = (0..24).map(|key| element_with(7, Some(key))).collect();
        let ids = element_ids(&rows);

        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "ids collided: {ids:?}");
        assert!(ids.iter().all(|id| *id > 0));

        let scrolled = element_ids(&rows[1..]);
        assert_eq!(scrolled, ids[1..]);

        assert_eq!(element_ids(&[element_with(7, None)]), vec![7]);
        assert!(
            !ids.contains(&7),
            "a drawn control took the layout node's id"
        );

        let across = element_ids(&[element_with(7, Some(3)), element_with(8, Some(3))]);
        assert_ne!(across[0], across[1]);
    }

    #[test]
    fn an_element_id_resolves_back_to_the_element_that_published_it() {
        let elements = vec![
            element_with(7, None),
            element_with(7, Some(3)),
            element_with(9, Some(3)),
        ];
        let ids = element_ids(&elements);

        assert_eq!(resolve_element_id(&elements, ids[1]), Some((7, Some(3))));
        assert_eq!(resolve_element_id(&elements, ids[2]), Some((9, Some(3))));
        assert_eq!(resolve_element_id(&elements, ids[0]), Some((7, None)));
        assert_eq!(resolve_element_id(&elements, -12), None);
    }

    #[test]
    fn a_layout_node_element_keeps_its_cranpose_node_id() {
        assert_eq!(element_id(42, None), 42);
        assert_eq!(element_id(0, None), 1);
        assert_eq!(element_id(42, None), element_id(42, None));
    }

    #[test]
    fn actionable_parent_uses_descendant_text_without_duplicate_leaf() {
        let button_id = 2;
        let root = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![
                node(
                    button_id,
                    SemanticsRole::Button,
                    vec![SemanticsAction::Click {
                        handler: SemanticsCallback::new(button_id),
                    }],
                    None,
                    vec![node(
                        3,
                        SemanticsRole::Text {
                            value: "Library".into(),
                        },
                        Vec::new(),
                        None,
                        Vec::new(),
                    )],
                ),
                node(
                    4,
                    SemanticsRole::Text {
                        value: "Receipts".into(),
                    },
                    Vec::new(),
                    None,
                    Vec::new(),
                ),
            ],
        );
        let bounds = HashMap::from_iter([
            (button_id, AccessibilityRect::new(8.0, 700.0, 80.0, 64.0)),
            (3, AccessibilityRect::new(20.0, 712.0, 50.0, 20.0)),
            (4, AccessibilityRect::new(16.0, 80.0, 100.0, 28.0)),
        ]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].node_id, button_id);
        assert_eq!(projected[0].label, "Library");
        assert_eq!(projected[0].role, AccessibilityRole::Button);
        assert_eq!(projected[0].bounds.center(), (48.0, 732.0));
        assert_eq!(projected[1].label, "Receipts");
        assert_eq!(projected[1].role, AccessibilityRole::StaticText);
    }

    #[test]
    fn drawn_controls_become_elements_positioned_inside_their_canvas() {
        let canvas_id = 7;
        let mut root = node(
            canvas_id,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            Vec::new(),
        );
        root.canvas_children = vec![
            CanvasSemanticsNode::text(1, rect(0.0, 0.0, 200.0, 30.0), "SETTINGS")
                .with_role(SemanticsWidgetRole::Header),
            CanvasSemanticsNode::control(2, rect(0.0, 40.0, 200.0, 52.0), "Haptics")
                .with_role(SemanticsWidgetRole::Switch)
                .with_toggled(true)
                .with_state_description("On"),
            CanvasSemanticsNode::control(3, rect(0.0, 100.0, 200.0, 52.0), "Reset progress")
                .with_click_label("Reset")
                .with_enabled(false),
        ];
        let bounds =
            HashMap::from_iter([(canvas_id, AccessibilityRect::new(20.0, 100.0, 200.0, 300.0))]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected.len(), 3);
        assert!(projected.iter().all(|element| element.node_id == canvas_id));
        assert_eq!(
            projected
                .iter()
                .map(|element| element.canvas_key)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3)]
        );

        assert_eq!(projected[0].role, AccessibilityRole::Header);
        assert!(!projected[0].clickable);

        assert_eq!(projected[1].label, "Haptics");
        assert_eq!(projected[1].role, AccessibilityRole::Switch);
        assert_eq!(projected[1].toggled, Some(true));
        assert_eq!(projected[1].state_description.as_deref(), Some("On"));
        assert!(projected[1].clickable);
        assert_eq!(projected[1].bounds.center(), (120.0, 166.0));

        assert_eq!(projected[2].click_label.as_deref(), Some("Reset"));
        assert!(!projected[2].enabled);
    }

    fn click(node_id: NodeId) -> SemanticsAction {
        SemanticsAction::Click {
            handler: SemanticsCallback::new(node_id),
        }
    }

    fn tab(node_id: NodeId, label: &str, picked: bool) -> SemanticsNode {
        let mut tab = node(
            node_id,
            SemanticsRole::Layout,
            vec![click(node_id)],
            Some(label),
            Vec::new(),
        );
        tab.selected = Some(picked);
        tab
    }

    fn tab_group(node_id: NodeId, tabs: Vec<SemanticsNode>) -> SemanticsNode {
        let mut group = node(node_id, SemanticsRole::Layout, Vec::new(), None, tabs);
        group.selectable_group = true;
        group
    }

    #[test]
    fn tabs_in_a_group_know_their_place() {
        let root = tab_group(
            1,
            vec![
                tab(2, "Home", true),
                tab(3, "Library", false),
                tab(4, "Settings", false),
            ],
        );
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 60.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 100.0, 60.0)),
            (3, AccessibilityRect::new(100.0, 0.0, 100.0, 60.0)),
            (4, AccessibilityRect::new(200.0, 0.0, 100.0, 60.0)),
        ]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(
            projected[0].collection,
            Some(CollectionInfo {
                rows: 1,
                columns: 3
            })
        );
        let places: Vec<_> = projected[1..]
            .iter()
            .map(|tab| tab.collection_item.expect("a tab knows its place"))
            .collect();
        assert_eq!(
            places,
            vec![
                CollectionItem {
                    position: 1,
                    count: 3,
                    horizontal: true
                },
                CollectionItem {
                    position: 2,
                    count: 3,
                    horizontal: true
                },
                CollectionItem {
                    position: 3,
                    count: 3,
                    horizontal: true
                },
            ]
        );
    }

    #[test]
    fn a_tab_pages_the_list_around_its_group() {
        let mut list = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![tab_group(2, vec![tab(3, "Home", true)])],
        );
        list.vertical_scroll = Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false));
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 600.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 60.0)),
            (3, AccessibilityRect::new(0.0, 0.0, 100.0, 60.0)),
        ]);

        let projected = project_semantics(&root_of(list), &bounds);
        let tab = projected.last().expect("the tab is published");

        assert_eq!(tab.scroll_parent, Some(2), "the tab sits under its group");
        assert_eq!(
            scroll_container_for(&projected, tab).map(|list| list.node_id),
            Some(1),
            "a page from the tab reaches the list above the group"
        );
    }

    fn root_of(node: SemanticsNode) -> SemanticsNode {
        node
    }

    fn text_node(node_id: NodeId, label: &str) -> SemanticsNode {
        node(
            node_id,
            SemanticsRole::Text {
                value: label.to_owned(),
            },
            Vec::new(),
            None,
            Vec::new(),
        )
    }

    fn one_row_tree(row: SemanticsNode) -> (SemanticsNode, HashMap<NodeId, AccessibilityRect>) {
        let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![row]);
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        ]);
        (root, bounds)
    }

    #[test]
    fn a_pane_is_published_with_its_title_and_no_label() {
        let mut screen = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![node(
                2,
                SemanticsRole::Text {
                    value: "Milk".into(),
                },
                Vec::new(),
                None,
                Vec::new(),
            )],
        );
        screen.pane_title = Some("Library".into());
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 600.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        ]);

        let projected = project_semantics(&screen, &bounds);

        assert_eq!(projected.len(), 2, "the pane and its text: {projected:?}");
        assert_eq!(projected[0].label, "");
        assert_eq!(projected[0].pane_title.as_deref(), Some("Library"));
        assert_eq!(projected[1].label, "Milk");
        assert_eq!(projected[1].scroll_parent, None, "a pane is no container");
    }

    #[test]
    fn a_merged_row_is_one_stop() {
        let mut row = node(
            2,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![
                node(
                    3,
                    SemanticsRole::Text {
                        value: "Milk".into(),
                    },
                    Vec::new(),
                    None,
                    Vec::new(),
                ),
                node(
                    4,
                    SemanticsRole::Text { value: "2".into() },
                    Vec::new(),
                    None,
                    Vec::new(),
                ),
                node(
                    5,
                    SemanticsRole::Text {
                        value: "3.40".into(),
                    },
                    Vec::new(),
                    None,
                    Vec::new(),
                ),
            ],
        );
        row.merge_descendants = true;
        let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![row]);
        let bounds = HashMap::from_iter(
            (1..=5).map(|id| (id, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0))),
        );

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected.len(), 1, "the row is one stop: {projected:?}");
        assert_eq!(projected[0].label, "Milk, 2, 3.40");
        assert_eq!(projected[0].role, AccessibilityRole::StaticText);
        assert!(!projected[0].clickable);
    }

    #[test]
    fn a_hidden_node_and_everything_under_it_stay_out() {
        let mut placeholder = node(
            2,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Item"),
            Vec::new(),
        );
        placeholder.hidden = true;
        let mut root = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![placeholder],
        );
        root.children[0].children.push(node(
            3,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Under it"),
            Vec::new(),
        ));
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
            (3, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        ]);

        let projected = project_semantics(&root, &bounds);

        assert!(
            projected.is_empty(),
            "a hidden node is not published: {projected:?}"
        );
    }

    fn projected_field(
        name: Option<&str>,
        text: &str,
        password: bool,
    ) -> Vec<AccessibilityElement> {
        let mut field = node(2, SemanticsRole::Layout, Vec::new(), name, Vec::new());
        field.editable_text = true;
        field.password = password;
        field.text = Some(text.to_owned());
        let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![field]);
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        ]);
        project_semantics(&root, &bounds)
    }

    #[test]
    fn an_empty_text_field_is_still_a_stop() {
        let projected = projected_field(Some(""), "", false);

        assert_eq!(
            projected.len(),
            1,
            "the field is published with nothing to read"
        );
        assert_eq!(projected[0].role, AccessibilityRole::TextField);
        assert_eq!(projected[0].label, "");
        assert_eq!(projected[0].value.as_deref(), Some(""));
    }

    #[test]
    fn a_named_text_field_keeps_its_name_and_carries_its_text() {
        let projected = projected_field(Some("Folder name"), "Milk", false);

        assert_eq!(projected[0].label, "Folder name");
        assert_eq!(projected[0].value.as_deref(), Some("Milk"));
    }

    #[test]
    fn drawn_controls_without_a_label_or_a_size_are_not_published() {
        let canvas_id = 4;
        let mut root = node(
            canvas_id,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            Vec::new(),
        );
        root.canvas_children = vec![
            CanvasSemanticsNode::control(1, rect(0.0, 0.0, 100.0, 40.0), "   "),
            CanvasSemanticsNode::control(2, rect(0.0, 40.0, 100.0, 0.0), "Off screen"),
            CanvasSemanticsNode::control(3, rect(0.0, 60.0, 100.0, 40.0), "Visible"),
        ];
        let bounds =
            HashMap::from_iter([(canvas_id, AccessibilityRect::new(0.0, 0.0, 100.0, 200.0))]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].label, "Visible");
    }

    #[test]
    fn a_labelled_canvas_keeps_its_own_element_ahead_of_its_drawn_controls() {
        let canvas_id = 9;
        let mut root = node(
            canvas_id,
            SemanticsRole::Layout,
            vec![SemanticsAction::Click {
                handler: SemanticsCallback::new(canvas_id),
            }],
            Some("Orbit Breaker. CAMPAIGN. Turn the crown to change the choice."),
            Vec::new(),
        );
        root.widget_role = Some(SemanticsWidgetRole::RadioButton);
        root.state_description = Some("CAMPAIGN".into());
        root.on_click_label = Some("CAMPAIGN".into());
        root.selected = Some(true);
        root.canvas_children = vec![
            CanvasSemanticsNode::control(1, rect(60.0, 10.0, 80.0, 40.0), "CAMPAIGN")
                .with_role(SemanticsWidgetRole::RadioButton)
                .with_selected(true),
            CanvasSemanticsNode::control(2, rect(60.0, 150.0, 80.0, 40.0), "DAILY")
                .with_role(SemanticsWidgetRole::RadioButton)
                .with_selected(false),
        ];
        let bounds =
            HashMap::from_iter([(canvas_id, AccessibilityRect::new(0.0, 0.0, 200.0, 200.0))]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected.len(), 3);
        assert_eq!(projected[0].canvas_key, None);
        assert_eq!(projected[0].role, AccessibilityRole::RadioButton);
        assert_eq!(projected[0].state_description.as_deref(), Some("CAMPAIGN"));
        assert_eq!(projected[0].click_label.as_deref(), Some("CAMPAIGN"));
        assert_eq!(projected[1].label, "CAMPAIGN");
        assert_eq!(projected[1].selected, Some(true));
        assert_eq!(projected[2].label, "DAILY");
        assert_eq!(projected[2].selected, Some(false));
    }

    #[test]
    fn custom_action_labels_reach_the_platform_in_publication_order() {
        let arena_id = 3;
        let mut root = node(
            arena_id,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Level 4. Score 120."),
            Vec::new(),
        );
        root.custom_actions = vec![
            SemanticsCustomAction::new("Pause", || {}),
            SemanticsCustomAction::new("Restart", || {}),
        ];
        let bounds =
            HashMap::from_iter([(arena_id, AccessibilityRect::new(0.0, 0.0, 200.0, 200.0))]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].custom_actions, vec!["Pause", "Restart"]);
    }

    #[test]
    fn a_custom_action_runs_the_handler_the_tree_currently_holds() {
        let arena_id = 3;
        let canvas_id = 5;
        let fired = Rc::new(RefCell::new(Vec::new()));

        let mut root = node(
            arena_id,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            Vec::new(),
        );
        root.custom_actions = vec![SemanticsCustomAction::new("Pause", {
            let fired = Rc::clone(&fired);
            move || fired.borrow_mut().push("pause")
        })];
        let mut child = node(
            canvas_id,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            Vec::new(),
        );
        child.canvas_children = vec![
            CanvasSemanticsNode::control(42, rect(0.0, 0.0, 40.0, 40.0), "Haptics")
                .with_custom_action(SemanticsCustomAction::new("Toggle", {
                    let fired = Rc::clone(&fired);
                    move || fired.borrow_mut().push("toggle")
                })),
        ];
        root.children = vec![child];

        assert!(perform_custom_action(&root, arena_id, None, 0));
        assert!(perform_custom_action(&root, canvas_id, Some(42), 0));

        assert!(!perform_custom_action(&root, 999, None, 0));
        assert!(!perform_custom_action(&root, canvas_id, Some(43), 0));
        assert!(!perform_custom_action(&root, arena_id, None, 1));

        assert_eq!(*fired.borrow(), vec!["pause", "toggle"]);
    }

    #[test]
    fn rebuilding_a_custom_action_handler_is_not_a_published_change() {
        let arena_id = 3;
        let bounds =
            HashMap::from_iter([(arena_id, AccessibilityRect::new(0.0, 0.0, 200.0, 200.0))]);
        let project = |handler: fn()| {
            let mut root = node(
                arena_id,
                SemanticsRole::Layout,
                Vec::new(),
                Some("Level 4."),
                Vec::new(),
            );
            root.custom_actions = vec![SemanticsCustomAction::new("Pause", handler)];
            project_semantics(&root, &bounds)
        };

        assert_eq!(project(|| {}), project(|| panic!("must not run")));
    }

    fn live_text(node_id: NodeId, text: &str) -> AccessibilityElement {
        AccessibilityElement {
            node_id,
            label: text.into(),
            bounds: AccessibilityRect::new(0.0, 0.0, 10.0, 10.0),
            live_region: Some(cranpose_ui::LiveRegionMode::Polite),
            ..AccessibilityElement::default()
        }
    }

    #[test]
    fn a_live_region_with_new_text_is_read_out() {
        let before = vec![live_text(1, "3 receipts")];
        let after = vec![live_text(1, "4 receipts")];
        let announcements = live_region_announcements(&before, &after);
        assert_eq!(announcements.len(), 1);
        assert_eq!(announcements[0].text, "4 receipts");
    }

    #[test]
    fn a_live_region_that_kept_its_text_stays_quiet() {
        let before = vec![live_text(1, "3 receipts")];
        let after = vec![live_text(1, "3 receipts")];
        assert!(live_region_announcements(&before, &after).is_empty());
    }

    fn pane(node_id: NodeId, title: &str) -> AccessibilityElement {
        AccessibilityElement {
            node_id,
            bounds: AccessibilityRect::new(0.0, 0.0, 300.0, 600.0),
            pane_title: Some(title.into()),
            ..AccessibilityElement::default()
        }
    }

    #[test]
    fn a_new_pane_title_is_read_out() {
        let before = vec![pane(1, "Library")];
        let after = vec![pane(1, "Receipt")];
        let announcements = pane_title_announcements(&before, &after);
        assert_eq!(announcements.len(), 1);
        assert_eq!(announcements[0].text, "Receipt");
        assert!(
            pane_title_announcements(&after, &after).is_empty(),
            "the same title stays quiet"
        );
    }

    #[test]
    fn the_first_publish_keeps_pane_titles_quiet() {
        assert!(pane_title_announcements(&[], &[pane(1, "Library")]).is_empty());
    }

    #[test]
    fn a_toggle_that_flipped_is_a_spoken_change() {
        let mut before = live_text(1, "Dark theme");
        before.toggled = Some(false);
        let mut after = before.clone();
        after.toggled = Some(true);
        assert_eq!(spoken_changes(&[before], &[after]), vec![true]);
    }

    #[test]
    fn an_element_that_kept_its_words_is_not_a_spoken_change() {
        let before = live_text(1, "Dark theme");
        let after = live_text(1, "Dark theme");
        let fresh = live_text(2, "Fresh");
        assert_eq!(
            spoken_changes(&[before], &[after, fresh]),
            vec![false, false]
        );
    }

    #[test]
    fn a_new_error_is_a_spoken_change() {
        let before = live_text(1, "Amount");
        let mut after = live_text(1, "Amount");
        after.error = Some("needs a number".into());
        assert_eq!(spoken_changes(&[before], &[after.clone()]), vec![true]);
        assert_eq!(
            state_with_error(&after).as_deref(),
            Some("invalid, needs a number")
        );
    }

    #[test]
    fn a_password_field_never_reads_its_text_out() {
        let named = projected_field(Some("Passphrase"), "hunter2", true);
        assert_eq!(named[0].label, "Passphrase");
        assert_eq!(named[0].value, None, "the text stays unspoken");
        assert!(named[0].password);

        let unnamed = projected_field(None, "hunter2", true);
        assert_eq!(unnamed[0].label, "password", "no name reads no secret");
        assert_eq!(unnamed[0].value, None);
    }

    #[test]
    fn a_traversal_index_moves_a_node_in_the_reading_order() {
        let mut search = node(
            2,
            SemanticsRole::Text {
                value: "Search".into(),
            },
            Vec::new(),
            None,
            Vec::new(),
        );
        search.traversal_index = -1.0;
        let title = node(
            3,
            SemanticsRole::Text {
                value: "Receipts".into(),
            },
            Vec::new(),
            None,
            Vec::new(),
        );
        let root = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![title, search],
        );
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
            (2, AccessibilityRect::new(0.0, 60.0, 300.0, 40.0)),
            (3, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        ]);

        let projected = project_semantics(&root, &bounds);
        let labels: Vec<&str> = projected
            .iter()
            .map(|element| element.label.as_str())
            .collect();

        assert_eq!(
            labels,
            vec!["Search", "Receipts"],
            "the search field is read first"
        );
    }

    #[test]
    fn a_control_that_opens_reads_as_closed_and_back() {
        let mut row = node(
            2,
            SemanticsRole::Text {
                value: "Details".into(),
            },
            Vec::new(),
            None,
            Vec::new(),
        );
        row.expand = Some(cranpose_ui::SemanticsExpand::new(|| true));
        let root = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![row.clone()],
        );
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
        ]);

        let closed = project_semantics(&root, &bounds);
        assert_eq!(
            closed[0].expanded,
            Some(false),
            "a control that opens is closed"
        );
        assert_eq!(expansion_word(&closed[0]), Some("collapsed"));
        assert!(set_expanded(&root, 2, true), "the control takes the ask");
        assert!(!set_expanded(&root, 2, false), "it has no way to close yet");

        let mut open = row;
        open.expand = None;
        open.collapse = Some(cranpose_ui::SemanticsExpand::new(|| true));
        let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![open]);
        let projected = project_semantics(&root, &bounds);
        assert_eq!(projected[0].expanded, Some(true));
        assert_eq!(expansion_word(&projected[0]), Some("expanded"));
        assert!(set_expanded(&root, 2, false));
    }

    #[test]
    fn a_control_says_what_it_does_when_a_reader_sends_it_away() {
        let ran = Rc::new(Cell::new(0));
        let mut row = text_node(2, "Milk");
        row.dismiss = Some(cranpose_ui::SemanticsDismiss::new({
            let ran = Rc::clone(&ran);
            move || {
                ran.set(ran.get() + 1);
                true
            }
        }));
        let quiet = text_node(3, "Bread");
        let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![row, quiet]);
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
            (3, AccessibilityRect::new(0.0, 40.0, 300.0, 40.0)),
        ]);

        let projected = project_semantics(&root, &bounds);
        assert!(projected[0].dismissable, "the row says it has a way out");
        assert!(!projected[1].dismissable, "the other row says nothing");
        assert_eq!(listed_actions(&projected[0]), vec![DISMISS_LABEL]);
        assert!(listed_actions(&projected[1]).is_empty());

        assert!(dismiss(&root, 2), "the row takes the ask");
        assert_eq!(ran.get(), 1);
        assert!(!dismiss(&root, 3), "the other row has no way out");
    }

    #[test]
    fn the_way_out_sits_after_the_actions_the_app_named() {
        let ran = Rc::new(Cell::new(String::new()));
        let mut row = text_node(2, "Milk");
        row.custom_actions = vec![cranpose_ui::SemanticsCustomAction::new("Pin", {
            let ran = Rc::clone(&ran);
            move || ran.set("Pin".into())
        })];
        row.dismiss = Some(cranpose_ui::SemanticsDismiss::new({
            let ran = Rc::clone(&ran);
            move || {
                ran.set("Dismiss".into());
                true
            }
        }));
        let (root, bounds) = one_row_tree(row);

        let projected = project_semantics(&root, &bounds);
        assert_eq!(listed_actions(&projected[0]), vec!["Pin", DISMISS_LABEL]);

        assert!(perform_listed_action(&root, 2, None, 1, 0));
        assert_eq!(ran.take(), "Pin");
        assert!(perform_listed_action(&root, 2, None, 1, 1));
        assert_eq!(ran.take(), "Dismiss");
    }

    #[test]
    fn a_long_press_is_named_and_sits_after_the_custom_actions() {
        let ran = Rc::new(Cell::new(0));
        let mut row = text_node(2, "Milk");
        row.custom_actions = vec![cranpose_ui::SemanticsCustomAction::new("Pause", || {})];
        row.on_long_click_label = Some("Remove receipt".into());
        row.on_long_click = Some(cranpose_ui::SemanticsLongClick::new({
            let ran = Rc::clone(&ran);
            move || {
                ran.set(ran.get() + 1);
                true
            }
        }));
        let (root, bounds) = one_row_tree(row);

        let projected = project_semantics(&root, &bounds);
        assert_eq!(
            projected[0].long_click_label.as_deref(),
            Some("Remove receipt")
        );
        assert_eq!(
            reader_actions(&projected[0]),
            vec!["Pause".to_owned(), "Remove receipt".to_owned()],
            "the long press is the last action a reader lists"
        );

        assert!(perform_custom_action(&root, 2, None, 1));
        assert_eq!(ran.get(), 1, "the trailing action is the long press");
        assert!(long_click(&root, 2));
        assert_eq!(ran.get(), 2);
        assert!(!perform_custom_action(&root, 2, None, 2));
    }

    #[test]
    fn a_control_with_no_long_press_offers_none_and_a_nameless_one_is_still_read() {
        let mut plain = text_node(2, "Milk");
        let (root, bounds) = one_row_tree(plain.clone());
        assert_eq!(project_semantics(&root, &bounds)[0].long_click_label, None);
        assert!(!long_click(&root, 2), "there is nothing to run");

        plain.on_long_click = Some(cranpose_ui::SemanticsLongClick::new(|| true));
        plain.on_long_click_label = Some("   ".into());
        let (root, bounds) = one_row_tree(plain);
        assert_eq!(
            project_semantics(&root, &bounds)[0]
                .long_click_label
                .as_deref(),
            Some("long press"),
            "a control that takes a long press with no phrase still reads as one"
        );
    }

    #[test]
    fn a_dropdown_and_a_picker_keep_their_own_roles() {
        let mut dropdown = node(
            2,
            SemanticsRole::Text {
                value: "Sort by".into(),
            },
            Vec::new(),
            None,
            Vec::new(),
        );
        dropdown.widget_role = Some(cranpose_ui::SemanticsWidgetRole::DropdownList);
        let mut picker = node(
            3,
            SemanticsRole::Text {
                value: "Copies".into(),
            },
            Vec::new(),
            None,
            Vec::new(),
        );
        picker.widget_role = Some(cranpose_ui::SemanticsWidgetRole::ValuePicker);
        let root = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![dropdown, picker],
        );
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
            (3, AccessibilityRect::new(0.0, 40.0, 300.0, 40.0)),
        ]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected[0].role, AccessibilityRole::DropdownList);
        assert_eq!(projected[1].role, AccessibilityRole::ValuePicker);
    }

    #[test]
    fn a_live_region_that_just_appeared_is_read_out() {
        let before = vec![live_text(1, "3 receipts")];
        let after = vec![live_text(1, "3 receipts"), live_text(2, "Import failed")];
        let announcements = live_region_announcements(&before, &after);
        assert_eq!(announcements.len(), 1);
        assert_eq!(announcements[0].text, "Import failed");
    }

    #[test]
    fn the_first_screen_is_not_read_out_as_a_change() {
        assert!(live_region_announcements(&[], &[live_text(1, "3 receipts")]).is_empty());
    }

    #[test]
    fn a_node_without_a_live_region_is_never_read_out_on_a_change() {
        let before = vec![element_with(1, None)];
        let mut after = element_with(1, None);
        after.label = "Row 2".into();
        assert!(live_region_announcements(&before, &[after]).is_empty());
    }

    #[test]
    fn a_live_region_reaches_every_control_under_it() {
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 200.0, 50.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 200.0, 50.0)),
        ]);
        let mut root = node(
            1,
            SemanticsRole::Layout,
            Vec::new(),
            None,
            vec![node(
                2,
                SemanticsRole::Text {
                    value: "2 left".into(),
                },
                Vec::new(),
                None,
                Vec::new(),
            )],
        );
        root.live_region = Some(cranpose_ui::LiveRegionMode::Assertive);
        let elements = project_semantics(&root, &bounds);
        assert_eq!(elements.len(), 1);
        assert_eq!(
            elements[0].live_region,
            Some(cranpose_ui::LiveRegionMode::Assertive)
        );
    }

    #[test]
    fn an_adjustable_control_publishes_its_range_and_takes_a_new_value() {
        let taken = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&taken);
        let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 200.0, 40.0))]);
        let mut root = node(
            7,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Volume"),
            Vec::new(),
        );
        root.progress = Some(cranpose_ui::ProgressBarRangeInfo::new(0.4, 0.0, 1.0, 0));
        root.set_progress = Some(cranpose_ui::SemanticsSetProgress::new(move |value| {
            seen.borrow_mut().push(value);
            true
        }));

        let elements = project_semantics(&root, &bounds);
        assert_eq!(elements.len(), 1);
        let published = elements[0]
            .progress
            .expect("the range reaches the platform");
        assert_eq!(published.current, 0.4);
        assert_eq!(published.end, 1.0);
        assert!(elements[0].adjustable);

        assert!(set_progress(&root, 7, 0.6));
        assert_eq!(*taken.borrow(), vec![0.6]);
        assert!(!set_progress(&root, 99, 0.6), "no such control");
    }

    #[test]
    fn a_reader_hands_a_field_its_text() {
        let taken = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&taken);
        let mut root = node(7, SemanticsRole::Layout, Vec::new(), Some(""), Vec::new());
        root.editable_text = true;
        root.set_text = Some(cranpose_ui::SemanticsSetText::new(move |text| {
            seen.borrow_mut().push(text.to_owned());
            true
        }));

        assert!(set_text(&root, 7, "Milk"));
        assert_eq!(*taken.borrow(), vec!["Milk".to_owned()]);
        assert!(!set_text(&root, 99, "Milk"), "no such field");
    }

    #[test]
    fn every_role_is_named_once_on_every_platform() {
        let once = |count: usize, what: &str, role: AccessibilityRole| {
            assert_eq!(count, 1, "{role:?} should be in the {what} table once");
        };
        for role in AccessibilityRole::ALL {
            let widget = WIDGET_ROLES.iter().filter(|(_, own)| *own == role).count();
            if role != AccessibilityRole::StaticText && role != AccessibilityRole::TextField {
                once(widget, "widget role", role);
            }
            once(
                ARIA_ROLES
                    .iter()
                    .filter(|(named, _)| *named == role)
                    .count(),
                "ARIA",
                role,
            );
            once(
                ANDROID_ROLE_CODES
                    .iter()
                    .filter(|(named, _)| *named == role)
                    .count(),
                "Android",
                role,
            );
        }
        let mut codes: Vec<i32> = ANDROID_ROLE_CODES.iter().map(|(_, code)| *code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(
            codes.len(),
            ANDROID_ROLE_CODES.len(),
            "every Android code is its own"
        );
        assert_eq!(
            AccessibilityRole::from_widget_role(SemanticsWidgetRole::SearchField),
            AccessibilityRole::SearchField
        );
        assert_eq!(AccessibilityRole::SearchField.aria_name(), "searchbox");
        assert_eq!(AccessibilityRole::ListItem.android_code(), 23);
    }

    #[test]
    fn a_magic_tap_is_listed_after_the_long_press_and_runs_from_the_list() {
        let taps = Rc::new(Cell::new(0));
        let presses = Rc::new(Cell::new(0));
        let mut root = node(
            7,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Shutter"),
            Vec::new(),
        );
        root.custom_actions = vec![SemanticsCustomAction::new("Flash", || {})];
        root.on_long_click_label = Some("Hold to focus".into());
        root.on_long_click = Some(cranpose_ui::SemanticsLongClick::new({
            let presses = Rc::clone(&presses);
            move || {
                presses.set(presses.get() + 1);
                true
            }
        }));
        root.on_magic_tap_label = Some("Take the photo".into());
        root.on_magic_tap = Some(cranpose_ui::SemanticsMagicTap::new({
            let taps = Rc::clone(&taps);
            move || {
                taps.set(taps.get() + 1);
                true
            }
        }));
        let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 80.0, 44.0))]);
        let projected = project_semantics(&root, &bounds);

        assert_eq!(
            reader_actions(&projected[0]),
            vec!["Flash", "Hold to focus", "Take the photo"]
        );
        assert!(perform_custom_action(&root, 7, None, 1));
        assert!(perform_custom_action(&root, 7, None, 2));
        assert!(!perform_custom_action(&root, 7, None, 3));
        assert!(magic_tap(&root, 7));
        assert!(!magic_tap(&root, 99));
        assert_eq!((presses.get(), taps.get()), (1, 2));

        root.on_long_click = None;
        root.on_long_click_label = None;
        let projected = project_semantics(&root, &bounds);
        assert_eq!(
            reader_actions(&projected[0]),
            vec!["Flash", "Take the photo"]
        );
        assert!(perform_custom_action(&root, 7, None, 1));
        assert_eq!(taps.get(), 3);
    }

    #[test]
    fn voice_control_names_and_a_language_reach_the_element() {
        let mut root = node(
            7,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Importieren"),
            Vec::new(),
        );
        root.input_labels = vec!["Import".into()];
        root.language = Some("de".into());
        let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 80.0, 44.0))]);

        let projected = project_semantics(&root, &bounds);

        assert_eq!(projected[0].input_labels, vec!["Import".to_owned()]);
        assert_eq!(projected[0].language.as_deref(), Some("de"));
        assert_eq!(projected[0].magic_tap_label, None);
    }

    #[test]
    fn a_reader_moves_the_caret_of_a_field() {
        let taken = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&taken);
        let mut root = node(7, SemanticsRole::Layout, Vec::new(), Some(""), Vec::new());
        root.editable_text = true;
        root.text = Some("añb😀c".to_owned());
        root.set_selection = Some(cranpose_ui::SemanticsSetSelection::new(
            move |anchor, focus| {
                seen.borrow_mut().push((anchor, focus));
                true
            },
        ));

        assert!(set_text_selection(&root, 7, 1, 3));
        assert!(set_text_selection_utf16(&root, 7, 2, 5));
        assert!(set_text_selection_chars(&root, 7, 4, 2));
        assert_eq!(*taken.borrow(), vec![(1, 3), (3, 8), (8, 3)]);
        assert!(!set_text_selection(&root, 99, 0, 0), "no such field");
    }

    #[test]
    fn text_offsets_convert_between_bytes_utf16_units_and_characters() {
        let text = "añb😀c";
        assert_eq!(utf16_offset(text, 0), 0);
        assert_eq!(utf16_offset(text, 3), 2);
        assert_eq!(utf16_offset(text, 8), 5);
        assert_eq!(
            utf16_offset(text, 2),
            1,
            "inside ñ rounds down to its start"
        );
        assert_eq!(utf16_offset(text, 99), 6);
        assert_eq!(byte_offset_for_utf16(text, 2), 3);
        assert_eq!(
            byte_offset_for_utf16(text, 4),
            8,
            "inside the emoji lands after it"
        );
        assert_eq!(byte_offset_for_utf16(text, 99), 9);
        assert_eq!(char_offset(text, 8), 4);
        assert_eq!(char_offset(text, 99), 5);
        assert_eq!(byte_offset_for_chars(text, 3), 4);
        assert_eq!(byte_offset_for_chars(text, 4), 8);
        assert_eq!(byte_offset_for_chars(text, 99), 9);
    }

    #[test]
    fn an_editable_field_publishes_where_its_caret_is_and_a_password_does_not() {
        for (password, expected) in [(false, Some((1, 3))), (true, None)] {
            let mut field = node(
                2,
                SemanticsRole::Layout,
                Vec::new(),
                Some("Name"),
                Vec::new(),
            );
            field.editable_text = true;
            field.password = password;
            field.text = Some("Milk".to_owned());
            field.text_selection = Some(cranpose_ui::TextRange::new(1, 3));
            let root = node(1, SemanticsRole::Layout, Vec::new(), None, vec![field]);
            let bounds = HashMap::from_iter([
                (1, AccessibilityRect::new(0.0, 0.0, 300.0, 200.0)),
                (2, AccessibilityRect::new(0.0, 0.0, 300.0, 40.0)),
            ]);

            let projected = project_semantics(&root, &bounds);

            assert_eq!(projected[0].text_selection, expected);
        }
    }

    #[test]
    fn a_step_moves_one_stop_and_stops_at_the_ends() {
        let ten = cranpose_ui::ProgressBarRangeInfo::new(0.5, 0.0, 1.0, 0);
        assert!((stepped_value(&ten, true) - 0.6).abs() < 1e-6);
        assert!((stepped_value(&ten, false) - 0.4).abs() < 1e-6);

        let top = cranpose_ui::ProgressBarRangeInfo::new(1.0, 0.0, 1.0, 0);
        assert_eq!(stepped_value(&top, true), 1.0, "a step up stays at the end");

        let four_stops = cranpose_ui::ProgressBarRangeInfo::new(0.0, 0.0, 1.0, 4);
        assert!((four_stops.step() - 0.2).abs() < 1e-6);
    }

    fn scroll_box(
        node_id: NodeId,
        scroll_parent: Option<NodeId>,
        height: f32,
    ) -> AccessibilityElement {
        AccessibilityElement {
            node_id,
            bounds: AccessibilityRect::new(0.0, 0.0, 400.0, height),
            vertical_scroll: Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false)),
            scroll_parent,
            ..AccessibilityElement::default()
        }
    }

    fn row_in(node_id: NodeId, label: &str, scroll_parent: Option<NodeId>) -> AccessibilityElement {
        AccessibilityElement {
            node_id,
            label: label.into(),
            bounds: AccessibilityRect::new(0.0, -300.0, 400.0, 40.0),
            scroll_parent,
            ..AccessibilityElement::default()
        }
    }

    #[test]
    fn a_row_pages_the_list_above_it_even_once_it_left_the_screen() {
        let outer = scroll_box(1, None, 800.0);
        let inner = scroll_box(2, Some(1), 300.0);
        let row = row_in(3, "Milk", Some(2));
        let footer = row_in(4, "Total", Some(1));
        let elements = vec![outer, inner, row.clone(), footer.clone()];

        let around_row = scroll_container_for(&elements, &row).expect("the row sits in a list");
        let around_footer =
            scroll_container_for(&elements, &footer).expect("the footer sits in the outer list");
        let page = page_delta(around_row, true);

        assert_eq!(around_row.node_id, 2, "the list right above the row wins");
        assert_eq!(around_footer.node_id, 1);
        assert_eq!(
            page,
            (0.0, 270.0),
            "one page is nine tenths of the list height"
        );
        assert_eq!(page_delta(around_row, false), (0.0, -270.0));
    }

    #[test]
    fn a_list_publishes_its_rows_and_takes_a_row_number() {
        let taken = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&taken);
        let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 400.0, 600.0))]);
        let mut root = node(7, SemanticsRole::Layout, Vec::new(), Some(""), Vec::new());
        root.vertical_scroll = Some(cranpose_ui::ScrollAxisRange::new(0.0, 1.0, false));
        root.collection = Some(cranpose_ui::CollectionInfo {
            rows: 500,
            columns: 1,
        });
        root.scroll_to_index = Some(cranpose_ui::SemanticsScrollToIndex::new(move |index| {
            seen.borrow_mut().push(index);
            true
        }));

        let elements = project_semantics(&root, &bounds);
        assert_eq!(elements.len(), 1);
        assert!(elements[0].scroll_to_index);
        assert_eq!(row_count(&elements[0]), 500, "the last row is 499");

        assert!(scroll_to_index(&root, 7, 300));
        assert_eq!(*taken.borrow(), vec![300]);
        assert!(!scroll_to_index(&root, 99, 300), "no such list");
    }

    #[test]
    fn a_plain_scroll_view_takes_no_row_number() {
        let list = scroll_box(1, None, 300.0);

        assert!(!list.scroll_to_index);
        assert_eq!(
            row_count(&list),
            0,
            "a container that answers no row number names no last row"
        );
    }

    #[test]
    fn an_element_outside_every_list_pages_nothing() {
        let list = scroll_box(1, None, 300.0);
        let button = row_in(2, "Pay", None);

        assert!(scroll_container_for(&[list], &button).is_none());
    }

    #[test]
    fn the_projection_names_the_list_above_each_row() {
        let bounds = HashMap::from_iter([
            (1, AccessibilityRect::new(0.0, 0.0, 400.0, 600.0)),
            (2, AccessibilityRect::new(0.0, 0.0, 400.0, 40.0)),
            (3, AccessibilityRect::new(0.0, 700.0, 400.0, 40.0)),
        ]);
        let mut list = node(
            1,
            SemanticsRole::Layout,
            vec![],
            None,
            vec![node(2, SemanticsRole::Button, vec![], Some("Milk"), vec![])],
        );
        list.vertical_scroll = Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false));
        let root = node(
            0,
            SemanticsRole::Layout,
            vec![],
            None,
            vec![
                list,
                node(3, SemanticsRole::Button, vec![], Some("Pay"), vec![]),
            ],
        );

        let elements = project_semantics(&root, &bounds);
        let by_id = |id: NodeId| {
            elements
                .iter()
                .find(|e| e.node_id == id)
                .expect("projected")
        };

        assert_eq!(
            by_id(1).scroll_parent,
            None,
            "the list itself sits under no list"
        );
        assert_eq!(
            by_id(2).scroll_parent,
            Some(1),
            "the row names the list above it"
        );
        assert_eq!(
            by_id(3).scroll_parent,
            None,
            "the button outside names none"
        );
    }

    #[test]
    fn a_control_without_a_range_is_never_adjustable() {
        let bounds = HashMap::from_iter([(7, AccessibilityRect::new(0.0, 0.0, 200.0, 40.0))]);
        let root = node(
            7,
            SemanticsRole::Layout,
            Vec::new(),
            Some("Volume"),
            Vec::new(),
        );
        let elements = project_semantics(&root, &bounds);
        assert!(elements[0].progress.is_none());
        assert!(!elements[0].adjustable);
    }

    #[test]
    fn a_spoken_line_says_the_name_the_role_the_state_and_the_actions() {
        let mut flash = element_with(1, None);
        flash.label = "Flash".to_string();
        flash.role = AccessibilityRole::Switch;
        flash.toggled = Some(true);
        flash.custom_actions = vec!["Reset".to_string()];
        assert_eq!(spoken_line(&flash), "Flash, switch, on, actions: Reset");

        let mut loading = element_with(2, None);
        loading.label = "Loading".to_string();
        loading.role = AccessibilityRole::ProgressBar;
        loading.progress = Some(ProgressBarRangeInfo::new(0.4, 0.0, 1.0, 0));
        loading.enabled = false;
        assert_eq!(
            spoken_line(&loading),
            "Loading, progress bar, 40 percent, dimmed"
        );

        let mut library = element_with(3, None);
        library.label = String::new();
        library.pane_title = Some("Library".to_string());
        assert_eq!(spoken_line(&library), "Library, pane");

        let mut plain = element_with(4, None);
        plain.label = "Milk".to_string();
        plain.focused = true;
        assert_eq!(spoken_line(&plain), "Milk, focused");
    }
}
