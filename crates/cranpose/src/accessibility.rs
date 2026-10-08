use std::fmt::Debug;

use cranpose_app_shell::AppShell;
use cranpose_core::{
    NodeId,
    collections::rare::{RareProperties, rare, update_rare},
};
use cranpose_render_common::Renderer;
use cranpose_ui::{
    Announcement, CollectionInfo, LiveRegionMode, ProgressBarRangeInfo, ScrollAxisRange,
    SemanticsAction, SemanticsDetails, SemanticsNode, SemanticsRole, SemanticsWidgetRole,
};

#[path = "accessibility_identity.rs"]
mod identity;
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
pub(crate) use identity::AccessibilityIdentityError;
pub(crate) use identity::AccessibilitySnapshot;
#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) use identity::Replaced;

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

/// Whether a person edits the text of this field through the mirror: a text
/// field with a caret, or one that holds a secret.
#[cfg(any(
    test,
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) fn edits_text(element: &AccessibilityElement) -> bool {
    element.role.is_text_field()
        && (element.details().text_selection.is_some() || element.details().password)
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
    let details = element.details();
    if details.progress.is_some()
        && details.adjustable
        && element.role != AccessibilityRole::ValuePicker
    {
        "slider"
    } else if details.pane_title.is_some() && element.role == AccessibilityRole::StaticText {
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
    pub(crate) value: Option<String>,
    pub(crate) bounds: AccessibilityRect,
    pub(crate) role: AccessibilityRole,
    pub(crate) clickable: bool,
    pub(crate) selected: Option<bool>,
    pub(crate) toggled: Option<bool>,
    pub(crate) enabled: bool,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) focused: bool,
    pub(crate) scroll_parent: Option<NodeId>,
    pub(crate) collection_item: Option<CollectionItem>,
    pub(crate) details: Option<Box<AccessibilityDetails>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccessibilityDetails {
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
    pub(crate) custom_actions: Vec<String>,
    pub(crate) live_region: Option<LiveRegionMode>,
    pub(crate) progress: Option<ProgressBarRangeInfo>,
    pub(crate) adjustable: bool,
    pub(crate) vertical_scroll: Option<ScrollAxisRange>,
    pub(crate) horizontal_scroll: Option<ScrollAxisRange>,
    /// Whether a screen reader may ask this list for the row at an index.
    pub(crate) scroll_to_index: bool,
    pub(crate) collection: Option<CollectionInfo>,
    pub(crate) pane_title: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) password: bool,
    pub(crate) expanded: Option<bool>,
    pub(crate) dismissable: bool,
    pub(crate) is_modal: bool,
    /// Where the caret of an editable field sits, or which stretch of its
    /// text is picked: the anchor and the end that moves, as byte offsets into
    /// the element's `value`. A field that holds a secret publishes none.
    pub(crate) text_selection: Option<(usize, usize)>,
    pub(crate) multiline: bool,
}

impl AccessibilityDetails {
    const NONE: Self = Self {
        state_description: None,
        click_label: None,
        long_click_label: None,
        magic_tap_label: None,
        input_labels: Vec::new(),
        language: None,
        custom_actions: Vec::new(),
        live_region: None,
        progress: None,
        adjustable: false,
        vertical_scroll: None,
        horizontal_scroll: None,
        scroll_to_index: false,
        collection: None,
        pane_title: None,
        error: None,
        password: false,
        expanded: None,
        dismissable: false,
        is_modal: false,
        text_selection: None,
        multiline: false,
    };
}

impl Default for AccessibilityDetails {
    fn default() -> Self {
        Self::NONE
    }
}

impl RareProperties for AccessibilityDetails {
    const EMPTY: &'static Self = &Self::NONE;
}

impl AccessibilityElement {
    fn identity_key(&self) -> (NodeId, u32, Option<u64>) {
        (self.node_id, self.node_generation, self.canvas_key)
    }

