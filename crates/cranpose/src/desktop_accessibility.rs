#![allow(unsafe_code)]

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, CustomAction,
    DeactivationHandler, Invalid, Live, Node, NodeId, Rect, Role, Toggled, Tree, TreeId,
    TreeUpdate,
};
use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{Announcement, LiveRegionMode};
use winit::{event::WindowEvent, event_loop::EventLoopProxy, window::Window};

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};

const ROOT_ID: NodeId = NodeId(u64::MAX);
const ANNOUNCEMENT_ID: NodeId = NodeId(u64::MAX - 1);

#[derive(Clone)]
struct InitialTree(Arc<Mutex<Option<TreeUpdate>>>);

impl ActivationHandler for InitialTree {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[derive(Clone)]
struct Actions {
    queue: Arc<Mutex<Vec<ActionRequest>>>,
    waker: EventLoopProxy,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request);
        self.waker.wake_up();
    }
}

struct Deactivation;

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {}
}

pub(crate) struct DesktopAccessibilityBridge {
    adapter: PlatformAdapter,
    initial_tree: Arc<Mutex<Option<TreeUpdate>>>,
    actions: Arc<Mutex<Vec<ActionRequest>>>,
    centers: HashMap<NodeId, (f32, f32)>,
    pending_custom_actions: Vec<(NodeId, usize)>,
    pending_focus: Vec<NodeId>,
    pending_values: Vec<(NodeId, f32)>,
    pending_texts: Vec<(NodeId, String)>,
    pending_scrolls: Vec<(NodeId, bool)>,
    pending_expansions: Vec<(NodeId, bool)>,
    previous: Vec<AccessibilityElement>,
    seen_revision: Option<u64>,
    announcement: Option<Announcement>,
    announcement_turn: bool,
}

impl DesktopAccessibilityBridge {
    pub(crate) fn new(window: &dyn Window, waker: EventLoopProxy) -> Self {
        let initial_tree = Arc::new(Mutex::new(None));
        let actions = Arc::new(Mutex::new(Vec::new()));
        let adapter = PlatformAdapter::new(
            window,
            InitialTree(Arc::clone(&initial_tree)),
            Actions {
                queue: Arc::clone(&actions),
                waker,
            },
            Deactivation,
        );
        Self {
            adapter,
            initial_tree,
            actions,
            centers: HashMap::new(),
            pending_custom_actions: Vec::new(),
            pending_focus: Vec::new(),
            pending_values: Vec::new(),
            pending_texts: Vec::new(),
            pending_scrolls: Vec::new(),
            pending_expansions: Vec::new(),
            previous: Vec::new(),
            seen_revision: None,
            announcement: None,
            announcement_turn: false,
        }
    }

    pub(crate) fn process_event(&mut self, window: &dyn Window, event: &WindowEvent) {
        self.adapter.process_event(window, event);
    }

    pub(crate) fn sync(&mut self, shell: &mut AppShell<WgpuRenderer>) {
        let mut announcements = accessibility::drain_app_announcements();
        let mut changed = false;
        if let Some(elements) = accessibility::snapshot_if_changed(shell, &mut self.seen_revision)
            && elements != self.previous
        {
            announcements.extend(accessibility::pane_title_announcements(
                &self.previous,
                &elements,
            ));
            self.previous = elements;
            self.centers = accessibility::element_ids(&self.previous)
                .into_iter()
                .zip(&self.previous)
                .map(|(id, element)| (NodeId(id as u64), element.bounds.center()))
                .collect();
            changed = true;
        }
        if let Some(spoken) = join_announcements(announcements) {
            self.announcement = Some(spoken);
            self.announcement_turn = !self.announcement_turn;
            changed = true;
        }
        if !changed {
            return;
        }
        let update = tree_update(
            &self.previous,
            self.announcement.as_ref(),
            self.announcement_turn,
        );
        *self
            .initial_tree
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(update.clone());
        self.adapter.update_if_active(|| update);
    }

