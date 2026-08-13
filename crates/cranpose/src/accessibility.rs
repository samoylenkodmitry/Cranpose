//! Platform-neutral projection of Cranpose semantics into accessibility elements.

use cranpose_core::collections::map::HashMap;
use cranpose_core::NodeId;
use cranpose_ui::{SemanticsAction, SemanticsNode, SemanticsRole, SemanticsWidgetRole};
use std::borrow::Cow;

use cranpose_app_shell::AppShell;
use cranpose_render_common::Renderer;
use cranpose_ui::LayoutBox;
use std::fmt::Debug;

/// Logical bounds for a platform accessibility element.
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

/// Role understood by native accessibility backends.
///
/// The first three are structural — what the node *is* in the tree. The rest
/// are Compose `Role` values an app asked for explicitly, and a backend that
/// understands them says so out loud ("radio button", "switch, on").
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
}

impl AccessibilityRole {
    fn from_widget_role(role: SemanticsWidgetRole) -> Self {
        match role {
            SemanticsWidgetRole::Button => Self::Button,
            SemanticsWidgetRole::Checkbox => Self::Checkbox,
            SemanticsWidgetRole::Switch => Self::Switch,
            SemanticsWidgetRole::RadioButton => Self::RadioButton,
            SemanticsWidgetRole::Tab => Self::Tab,
            SemanticsWidgetRole::Image => Self::Image,
            SemanticsWidgetRole::Header => Self::Header,
        }
    }
}

/// A flattened native accessibility element with stable Cranpose identity.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccessibilityElement {
    pub(crate) node_id: NodeId,
    /// Which drawn control inside `node_id` this element is, when it came from
    /// a [`CanvasSemanticsNode`]. `None` means the element *is* the layout
    /// node. Kept alongside `node_id` because an accessibility action arriving
    /// from the platform has to be routed back to the exact publisher.
    pub(crate) canvas_key: Option<u64>,
    pub(crate) label: String,
    pub(crate) state_description: Option<String>,
    pub(crate) click_label: Option<String>,
    pub(crate) value: Option<String>,
    pub(crate) bounds: AccessibilityRect,
    pub(crate) role: AccessibilityRole,
    pub(crate) clickable: bool,
    pub(crate) selected: Option<bool>,
    pub(crate) toggled: Option<bool>,
    pub(crate) enabled: bool,
    /// Custom action labels in publication order. The index *is* the wire id:
    /// the platform sends it back and [`perform_custom_action`] resolves it
    /// against the live semantics tree, so the handler that runs is always the
    /// current one even if the published snapshot is a frame stale.
    pub(crate) custom_actions: Vec<String>,
}

impl Default for AccessibilityElement {
    fn default() -> Self {
        Self {
            node_id: 0,
            canvas_key: None,
            label: String::new(),
            state_description: None,
            click_label: None,
            value: None,
            bounds: AccessibilityRect::default(),
            role: AccessibilityRole::StaticText,
            clickable: false,
            selected: None,
            toggled: None,
            enabled: true,
            custom_actions: Vec::new(),
        }
    }
}

/// Re-projects accessibility elements only when the shell's semantics revision
/// moved since `seen_revision`. On an animation-only frame — the steady state
/// of any game or transition — this is one integer compare, where the
/// unconditional snapshot used to walk the full layout tree every frame.
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