    pub(crate) fn details(&self) -> &AccessibilityDetails {
        rare(&self.details)
    }

    /// Whether two elements describe the same control in the same state,
    /// wherever each of them sits.
    #[cfg(any(
        test,
        all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
    ))]
    pub(crate) fn same_but_bounds(&self, other: &Self) -> bool {
        let Self {
            node_id,
            node_generation,
            canvas_key,
            label,
            value,
            bounds: _,
            role,
            clickable,
            selected,
            toggled,
            enabled,
            focusable,
            tab_stop,
            focused,
            scroll_parent,
            collection_item,
            details,
        } = self;
        *node_id == other.node_id
            && *node_generation == other.node_generation
            && *canvas_key == other.canvas_key
            && *role == other.role
            && *clickable == other.clickable
            && *selected == other.selected
            && *toggled == other.toggled
            && *enabled == other.enabled
            && *focusable == other.focusable
            && *tab_stop == other.tab_stop
            && *focused == other.focused
            && *scroll_parent == other.scroll_parent
            && *collection_item == other.collection_item
            && *label == other.label
            && *value == other.value
            && *details == other.details
    }

    pub(crate) fn update_details(&mut self, update: impl FnOnce(&mut AccessibilityDetails)) {
        update_rare(&mut self.details, update);
    }
}

impl Default for AccessibilityElement {
    fn default() -> Self {
        Self {
            node_id: 0,
            node_generation: 0,
            canvas_key: None,
            label: String::new(),
            value: None,
            bounds: AccessibilityRect::default(),
            role: AccessibilityRole::StaticText,
            clickable: false,
            selected: None,
            toggled: None,
            enabled: true,
            focusable: false,
            tab_stop: true,
            focused: false,
            scroll_parent: None,
            collection_item: None,
            details: None,
        }
    }
}

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
#[derive(Default)]
pub(crate) struct TreeWatch {
    revision: Option<u64>,
    focus: Option<NodeId>,
    edits: u64,
}

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
impl TreeWatch {
    pub(crate) fn forget(&mut self) {
        self.revision = None;
    }