    pub(crate) fn drain_clicks(&mut self) -> Vec<(f32, f32)> {
        let requests = std::mem::take(
            &mut *self
                .actions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let mut clicks = Vec::new();
        for request in requests {
            match request.action {
                Action::Click => {
                    if let Some(center) = self.centers.get(&request.target_node) {
                        clicks.push(*center);
                    }
                }
                Action::CustomAction => {
                    if let Some(ActionData::CustomAction(index)) = request.data
                        && index >= 0
                    {
                        self.pending_custom_actions
                            .push((request.target_node, index as usize));
                    }
                }
                Action::Focus => self.pending_focus.push(request.target_node),
                Action::SetValue => match request.data {
                    Some(ActionData::NumericValue(value)) => {
                        self.pending_values
                            .push((request.target_node, value as f32));
                    }
                    Some(ActionData::Value(text)) => {
                        self.pending_texts
                            .push((request.target_node, text.to_string()));
                    }
                    _ => {}
                },
                Action::Expand => self.pending_expansions.push((request.target_node, true)),
                Action::Collapse => self.pending_expansions.push((request.target_node, false)),
                Action::Increment => self.step_value(request.target_node, true),
                Action::Decrement => self.step_value(request.target_node, false),
                Action::ScrollDown | Action::ScrollRight => {
                    self.pending_scrolls.push((request.target_node, true));
                }
                Action::ScrollUp | Action::ScrollLeft => {
                    self.pending_scrolls.push((request.target_node, false));
                }
                _ => {}
            }
        }
        clicks
    }

    /// Queues the value one step up or down from the one an adjustable control
    /// holds now, for a reader that offers a step rather than a value.
    fn step_value(&mut self, target: NodeId, up: bool) {
        let ids = accessibility::element_ids(&self.previous);
        let Some(element) = ids
            .iter()
            .position(|id| NodeId(*id as u64) == target)
            .and_then(|position| self.previous.get(position))
        else {
            return;
        };
        if let Some(progress) = element.progress {
            self.pending_values
                .push((target, accessibility::stepped_value(&progress, up)));
        }
    }

    /// Moves app focus onto the element a screen reader asked for, so the two
    /// agree on what holds focus. Answers whether focus moved.
    pub(crate) fn run_focus_requests(&mut self) -> bool {
        if self.pending_focus.is_empty() {
            return false;
        }
        let pending = std::mem::take(&mut self.pending_focus);
        let ids = accessibility::element_ids(&self.previous);
        let mut moved = false;
        for target in pending {
            let Some(element) = ids
                .iter()
                .position(|id| NodeId(*id as u64) == target)
                .and_then(|position| self.previous.get(position))
            else {
                continue;
            };
            moved |= accessibility::focus_node(element.node_id);
        }
        moved
    }

    pub(crate) fn run_custom_actions(&mut self, shell: &mut AppShell<WgpuRenderer>) -> bool {
        if self.pending_custom_actions.is_empty() {
            return false;
        }
        let pending = std::mem::take(&mut self.pending_custom_actions);
        let ids = accessibility::element_ids(&self.previous);
        let mut ran = false;
        for (target, index) in pending {
            let Some(element) = ids
                .iter()
                .position(|id| NodeId(*id as u64) == target)
                .and_then(|position| self.previous.get(position))
            else {
                continue;
            };
            let (node_id, canvas_key) = (element.node_id, element.canvas_key);
            let named = accessibility::reader_actions(element).len();
            ran |= accessibility::run_reader_action(shell, |root| {
                accessibility::perform_listed_action(root, node_id, canvas_key, named, index)
            });
        }
        ran
    }

    /// Pages a scroll container a screen reader asked to move on or back.
    /// Answers whether one moved.
    pub(crate) fn run_scroll_requests(&mut self, shell: &mut AppShell<WgpuRenderer>) -> bool {
        if self.pending_scrolls.is_empty() {
            return false;
        }
        let pending = std::mem::take(&mut self.pending_scrolls);
        let ids = accessibility::element_ids(&self.previous);
        let mut moved = false;
        for (target, forward) in pending {
            let Some(element) = ids
                .iter()
                .position(|id| NodeId(*id as u64) == target)
                .and_then(|position| self.previous.get(position))
            else {
                continue;
            };
            let (dx, dy) = accessibility::page_delta(element, forward);
            let node_id = element.node_id;
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::scroll_by(root, node_id, dx, dy)
            });
        }
        moved
    }

    /// Moves the value of an adjustable control a screen reader asked to
    /// change. Answers whether one took the new value.
    pub(crate) fn run_value_requests(&mut self, shell: &mut AppShell<WgpuRenderer>) -> bool {
        let mut moved = false;
        for (target, value) in std::mem::take(&mut self.pending_values) {
            let Some(node_id) = self.node_id_for(target) else {
                continue;
            };
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::set_progress(root, node_id, value)
            });
        }
        for (target, open) in std::mem::take(&mut self.pending_expansions) {
            let Some(node_id) = self.node_id_for(target) else {
                continue;
            };
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::set_expanded(root, node_id, open)
            });
        }
        for (target, text) in std::mem::take(&mut self.pending_texts) {
            let Some(node_id) = self.node_id_for(target) else {
                continue;
            };
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::set_text(root, node_id, &text)
            });
        }
        moved
    }

    /// The live node behind the accesskit id of a published element.
    fn node_id_for(&self, target: NodeId) -> Option<cranpose_core::NodeId> {
        let ids = accessibility::element_ids(&self.previous);
        let position = ids.iter().position(|id| NodeId(*id as u64) == target)?;
        self.previous.get(position).map(|element| element.node_id)
    }
}