/// Extracts the current semantics and layout snapshots from an app shell.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn snapshot<R>(shell: &mut AppShell<R>) -> Vec<AccessibilityElement>
where
    R: Renderer,
    R::Error: Debug,
{
    // With semantics tracking off the projection is empty by definition, so
    // don't pay for the layout walk that would only prove it.
    if !shell.semantics_active() {
        return Vec::new();
    }
    // Borrowed rather than cloned: this runs on every frame that updates, and
    // deep-copying the layout and semantics trees to read them dominated the
    // accessibility cost.
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

/// Assigns the ids every platform backend addresses elements by — Android's
/// virtual view ids, AccessKit's node ids, the iOS element map's keys.
///
/// A screen reader parks its cursor on an id, so the id has to be a function of
/// *what the element is* rather than where it sits in the list — otherwise
/// inserting a row above the focused one moves the cursor. That is why the id
/// is derived from the publishing layout node plus the app's own canvas key,
/// and why a collision is resolved by probing forward deterministically instead
/// of by falling back to the index: the same element set always yields the same
/// ids.
///
/// The result is a positive 31-bit value because Android's virtual view ids are
/// `int` and `View.NO_ID` (-1) already means "the host".
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
        // A layout node keeps the identity it always had, so ids for the
        // widgets Cranpose lays out do not move because canvas semantics now
        // exist next to them.
        None => node_id as u64,
        // Fibonacci hashing of the app's key, folded into the node it belongs
        // to. Distinct keys on one node land far apart, which keeps the probe
        // loop above from degenerating on a list of adjacent row indices.
        Some(key) => (node_id as u64) ^ (key.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 29),
    };
    ((mixed & 0x7fff_ffff) as i32).max(1)
}

/// Maps an id the platform sent back to the element that owns it.
///
/// Recomputed from the published element list rather than remembered, because
/// the list *is* the assignment: [`element_ids`] is a pure function of it.
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
    project_node(root, bounds, false, &mut elements);
    elements
}

fn project_node(
    node: &SemanticsNode,
    bounds: &HashMap<NodeId, AccessibilityRect>,
    suppress_static_text: bool,
    elements: &mut Vec<AccessibilityElement>,
) {
    let clickable = node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }));
    let actionable = clickable || node.editable_text;
    let own_label = node_label(node).map(Cow::Borrowed);
    let label = if actionable {
        own_label.or_else(|| descendant_label(node).map(Cow::Owned))
    } else {
        own_label
    };
    let rect = bounds.get(&node.node_id).copied().unwrap_or_default();

    if let Some(label) = label.filter(|label| !label.trim().is_empty()) {
        if rect.is_visible() && (actionable || !suppress_static_text) {
            let role = if let Some(role) = node.widget_role {
                AccessibilityRole::from_widget_role(role)
            } else if node.editable_text {
                AccessibilityRole::TextField
            } else if clickable || matches!(node.role, SemanticsRole::Button) {
                AccessibilityRole::Button
            } else {
                AccessibilityRole::StaticText
            };
            let label = label.into_owned();
            elements.push(AccessibilityElement {
                node_id: node.node_id,
                canvas_key: None,
                value: node.editable_text.then(|| label.clone()),
                label,
                state_description: node.state_description.clone(),
                click_label: node.on_click_label.clone(),
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
            });
        }
    }

    // Drawn controls come straight after the node that drew them, so a screen
    // reader walks a canvas screen in the order the app published it. They are
    // deliberately not subject to `suppress_static_text`: that rule exists so a
    // button does not read its own inner `Text` twice, and a canvas child is
    // never a duplicate of anything — it only exists because the app said so.
    project_canvas_children(node, rect, elements);

    let suppress_children = suppress_static_text || actionable;
    for child in &node.children {
        project_node(child, bounds, suppress_children, elements);
    }
}

fn project_canvas_children(
    node: &SemanticsNode,
    owner: AccessibilityRect,
    elements: &mut Vec<AccessibilityElement>,
) {
    for child in &node.canvas_children {
        // Canvas bounds are in the publishing node's own space, which is what a
        // draw scope reports; the platform wants window coordinates.
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
            value: None,
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
        });
    }
}

/// Runs the custom action a screen reader picked, resolving it against the
/// *live* semantics tree rather than the published snapshot.
///
/// The snapshot exists to answer "has anything changed", and custom-action
/// handlers are excluded from that comparison on purpose (a recorder closure is
/// rebuilt every collection, so comparing handler identity would re-publish the
/// whole tree over JNI every frame). The consequence is that the snapshot's
/// handlers may be one collection old, so the action is looked up again here,
/// by the identity the platform sent back: the owning node, which drawn control
/// inside it, and the action's index in that control's list.
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
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
        None => false,
    }
}