    pub(crate) fn elements_due<R>(
        &mut self,
        shell: &mut AppShell<R>,
        policy: &mut crate::accessibility_publish_policy::AccessibilityPublishPolicy,
        now: web_time::Instant,
        at_once: bool,
        published: &mut AccessibilitySnapshot,
    ) -> Option<Vec<AccessibilityElement>>
    where
        R: Renderer,
        R::Error: Debug,
    {
        let (focus, edits) = shell.app_context().enter(|| {
            (
                cranpose_ui::active_focus_target(),
                cranpose_ui::text_field_focus::edit_count(),
            )
        });
        let at_once = at_once || focus != self.focus || edits != self.edits;
        let changed = self.revision != Some(shell.semantics_snapshot_revision());
        if !policy.try_publish_change(now, changed, at_once) {
            return None;
        }
        self.focus = focus;
        self.edits = edits;
        snapshot_if_changed(shell, &mut self.revision, published)
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn snapshot_if_changed<R>(
    shell: &mut AppShell<R>,
    seen_revision: &mut Option<u64>,
    published: &mut AccessibilitySnapshot,
) -> Option<Vec<AccessibilityElement>>
where
    R: Renderer,
    R::Error: Debug,
{
    let revision = shell.semantics_snapshot_revision();
    if *seen_revision == Some(revision) {
        return None;
    }
    let next = snapshot(shell, published);
    *seen_revision = Some(shell.semantics_snapshot_revision());
    Some(next)
}

/// The elements a reader reaches, written over the elements `published`
/// holds no longer: a snapshot like the one before reuses their strings and
/// lists instead of allocating its own.
fn snapshot<R>(
    shell: &mut AppShell<R>,
    published: &mut AccessibilitySnapshot,
) -> Vec<AccessibilityElement>
where
    R: Renderer,
    R::Error: Debug,
{
    if !shell.semantics_active() {
        return Vec::new();
    }
    let Some(semantics_tree) = shell.semantics_tree() else {
        return Vec::new();
    };
    let mut projection = Projection::new(published.take_spare());
    project_node(semantics_tree.root(), false, None, None, &mut projection);
    let elements = projection.finish();
    #[cfg(debug_assertions)]
    assert_same_elements(&elements, &project_semantics(semantics_tree.root()));
    elements
}

fn project_semantics(root: &SemanticsNode) -> Vec<AccessibilityElement> {
    let mut projection = Projection::new(Vec::new());
    project_node(root, false, None, None, &mut projection);
    projection.finish()
}

#[cfg(debug_assertions)]
fn assert_same_elements(reused: &[AccessibilityElement], fresh: &[AccessibilityElement]) {
    if reused != fresh && format!("{reused:?}") != format!("{fresh:?}") {
        let at = reused
            .iter()
            .zip(fresh)
            .position(|(reused, fresh)| format!("{reused:?}") != format!("{fresh:?}"))
            .unwrap_or_else(|| reused.len().min(fresh.len()));
        panic!(
            "a projection over reused buffers differs from a fresh one at element {at}: {:?} != {:?}",
            reused.get(at),
            fresh.get(at)
        );
    }
}

struct Projection {
    elements: Vec<AccessibilityElement>,
    written: usize,
    members: Vec<usize>,
}

impl Projection {
    fn new(elements: Vec<AccessibilityElement>) -> Self {
        Self {
            elements,
            written: 0,
            members: Vec::new(),
        }
    }

    fn next_slot(&mut self) -> &mut AccessibilityElement {
        if self.written == self.elements.len() {
            self.elements.push(AccessibilityElement::default());
        }
        &mut self.elements[self.written]
    }

    fn commit(&mut self) {
        self.written += 1;
    }

    fn written(&mut self) -> &mut [AccessibilityElement] {
        &mut self.elements[..self.written]
    }

    fn finish(mut self) -> Vec<AccessibilityElement> {
        self.elements.truncate(self.written);
        self.elements
    }
}

fn reuse_text(buffer: Option<String>, text: Option<&str>) -> Option<String> {
    text.map(|text| reuse_string(buffer.unwrap_or_default(), text))
}

fn reuse_string(mut buffer: String, text: &str) -> String {
    buffer.clear();
    buffer.push_str(text);
    buffer
}

fn reuse_texts<'a>(mut buffer: Vec<String>, texts: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut written = 0;
    for text in texts {
        match buffer.get_mut(written) {
            Some(slot) => {
                slot.clear();
                slot.push_str(text);
            }
            None => buffer.push(text.to_owned()),
        }
        written += 1;
    }
    buffer.truncate(written);
    buffer
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
    semantics: &cranpose_ui::SemanticsTree,
) -> Vec<cranpose_app_shell::inspector::InspectorNode> {
    project_semantics(semantics.root())
        .into_iter()
        .map(inspector_node)
        .collect()
}

fn inspector_node(element: AccessibilityElement) -> cranpose_app_shell::inspector::InspectorNode {
    let details = element.details();
    let value = if details.password {
        "[protected]"
    } else {
        element.value.as_deref().unwrap_or("")
    };
    let mut actions = Vec::new();
    if element.clickable {
        actions.push("Activate".to_string());
    }
    if details.adjustable {
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
    actions.extend(details.custom_actions.iter().cloned());
    actions.extend(details.long_click_label.iter().cloned());
    actions.extend(details.magic_tap_label.iter().cloned());
    let summary = format!(
        "Name: {}\nRole: {:?}\nValue: {}\nState: {}\nEnabled: {}  Focused: {}\nSelected: {:?}  Toggled: {:?}\nBounds: {:.1}, {:.1}  {:.1} x {:.1}\nActions: {}\nLive: {:?}\nRange: {:?}\nError: {}",
        element.label,
        element.role,
        value,
        details.state_description.as_deref().unwrap_or(""),
        element.enabled,
        element.focused,
        element.selected,
        checked_state(&element),
        element.bounds.x,
        element.bounds.y,
        element.bounds.width,
        element.bounds.height,
        actions.join(", "),
        details.live_region,
        details.progress,
        details.error.as_deref().unwrap_or("")
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
        details: summary,
        focused: element.focused,
        issue: element.label.is_empty() && (element.clickable || element.focusable),
    }
}

fn project_node(
    node: &SemanticsNode,
    suppress_static_text: bool,
    inherited_live_region: Option<LiveRegionMode>,
    inherited_scroll: Option<NodeId>,
    out: &mut Projection,
) {
    if node.hidden {
        return;
    }
    let live_region = node.details().live_region.or(inherited_live_region);
    let first_new = out.written;
    let clickable = node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }));
    let actionable = clickable || node.details().editable_text;
    let merges = node.merges_accessibility_descendants();
    let boundary = node.is_accessibility_boundary();
    let rect = AccessibilityRect::new(
        node.bounds.x,
        node.bounds.y,
        node.bounds.width,
        node.bounds.height,
    );

    let container = is_container(node);
    if rect.is_visible() {
        let named = boundary || !suppress_static_text;
        if !write_node_element(node, rect, named, container, clickable, live_region, out)
            && actionable
        {
            warn_unlabeled(node.node_id);
        }
    }

    project_canvas_children(node, rect, live_region, out);
    for element in &mut out.written()[first_new..] {
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
        suppress_children,
        live_region,
        scroll_for_children,
        out,
    );
}