fn tree_update(
    elements: &[AccessibilityElement],
    announcement: Option<&Announcement>,
    announcement_turn: bool,
) -> TreeUpdate {
    let ids = accessibility::element_ids(elements);
    let mut nested = nested_children(&ids, elements);
    let mut children = nested.remove(&None).unwrap_or_default();
    if announcement.is_some() {
        children.push(ANNOUNCEMENT_ID);
    }
    let mut root = Node::new(Role::Window);
    root.set_label("Cranpose application");
    root.set_children(children);
    let mut nodes = vec![(ROOT_ID, root)];
    nodes.extend(ids.iter().zip(elements).map(|(id, element)| {
        let mut node = accesskit_node(element);
        if element.canvas_key.is_none()
            && let Some(children) = nested.remove(&Some(element.node_id))
        {
            node.set_children(children);
        }
        (NodeId(*id as u64), node)
    }));
    if let Some(announcement) = announcement {
        nodes.push((
            ANNOUNCEMENT_ID,
            announcement_node(announcement, announcement_turn),
        ));
    }
    let mut tree = Tree::new(ROOT_ID);
    tree.toolkit_name = Some("Cranpose".into());
    TreeUpdate {
        nodes,
        tree: Some(tree),
        tree_id: TreeId::ROOT,
        focus: focused_node(&ids, elements),
    }
}

/// The accesskit ids under each container, keyed by the container's node, and
/// under `None` the ones with nothing above them, so a reader hears "list, 12
/// items" and "tab 2 of 5" from the shape of the tree. A row whose container
/// was not published sits at the root rather than out of reach.
fn nested_children(
    ids: &[i32],
    elements: &[AccessibilityElement],
) -> HashMap<Option<cranpose_core::NodeId>, Vec<NodeId>> {
    let containers: HashSet<cranpose_core::NodeId> = elements
        .iter()
        .filter(|element| element.canvas_key.is_none())
        .map(|element| element.node_id)
        .collect();
    let mut nested: HashMap<Option<cranpose_core::NodeId>, Vec<NodeId>> = HashMap::new();
    for (id, element) in ids.iter().zip(elements) {
        let parent = element
            .scroll_parent
            .filter(|parent| containers.contains(parent));
        nested.entry(parent).or_default().push(NodeId(*id as u64));
    }
    nested
}