#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
fn find_semantics_node(node: &SemanticsNode, node_id: NodeId) -> Option<&SemanticsNode> {
    if node.node_id == node_id {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_semantics_node(child, node_id))
}

/// Borrows rather than clones: this runs for every semantics node on every
/// updating frame, and only the few nodes that become elements need an owned
/// label.
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
    for child in &node.children {
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
mod tests {
    use super::*;
    use cranpose_core::NodeId;
    use cranpose_ui::{
        CanvasSemanticsNode, SemanticsAction, SemanticsCallback, SemanticsCustomAction,
        SemanticsNode, SemanticsRole, SemanticsWidgetRole,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

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

    fn element_with(node_id: NodeId, canvas_key: Option<u64>) -> AccessibilityElement {
        AccessibilityElement {
            node_id,
            canvas_key,
            label: "Row".into(),
            bounds: AccessibilityRect::new(0.0, 0.0, 10.0, 10.0),
            ..AccessibilityElement::default()
        }
    }

    /// Drawn controls need ids of their own, and they must not shift when the
    /// app publishes a different set: a screen reader's cursor lives on the id.
    #[test]
    fn drawn_controls_get_distinct_ids_that_do_not_move_with_list_position() {
        let rows: Vec<_> = (0..24).map(|key| element_with(7, Some(key))).collect();
        let ids = element_ids(&rows);

        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "ids collided: {ids:?}");
        assert!(ids.iter().all(|id| *id > 0));

        // Drop the first row: every surviving row keeps the id it had.
        let scrolled = element_ids(&rows[1..]);
        assert_eq!(scrolled, ids[1..]);

        // And the layout node itself is untouched by the drawn controls
        // hanging off it.
        assert_eq!(element_ids(&[element_with(7, None)]), vec![7]);
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

    /// The case the whole canvas-semantics path exists for: a screen drawn as
    /// one `Canvas` has exactly one layout node, so without this the screen
    /// reader is offered one element for a list of controls.
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
        // The canvas is inset, so a drawn rect at y=40 is at y=140 on screen.
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
        // 20 + 0, 100 + 40 — the drawn rect translated into window space.
        assert_eq!(projected[1].bounds.center(), (120.0, 166.0));

        assert_eq!(projected[2].click_label.as_deref(), Some("Reset"));
        assert!(!projected[2].enabled);
    }

    /// A drawn control with nothing to say is not a stop on the screen
    /// reader's tour, and neither is one that was drawn to zero size (the
    /// scrolled-off rows of a scaling list report exactly that).
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

    /// The screen-level node keeps its own announcement (Compose's
    /// `RadialChoiceScreen` reads title + state + hint as one node) and the
    /// drawn options are published underneath it rather than instead of it.
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
        child.canvas_children =
            vec![
                CanvasSemanticsNode::control(42, rect(0.0, 0.0, 40.0, 40.0), "Haptics")
                    .with_custom_action(SemanticsCustomAction::new("Toggle", {
                        let fired = Rc::clone(&fired);
                        move || fired.borrow_mut().push("toggle")
                    })),
            ];
        root.children = vec![child];

        assert!(perform_custom_action(&root, arena_id, None, 0));
        assert!(perform_custom_action(&root, canvas_id, Some(42), 0));

        // Nothing published these, so nothing may run: an unknown node, a
        // drawn control that is not on that node, and an index past the end.
        assert!(!perform_custom_action(&root, 999, None, 0));
        assert!(!perform_custom_action(&root, canvas_id, Some(43), 0));
        assert!(!perform_custom_action(&root, arena_id, None, 1));

        assert_eq!(*fired.borrow(), vec!["pause", "toggle"]);
    }

    /// A recorder closure is rebuilt on every collection, so if a custom action
    /// compared by handler identity the Android bridge would decide the tree
    /// changed and re-serialise it across JNI on every frame.
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
}