fn write_node_element(
    node: &SemanticsNode,
    rect: AccessibilityRect,
    named: bool,
    container: bool,
    clickable: bool,
    live_region: Option<LiveRegionMode>,
    out: &mut Projection,
) -> bool {
    let slot = out.next_slot();
    let mut label = std::mem::take(&mut slot.label);
    label.clear();
    let labelled = named && node.write_accessibility_label(&mut label);
    if !labelled && !container {
        slot.label = label;
        return false;
    }
    if !labelled {
        label.clear();
    }
    fill_node_element(slot, node, rect, label, clickable, live_region);
    out.commit();
    true
}

/// A node a reader walks into rather than stops on: a list, a scroll view, or
/// a group of tabs or radio buttons.
fn is_container(node: &SemanticsNode) -> bool {
    let details = node.details();
    details.vertical_scroll.is_some()
        || details.horizontal_scroll.is_some()
        || details.selectable_group
        || details.is_modal
        || details.pane_title.is_some()
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
    suppress_static_text: bool,
    live_region: Option<LiveRegionMode>,
    scroll_for_children: Option<NodeId>,
    out: &mut Projection,
) {
    let first_child = out.written;
    for child in node.accessibility_children() {
        project_node(
            child,
            suppress_static_text,
            live_region,
            scroll_for_children,
            out,
        );
    }
    let selectable_group = node.details().selectable_group
        || matches!(
            node.widget_role,
            Some(SemanticsWidgetRole::RadioGroup | SemanticsWidgetRole::TabBar)
        );
    let elements = &mut out.elements[..out.written];
    if selectable_group {
        number_group(node.node_id, first_child, elements, &mut out.members);
    }
    if selectable_group || node.widget_role == Some(SemanticsWidgetRole::Menu) {
        mark_group_tab_stop(node.node_id, first_child, elements, &mut out.members);
    }
}