/// One control as accesskit describes it to a screen reader.
fn accesskit_node(element: &AccessibilityElement) -> Node {
    let scrolls = element.vertical_scroll.is_some() || element.horizontal_scroll.is_some();
    let role = match element.progress {
        Some(_) => Role::Slider,
        None if element.pane_title.is_some() => Role::Region,
        None if scrolls && element.label.is_empty() => scroll_role(element),
        None if element.password => Role::PasswordInput,
        None => accesskit_role(element.role),
    };
    let mut node = Node::new(role);
    if element.role == AccessibilityRole::StaticText {
        node.set_value(element.label.as_str());
    } else {
        node.set_label(element.label.as_str());
    }
    if let Some(title) = &element.pane_title {
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

/// A node with no control behind it, whose only job is to hold text a screen
/// reader reads at once. accesskit reads a live node again when its value
/// changes, so the same text twice in a row carries a trailing space one time
/// out of two to stay a change.
fn announcement_node(announcement: &Announcement, turn: bool) -> Node {
    let mut node = Node::new(Role::Label);
    let text = if turn {
        format!("{} ", announcement.text)
    } else {
        announcement.text.clone()
    };
    node.set_value(text);
    node.set_live(accesskit_live(announcement.mode));
    node
}

fn accesskit_live(mode: LiveRegionMode) -> Live {
    match mode {
        LiveRegionMode::Polite => Live::Polite,
        LiveRegionMode::Assertive => Live::Assertive,
    }
}

fn join_announcements(announcements: Vec<Announcement>) -> Option<Announcement> {
    let mode = announcements
        .iter()
        .map(|announcement| announcement.mode)
        .fold(LiveRegionMode::Polite, |mode, next| match next {
            LiveRegionMode::Assertive => LiveRegionMode::Assertive,
            LiveRegionMode::Polite => mode,
        });
    let text = announcements
        .into_iter()
        .map(|announcement| announcement.text)
        .collect::<Vec<_>>()
        .join(". ");
    (!text.is_empty()).then_some(Announcement { text, mode })
}

/// A scroll container with no label of its own: a list when it says how many
/// rows it holds, a plain scroll view otherwise.
fn scroll_role(element: &AccessibilityElement) -> Role {
    if element.collection.is_some() {
        Role::List
    } else {
        Role::ScrollView
    }
}

/// The accesskit role a screen reader reads the control as.
fn accesskit_role(role: AccessibilityRole) -> Role {
    match role {
        AccessibilityRole::Button => Role::Button,
        AccessibilityRole::StaticText => Role::Label,
        AccessibilityRole::TextField => Role::TextInput,
        AccessibilityRole::Checkbox => Role::CheckBox,
        AccessibilityRole::Switch => Role::Switch,
        AccessibilityRole::RadioButton => Role::RadioButton,
        AccessibilityRole::Tab => Role::Tab,
        AccessibilityRole::Image => Role::Image,
        AccessibilityRole::DropdownList => Role::ComboBox,
        AccessibilityRole::ValuePicker => Role::SpinButton,
        AccessibilityRole::Header => Role::Heading,
        AccessibilityRole::Dialog => Role::Dialog,
    }
}

/// What the control says about itself beyond its name: its value, the state
/// description, and whether it is selected, toggled or disabled.
fn apply_state(node: &mut Node, element: &AccessibilityElement) {
    if let Some(value) = &element.value {
        node.set_value(value.as_str());
    }
    if let Some(state) = accessibility::state_with_error(element) {
        node.set_description(state.as_str());
    }
    if element.error.is_some() {
        node.set_invalid(Invalid::True);
    }
    if let Some(item) = element.collection_item {
        node.set_position_in_set(item.position);
        node.set_size_of_set(item.count);
    }
    if let Some(selected) = element.selected {
        node.set_selected(selected);
    }
    if let Some(toggled) = element.toggled {
        node.set_toggled(if toggled {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    if !element.enabled {
        node.set_disabled();
    }
    if let Some(mode) = element.live_region {
        node.set_live(accesskit_live(mode));
    }
    if let Some(progress) = element.progress {
        node.set_numeric_value(progress.current as f64);
        node.set_min_numeric_value(progress.start as f64);
        node.set_max_numeric_value(progress.end as f64);
        node.set_numeric_value_step(progress.step() as f64);
    }
    if let Some(range) = element.vertical_scroll {
        node.set_scroll_y(range.value as f64);
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(range.max_value as f64);
    }
    if let Some(range) = element.horizontal_scroll {
        node.set_scroll_x(range.value as f64);
        node.set_scroll_x_min(0.0);
        node.set_scroll_x_max(range.max_value as f64);
    }
}

/// What a screen reader can do with the control: activate it, run one of the
/// actions it lists, or put focus on it. accesskit has no long press of its
/// own, so a long press is the last action in that list.
fn apply_actions(node: &mut Node, element: &AccessibilityElement) {
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
                    description: label.into(),
                })
                .collect::<Vec<_>>(),
        );
    }
    if let Some(expanded) = element.expanded {
        node.set_expanded(expanded);
        node.add_action(if expanded {
            Action::Collapse
        } else {
            Action::Expand
        });
    }
    if element.focusable {
        node.add_action(Action::Focus);
    }
    if element.role == AccessibilityRole::TextField {
        node.add_action(Action::SetValue);
    }
    if element.adjustable {
        node.add_action(Action::SetValue);
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
    }
    if let Some(range) = element.vertical_scroll {
        if range.can_scroll_forward() {
            node.add_action(Action::ScrollDown);
        }
        if range.can_scroll_backward() {
            node.add_action(Action::ScrollUp);
        }
    }
    if let Some(range) = element.horizontal_scroll {
        if range.can_scroll_forward() {
            node.add_action(Action::ScrollRight);
        }
        if range.can_scroll_backward() {
            node.add_action(Action::ScrollLeft);
        }
    }
}

/// The node a screen reader should sit on: the control the app focused, or the
/// window when nothing holds focus.
fn focused_node(ids: &[i32], elements: &[AccessibilityElement]) -> NodeId {
    ids.iter()
        .zip(elements)
        .find(|(_, element)| element.focused)
        .map(|(id, _)| NodeId(*id as u64))
        .unwrap_or(ROOT_ID)
}

#[cfg(target_os = "macos")]
struct PlatformAdapter(accesskit_macos::SubclassingAdapter);

#[cfg(target_os = "macos")]
impl PlatformAdapter {
    fn new(
        window: &dyn Window,
        activation: impl 'static + ActivationHandler,
        actions: impl 'static + ActionHandler,
        _deactivation: impl 'static + DeactivationHandler,
    ) -> Self {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let RawWindowHandle::AppKit(handle) = window.window_handle().unwrap().as_raw() else {
            unreachable!("macOS desktop window did not expose an AppKit view")
        };
        Self(unsafe {
            accesskit_macos::SubclassingAdapter::new(handle.ns_view.as_ptr(), activation, actions)
        })
    }

    fn process_event(&mut self, _window: &dyn Window, event: &WindowEvent) {
        if let WindowEvent::Focused(focused) = event
            && let Some(events) = self.0.update_view_focus_state(*focused)
        {
            events.raise();
        }
    }

    fn update_if_active(&mut self, update: impl FnOnce() -> TreeUpdate) {
        if let Some(events) = self.0.update_if_active(update) {
            events.raise();
        }
    }
}

#[cfg(target_os = "windows")]
struct PlatformAdapter(accesskit_windows::SubclassingAdapter);

#[cfg(target_os = "windows")]
impl PlatformAdapter {
    fn new(
        window: &dyn Window,
        activation: impl 'static + ActivationHandler,
        actions: impl 'static + ActionHandler + Send,
        _deactivation: impl 'static + DeactivationHandler,
    ) -> Self {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw() else {
            unreachable!("Windows desktop window did not expose an HWND")
        };
        Self(accesskit_windows::SubclassingAdapter::new(
            accesskit_windows::HWND(handle.hwnd.get() as *mut _),
            activation,
            actions,
        ))
    }

    fn process_event(&mut self, _window: &dyn Window, _event: &WindowEvent) {}

    fn update_if_active(&mut self, update: impl FnOnce() -> TreeUpdate) {
        if let Some(events) = self.0.update_if_active(update) {
            events.raise();
        }
    }
}

#[cfg(target_os = "linux")]
struct PlatformAdapter(accesskit_unix::Adapter);

#[cfg(target_os = "linux")]
impl PlatformAdapter {
    fn new(
        _window: &dyn Window,
        activation: impl 'static + ActivationHandler + Send,
        actions: impl 'static + ActionHandler + Send,
        deactivation: impl 'static + DeactivationHandler + Send,
    ) -> Self {
        Self(accesskit_unix::Adapter::new(
            activation,
            actions,
            deactivation,
        ))
    }

    fn process_event(&mut self, window: &dyn Window, event: &WindowEvent) {
        match event {
            WindowEvent::Moved(_) | WindowEvent::SurfaceResized(_) => {
                let Ok(outer_origin) = window.outer_position() else {
                    return;
                };
                let surface_origin = window.surface_position();
                let outer_position = (outer_origin.x as f64, outer_origin.y as f64);
                let outer_size: (_, _) = window.outer_size().cast::<f64>().into();
                let inner_position = (
                    (outer_origin.x + surface_origin.x) as f64,
                    (outer_origin.y + surface_origin.y) as f64,
                );
                let inner_size: (_, _) = window.surface_size().cast::<f64>().into();
                self.0.set_root_window_bounds(
                    Rect::from_origin_size(outer_position, outer_size),
                    Rect::from_origin_size(inner_position, inner_size),
                );
            }
            WindowEvent::Focused(focused) => self.0.update_window_focus_state(*focused),
            _ => {}
        }
    }

    fn update_if_active(&mut self, update: impl FnOnce() -> TreeUpdate) {
        self.0.update_if_active(update);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::AccessibilityRect;

    #[test]
    fn the_tree_points_at_the_focused_control_and_offers_focus_on_the_others() {
        let elements = vec![
            AccessibilityElement {
                node_id: 7,
                label: "Name".into(),
                bounds: AccessibilityRect::new(0.0, 0.0, 80.0, 44.0),
                role: AccessibilityRole::TextField,
                focusable: true,
                ..AccessibilityElement::default()
            },
            AccessibilityElement {
                node_id: 8,
                label: "Save".into(),
                bounds: AccessibilityRect::new(0.0, 50.0, 80.0, 44.0),
                role: AccessibilityRole::Button,
                clickable: true,
                focusable: true,
                focused: true,
                ..AccessibilityElement::default()
            },
        ];

        let update = tree_update(&elements, None, false);
        let ids = accessibility::element_ids(&elements);

        assert_eq!(
            update.focus,
            NodeId(ids[1] as u64),
            "a screen reader reads focus from the tree, and it sits on Save"
        );
        for (id, _) in ids.iter().zip(&elements) {
            let node = update
                .nodes
                .iter()
                .find(|(node_id, _)| *node_id == NodeId(*id as u64))
                .map(|(_, node)| node)
                .expect("every element is in the tree");
            assert!(
                node.supports_action(Action::Focus),
                "a control focus can land on offers the Focus action"
            );
        }
    }

    #[test]
    fn a_tree_with_nothing_focused_leaves_focus_on_the_window() {
        let elements = vec![AccessibilityElement {
            node_id: 7,
            label: "Name".into(),
            bounds: AccessibilityRect::new(0.0, 0.0, 80.0, 44.0),
            role: AccessibilityRole::TextField,
            focusable: true,
            ..AccessibilityElement::default()
        }];

        assert_eq!(tree_update(&elements, None, false).focus, ROOT_ID);
    }

    #[test]
    fn desktop_tree_maps_controls_to_native_roles_and_click_actions() {
        let elements = vec![
            AccessibilityElement {
                node_id: 7,
                label: "Items".into(),
                bounds: AccessibilityRect::new(10.0, 20.0, 80.0, 44.0),
                role: AccessibilityRole::Button,
                clickable: true,
                ..AccessibilityElement::default()
            },
            AccessibilityElement {
                node_id: 8,
                label: "Receipts".into(),
                bounds: AccessibilityRect::new(10.0, 70.0, 120.0, 24.0),
                role: AccessibilityRole::StaticText,
                ..AccessibilityElement::default()
            },
        ];

        let update = tree_update(&elements, None, false);
        assert_eq!(update.tree.as_ref().map(|tree| tree.root), Some(ROOT_ID));
        let button = &update.nodes[1].1;
        assert_eq!(button.role(), Role::Button);
        assert_eq!(button.label(), Some("Items"));
        assert!(button.supports_action(Action::Click));
        let label = &update.nodes[2].1;
        assert_eq!(label.role(), Role::Label);
        assert_eq!(label.value(), Some("Receipts"));
    }

    #[test]
    fn drawn_controls_sharing_a_layout_node_become_separate_accesskit_nodes() {
        let elements = vec![
            AccessibilityElement {
                node_id: 4,
                canvas_key: Some(1),
                label: "Haptics".into(),
                state_description: Some("On".into()),
                bounds: AccessibilityRect::new(0.0, 0.0, 100.0, 50.0),
                role: AccessibilityRole::Switch,
                clickable: true,
                toggled: Some(true),
                ..AccessibilityElement::default()
            },
            AccessibilityElement {
                node_id: 4,
                canvas_key: Some(2),
                label: "Sound effects".into(),
                bounds: AccessibilityRect::new(0.0, 60.0, 100.0, 50.0),
                role: AccessibilityRole::Switch,
                clickable: true,
                toggled: Some(false),
                enabled: false,
                ..AccessibilityElement::default()
            },
        ];

        let update = tree_update(&elements, None, false);
        let ids: Vec<_> = update.nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), 3, "root plus one node per drawn control");
        assert_ne!(ids[1], ids[2]);
        assert_eq!(update.nodes[0].1.children(), &ids[1..]);

        let haptics = &update.nodes[1].1;
        assert_eq!(haptics.role(), Role::Switch);
        assert_eq!(haptics.toggled(), Some(Toggled::True));
        assert_eq!(haptics.description(), Some("On"));
        assert!(!haptics.is_disabled());

        let sound = &update.nodes[2].1;
        assert_eq!(sound.toggled(), Some(Toggled::False));
        assert!(sound.is_disabled());
    }

    #[test]
    fn an_announcement_rides_along_as_a_live_node_under_the_window() {
        let elements = vec![AccessibilityElement {
            node_id: 1,
            label: "Import".into(),
            bounds: AccessibilityRect::new(0.0, 0.0, 100.0, 40.0),
            role: AccessibilityRole::Button,
            clickable: true,
            ..AccessibilityElement::default()
        }];
        let announcement = Announcement {
            text: "Seven receipts imported".into(),
            mode: LiveRegionMode::Assertive,
        };

        let update = tree_update(&elements, Some(&announcement), false);
        assert_eq!(update.nodes.len(), 3, "root, the button, the announcement");
        let (id, spoken) = &update.nodes[2];
        assert_eq!(*id, ANNOUNCEMENT_ID);
        assert_eq!(spoken.value(), Some("Seven receipts imported"));
        assert_eq!(spoken.live(), Some(Live::Assertive));
        assert!(update.nodes[0].1.children().contains(&ANNOUNCEMENT_ID));

        let again = tree_update(&elements, Some(&announcement), true);
        assert_eq!(again.nodes[2].1.value(), Some("Seven receipts imported "));
    }

    #[test]
    fn a_live_control_tells_accesskit_how_urgent_it_is() {
        let elements = vec![AccessibilityElement {
            node_id: 1,
            label: "3 receipts left".into(),
            bounds: AccessibilityRect::new(0.0, 0.0, 100.0, 40.0),
            live_region: Some(LiveRegionMode::Polite),
            ..AccessibilityElement::default()
        }];

        let update = tree_update(&elements, None, false);
        assert_eq!(update.nodes[1].1.live(), Some(Live::Polite));
    }

    #[test]
    fn an_adjustable_control_reads_as_a_slider_with_a_value_and_a_way_to_move_it() {
        let elements = vec![AccessibilityElement {
            node_id: 1,
            label: "Volume".into(),
            state_description: Some("40 %".into()),
            bounds: AccessibilityRect::new(0.0, 0.0, 200.0, 40.0),
            progress: Some(cranpose_ui::ProgressBarRangeInfo::new(0.4, 0.0, 1.0, 0)),
            adjustable: true,
            ..AccessibilityElement::default()
        }];

        let update = tree_update(&elements, None, false);
        let slider = &update.nodes[1].1;
        assert_eq!(slider.role(), Role::Slider);
        let near =
            |value: Option<f64>, want: f64| value.is_some_and(|value| (value - want).abs() < 1e-6);
        assert!(near(slider.numeric_value(), 0.4));
        assert!(near(slider.min_numeric_value(), 0.0));
        assert!(near(slider.max_numeric_value(), 1.0));
        assert!(near(slider.numeric_value_step(), 0.1));
        assert!(slider.supports_action(Action::SetValue));
        assert!(slider.supports_action(Action::Increment));
        assert!(slider.supports_action(Action::Decrement));
    }

    #[test]
    fn several_announcements_in_one_frame_become_one_line() {
        let joined = join_announcements(vec![
            Announcement {
                text: "Import done".into(),
                mode: LiveRegionMode::Polite,
            },
            Announcement {
                text: "Two receipts failed".into(),
                mode: LiveRegionMode::Assertive,
            },
        ])
        .expect("two announcements make one");
        assert_eq!(joined.text, "Import done. Two receipts failed");
        assert_eq!(joined.mode, LiveRegionMode::Assertive);
        assert!(join_announcements(Vec::new()).is_none());
    }

    #[test]
    fn rows_sit_under_their_list_in_the_desktop_tree() {
        let list = AccessibilityElement {
            node_id: 6,
            bounds: AccessibilityRect::new(0.0, 0.0, 400.0, 600.0),
            vertical_scroll: Some(cranpose_ui::ScrollAxisRange::new(0.0, 900.0, false)),
            ..AccessibilityElement::default()
        };
        let row = AccessibilityElement {
            node_id: 9,
            label: "Milk".into(),
            bounds: AccessibilityRect::new(0.0, 10.0, 400.0, 40.0),
            scroll_parent: Some(6),
            ..AccessibilityElement::default()
        };

        let update = tree_update(&[list, row], None, false);
        let ids: Vec<_> = update.nodes.iter().map(|(id, _)| *id).collect();

        assert_eq!(
            update.nodes[0].1.children(),
            &ids[1..2],
            "the root holds the list alone"
        );
        assert_eq!(
            update.nodes[1].1.children(),
            &ids[2..],
            "the list holds its row"
        );
    }
}