fn mark_group_tab_stop(
    group: NodeId,
    first_child: usize,
    elements: &mut [AccessibilityElement],
    members: &mut Vec<usize>,
) {
    members.clear();
    members.extend((first_child..elements.len()).filter(|index| {
        let element = &elements[*index];
        element.scroll_parent == Some(group)
            && matches!(
                element.role,
                AccessibilityRole::RadioButton
                    | AccessibilityRole::Tab
                    | AccessibilityRole::MenuItem
            )
    }));
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
    for &index in members.iter() {
        elements[index].tab_stop = Some(index) == stop;
    }
}

/// Gives each selectable control under a group its place and the group's
/// size, and tells the group's own element how many it holds and which way
/// it runs.
fn number_group(
    group: NodeId,
    first_child: usize,
    elements: &mut [AccessibilityElement],
    members: &mut Vec<usize>,
) {
    members.clear();
    members.extend((first_child..elements.len()).filter(|index| {
        elements[*index].selected.is_some() && elements[*index].scroll_parent == Some(group)
    }));
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
        element
            .update_details(|details| details.collection = Some(CollectionInfo { rows, columns }));
        if element.role == AccessibilityRole::StaticText && radio_group {
            element.role = AccessibilityRole::RadioGroup;
        }
    }
}

/// Whether a control reads as open or as closed: one that says what closing
/// it does is open now, and one that says what opening it does is closed.
/// A control that says neither is not a thing a reader opens at all.
fn expansion(details: &SemanticsDetails) -> Option<bool> {
    details
        .collapse
        .is_some()
        .then_some(true)
        .or_else(|| details.expand.is_some().then_some(false))
}

/// What a reader reads out for a control's long press: the verb phrase the
/// app gave, and for a control that declared the action with no phrase the
/// plain words for what it is. A control with no long press gets nothing.
fn long_click_label(details: &SemanticsDetails) -> Option<&str> {
    details.on_long_click.as_ref()?;
    let named = details
        .on_long_click_label
        .as_deref()
        .filter(|label| !label.trim().is_empty());
    Some(named.unwrap_or("long press"))
}

/// What a reader lists for a control's magic tap: the verb phrase the app
/// gave, or the plain words for the gesture. A control with no magic tap
/// gets nothing.
fn magic_tap_label(details: &SemanticsDetails) -> Option<&str> {
    details.on_magic_tap.as_ref()?;
    let named = details
        .on_magic_tap_label
        .as_deref()
        .filter(|label| !label.trim().is_empty());
    Some(named.unwrap_or("magic tap"))
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

/// Writes one control as the platforms see it into `slot`, reusing the
/// strings and lists an earlier snapshot left there. A scroll container with
/// no label of its own comes through with an empty label: a reader never
/// lands on it, but it is the node the reader pages through.
fn fill_node_element(
    slot: &mut AccessibilityElement,
    node: &SemanticsNode,
    rect: AccessibilityRect,
    label: String,
    clickable: bool,
    live_region: Option<LiveRegionMode>,
) {
    let details = node.details();
    let role = if details.is_modal {
        AccessibilityRole::Dialog
    } else if let Some(role) = node.widget_role {
        AccessibilityRole::from_widget_role(role)
    } else if details.editable_text {
        AccessibilityRole::TextField
    } else if details.progress.is_some() {
        AccessibilityRole::ProgressBar
    } else if clickable || matches!(node.role, SemanticsRole::Button) {
        AccessibilityRole::Button
    } else {
        AccessibilityRole::StaticText
    };
    let spare = std::mem::take(slot);
    let value = reuse_text(
        spare.value,
        node.text
            .as_deref()
            .or_else(|| details.editable_text.then_some(label.as_str()))
            .filter(|_| !details.password)
            .filter(|value| details.editable_text || !value.is_empty()),
    );
    let mut element_details = spare.details;
    update_rare(&mut element_details, |held| {
        fill_node_details(held, node, live_region);
    });
    *slot = AccessibilityElement {
        node_id: node.node_id,
        node_generation: node.node_generation,
        canvas_key: None,
        value,
        label,
        bounds: rect,
        role,
        clickable: clickable && node.enabled,
        selected: node.selected,
        toggled: node.toggled,
        enabled: node.enabled,
        focusable: node.focusable,
        tab_stop: true,
        focused: node.focused,
        scroll_parent: None,
        collection_item: None,
        details: element_details,
    };
}

fn fill_node_details(
    held: &mut AccessibilityDetails,
    node: &SemanticsNode,
    live_region: Option<LiveRegionMode>,
) {
    let details = node.details();
    *held = AccessibilityDetails {
        state_description: reuse_text(
            held.state_description.take(),
            details.state_description.as_deref(),
        ),
        click_label: reuse_text(held.click_label.take(), details.on_click_label.as_deref()),
        long_click_label: reuse_text(held.long_click_label.take(), long_click_label(details)),
        magic_tap_label: reuse_text(held.magic_tap_label.take(), magic_tap_label(details)),
        input_labels: reuse_texts(
            std::mem::take(&mut held.input_labels),
            details.input_labels.iter().map(String::as_str),
        ),
        language: reuse_text(held.language.take(), details.language.as_deref()),
        custom_actions: reuse_texts(
            std::mem::take(&mut held.custom_actions),
            details
                .custom_actions
                .iter()
                .map(|action| action.label.as_str()),
        ),
        live_region: live_region.or_else(|| {
            (node.widget_role == Some(SemanticsWidgetRole::Alert))
                .then_some(LiveRegionMode::Assertive)
        }),
        progress: details.progress,
        adjustable: details.set_progress.is_some(),
        vertical_scroll: details.vertical_scroll,
        horizontal_scroll: details.horizontal_scroll,
        scroll_to_index: details.scroll_to_index.is_some(),
        collection: details.collection,
        pane_title: reuse_text(held.pane_title.take(), details.pane_title.as_deref()),
        error: reuse_text(held.error.take(), details.error.as_deref()),
        password: details.password,
        expanded: expansion(details),
        dismissable: details.dismiss.is_some(),
        is_modal: details.is_modal,
        text_selection: details
            .text_selection
            .filter(|_| details.editable_text && !details.password)
            .map(|range| (range.start, range.end)),
        multiline: details.multiline,
    };
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
    match &node.details().scroll_by {
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
    match &node.details().scroll_to_index {
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
    if !element.details().scroll_to_index {
        return 0;
    }
    element
        .details()
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
    if element.details().vertical_scroll.is_some() {
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
        if candidate.details().vertical_scroll.is_some()
            || candidate.details().horizontal_scroll.is_some()
        {
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
    out: &mut Projection,
) {
    for child in &node.details().canvas_children {
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
        let slot = out.next_slot();
        let spare = std::mem::take(slot);
        let mut details = spare.details;
        update_rare(&mut details, |held| {
            *held = AccessibilityDetails {
                state_description: reuse_text(
                    held.state_description.take(),
                    child.state_description.as_deref(),
                ),
                click_label: reuse_text(held.click_label.take(), child.on_click_label.as_deref()),
                custom_actions: reuse_texts(
                    std::mem::take(&mut held.custom_actions),
                    child
                        .custom_actions
                        .iter()
                        .map(|action| action.label.as_str()),
                ),
                live_region,
                ..AccessibilityDetails::NONE
            };
        });
        *slot = AccessibilityElement {
            node_id: node.node_id,
            canvas_key: Some(child.key),
            label: reuse_string(spare.label, &child.label),
            bounds: rect,
            role,
            clickable: child.clickable && node.enabled && child.enabled,
            selected: child.selected,
            toggled: child.toggled,
            enabled: node.enabled && child.enabled,
            details,
            ..AccessibilityElement::default()
        };
        out.commit();
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
    let details = node.details();
    let actions = match canvas_key {
        Some(key) => match details
            .canvas_children
            .iter()
            .find(|child| child.key == key)
        {
            Some(child) if child.enabled => &child.custom_actions,
            Some(_) => return false,
            None => return false,
        },
        None => &details.custom_actions,
    };
    match actions.get(action_index) {
        Some(action) => {
            action.invoke();
            true
        }
        None if canvas_key.is_none() => {
            let after = action_index - actions.len();
            match (after, &details.on_long_click, &details.on_magic_tap) {
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
        node.details()
            .on_magic_tap
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
    let details = element.details();
    if !element.enabled {
        return Vec::new();
    }
    details
        .custom_actions
        .iter()
        .cloned()
        .chain(details.long_click_label.clone())
        .chain(details.magic_tap_label.clone())
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
    match &node.details().set_progress {
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
    match &node.details().set_text {
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
    match &node.details().set_selection {
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
    let action = if open {
        &node.details().expand
    } else {
        &node.details().collapse
    };
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
    match &node.details().dismiss {
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
        .chain((element.enabled && element.details().dismissable).then(|| DISMISS_LABEL.to_owned()))
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
    match &node.details().on_long_click {
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
        .details()
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
        let Some(mode) = element.details().live_region else {
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
                .details()
                .pane_title
                .as_deref()
                .filter(|title| !title.trim().is_empty())?;
            let was = previous
                .iter()
                .find(|other| other.identity_key() == element.identity_key())
                .and_then(|other| other.details().pane_title.as_deref());
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
    if let Some(state) = &element.details().state_description {
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
        .details()
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
        .details()
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
    let now_details = now.details();
    let was_details = was.details();
    let same_sources = was.label == now.label
        && was.value == now.value
        && was_details.state_description == now_details.state_description
        && was_details.error == now_details.error;
    (same_sources || spoken_text(was) == spoken_text(now))
        && was.toggled == now.toggled
        && was.selected == now.selected
        && was_details.progress == now_details.progress
}

/// For each element of `current`, whether it was published before and now
/// says something else: a toggle that flipped, a counter that moved on, a
/// value a reader just set. A reader speaks the one under its cursor again.
/// `was` holds each element's index in `previous`, as
/// [`AccessibilitySnapshot::update`](crate::accessibility_identity::AccessibilitySnapshot::update)
/// reports it.
#[cfg(any(
    test,
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
pub(crate) fn spoken_changes(
    previous: &[AccessibilityElement],
    current: &[AccessibilityElement],
    was: &[Option<usize>],
) -> Vec<bool> {
    current
        .iter()
        .zip(was)
        .map(|(element, was)| {
            was.and_then(|index| previous.get(index))
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
    let details = element.details();
    let role_word = SPOKEN_ROLES
        .iter()
        .find(|(role, _)| *role == element.role)
        .map_or("", |(_, word)| *word);
    let name = match (&details.pane_title, element.label.is_empty()) {
        (Some(title), true) => format!("{title}, pane"),
        _ => spoken_text(element),
    };
    let toggle_words = if element.role == AccessibilityRole::Switch {
        ("on", "off")
    } else {
        ("checked", "not checked")
    };
    let actions: Vec<&str> = details
        .custom_actions
        .iter()
        .map(String::as_str)
        .chain(details.long_click_label.as_deref())
        .chain(details.magic_tap_label.as_deref())
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
    parts.extend(details.expanded.map(|open| {
        if open {
            "expanded".to_string()
        } else {
            "collapsed".to_string()
        }
    }));
    if details.state_description.is_none() {
        parts.extend(details.progress.as_ref().and_then(spoken_percent));
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
    snapshot(shell, &mut AccessibilitySnapshot::default())
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
    let details = element.details();
    let mut parts = Vec::new();
    if details.password {
        parts.push("password".to_owned());
    } else if let Some(value) = &element.value
        && (element.role.is_text_field() || value != &element.label)
        && !value.trim().is_empty()
    {
        parts.push(value.clone());
    }
    if details
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
        if let Some(progress) = details.progress {
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
