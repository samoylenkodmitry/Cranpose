#![allow(unsafe_code)]

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, CustomAction,
    DeactivationHandler, Invalid, Live, Node, NodeId, Point, Rect, Role, TextDirection,
    TextPosition, TextSelection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{Announcement, LiveRegionMode};
use winit::{event::WindowEvent, event_loop::EventLoopProxy, window::Window};

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};

const ROOT_ID: NodeId = NodeId(u64::MAX);
const ANNOUNCEMENT_ID: NodeId = NodeId(u64::MAX - 1);
/// Set on the accesskit id of a text run, above the bits that hold the run's
/// index and the virtual id of the field it belongs to.
const TEXT_RUN_BIT: u64 = 1 << 40;
/// The most characters one text run holds: accesskit counts the words of a
/// run in a byte, so a long line is broken into runs at a space.
const TEXT_RUN_CHARS: usize = 200;

/// The tree a reader gets when it connects, and the flag that says one did.
#[derive(Clone)]
struct InitialTree {
    tree: Arc<Mutex<Option<TreeUpdate>>>,
    reader_connected: Arc<AtomicBool>,
}

impl ActivationHandler for InitialTree {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.reader_connected.store(true, Ordering::Relaxed);
        self.tree
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

/// Clears the reader flag when the reader lets go of the tree.
struct Deactivation(Arc<AtomicBool>);

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

pub(crate) struct DesktopAccessibilityBridge {
    adapter: PlatformAdapter,
    initial_tree: Arc<Mutex<Option<TreeUpdate>>>,
    actions: Arc<Mutex<Vec<ActionRequest>>>,
    reader_connected: Arc<AtomicBool>,
    centers: HashMap<NodeId, (f32, f32)>,
    pending_custom_actions: Vec<(NodeId, usize)>,
    pending_focus: Vec<NodeId>,
    pending_values: Vec<(NodeId, f32)>,
    pending_texts: Vec<(NodeId, String)>,
    pending_selections: Vec<(NodeId, TextSelection)>,
    pending_scrolls: Vec<(NodeId, bool)>,
    pending_jumps: Vec<(NodeId, usize)>,
    pending_expansions: Vec<(NodeId, bool)>,
    previous: Vec<AccessibilityElement>,
    seen_revision: Option<u64>,
    announcement: Option<Announcement>,
    announcement_turn: bool,
    options: crate::desktop_accessibility_options::OptionsProbe,
}

impl DesktopAccessibilityBridge {
    pub(crate) fn new(window: &dyn Window, waker: EventLoopProxy, robot_drives: bool) -> Self {
        let initial_tree = Arc::new(Mutex::new(None));
        let actions = Arc::new(Mutex::new(Vec::new()));
        let reader_connected = Arc::new(AtomicBool::new(false));
        let adapter = PlatformAdapter::new(
            window,
            InitialTree {
                tree: Arc::clone(&initial_tree),
                reader_connected: Arc::clone(&reader_connected),
            },
            Actions {
                queue: Arc::clone(&actions),
                waker,
            },
            Deactivation(Arc::clone(&reader_connected)),
        );
        Self {
            adapter,
            initial_tree,
            actions,
            reader_connected,
            centers: HashMap::new(),
            pending_custom_actions: Vec::new(),
            pending_focus: Vec::new(),
            pending_values: Vec::new(),
            pending_texts: Vec::new(),
            pending_selections: Vec::new(),
            pending_scrolls: Vec::new(),
            pending_jumps: Vec::new(),
            pending_expansions: Vec::new(),
            previous: Vec::new(),
            seen_revision: None,
            announcement: None,
            announcement_turn: false,
            options: crate::desktop_accessibility_options::OptionsProbe::start(!robot_drives),
        }
    }

    pub(crate) fn process_event(&mut self, window: &dyn Window, event: &WindowEvent) {
        self.adapter.process_event(window, event);
    }

    pub(crate) fn sync(&mut self, shell: &mut AppShell<WgpuRenderer>) {
        let reader_on = cranpose_services::AccessibilityState {
            screen_reader_on: self.reader_connected.load(Ordering::Relaxed),
        };
        if cranpose_services::set_platform_accessibility_state(reader_on) {
            shell.request_root_render();
        }
        self.options.apply(shell);
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
        requests
            .into_iter()
            .filter_map(|request| self.queue_request(request))
            .collect()
    }

    /// Notes one screen reader request for the frame loop to run against the
    /// live tree. A click is the exception: it answers the point on screen,
    /// which the shell takes as a tap.
    fn queue_request(&mut self, request: ActionRequest) -> Option<(f32, f32)> {
        let target = request.target_node;
        match request.action {
            Action::Click => return self.centers.get(&target).copied(),
            Action::Focus => self.pending_focus.push(target),
            Action::Expand => self.pending_expansions.push((target, true)),
            Action::Collapse => self.pending_expansions.push((target, false)),
            Action::Increment => self.step_value(target, true),
            Action::Decrement => self.step_value(target, false),
            Action::ScrollDown | Action::ScrollRight => self.pending_scrolls.push((target, true)),
            Action::ScrollUp | Action::ScrollLeft => self.pending_scrolls.push((target, false)),
            _ => self.queue_request_with_data(request),
        }
        None
    }

    /// The requests that carry a value of their own: which custom action a
    /// reader picked, the number or the text it handed a control, and the row
    /// it asked a list for.
    fn queue_request_with_data(&mut self, request: ActionRequest) {
        let target = request.target_node;
        match (request.action, request.data) {
            (Action::CustomAction, Some(ActionData::CustomAction(index))) if index >= 0 => {
                self.pending_custom_actions.push((target, index as usize));
            }
            (Action::SetValue, Some(ActionData::NumericValue(value))) => {
                self.pending_values.push((target, value as f32));
            }
            (Action::SetValue, Some(ActionData::Value(text))) => {
                self.pending_texts.push((target, text.to_string()));
            }
            (Action::SetTextSelection, Some(ActionData::SetTextSelection(selection))) => {
                self.pending_selections.push((target, selection));
            }
            (Action::SetScrollOffset, Some(ActionData::SetScrollOffset(point))) => {
                if let Some(index) = row_offset(&point) {
                    self.pending_jumps.push((target, index));
                }
            }
            _ => {}
        }
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
    pub(crate) fn run_focus_requests(&mut self, shell: &mut AppShell<WgpuRenderer>) -> bool {
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
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::focus_node(root, element.node_id)
            });
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
        moved | self.run_jump_requests(shell)
    }

    /// Puts the row a screen reader named by number in view. accesskit has no
    /// scroll-to-index action, so the offset a reader sets on a list counts
    /// rows. Answers whether a list moved.
    pub(crate) fn run_jump_requests(&mut self, shell: &mut AppShell<WgpuRenderer>) -> bool {
        let mut moved = false;
        for (target, index) in std::mem::take(&mut self.pending_jumps) {
            let Some(node_id) = self.node_id_for(target) else {
                continue;
            };
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::scroll_to_index(root, node_id, index)
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
        for (target, selection) in std::mem::take(&mut self.pending_selections) {
            let Some((node_id, anchor, focus)) =
                selection_chars(&self.previous, target, &selection)
            else {
                continue;
            };
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::set_text_selection_chars(root, node_id, anchor, focus)
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
    for (id, element) in ids.iter().zip(elements) {
        let mut node = accesskit_node(element);
        let mut below = if element.canvas_key.is_none() {
            nested.remove(&Some(element.node_id)).unwrap_or_default()
        } else {
            Vec::new()
        };
        let runs = text_run_nodes(*id, element);
        below.extend(runs.iter().map(|(run_id, _)| *run_id));
        if !below.is_empty() {
            node.set_children(below);
        }
        apply_text_selection(&mut node, *id, element);
        nodes.push((NodeId(*id as u64), node));
        nodes.extend(runs);
    }
    if let Some(announcement) = announcement {
        nodes.push((
            ANNOUNCEMENT_ID,
            announcement_node(announcement, announcement_turn),
        ));
    }
    let mut tree = TreeInfo::new(ROOT_ID);
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
        Some(_) if element.adjustable && element.role != AccessibilityRole::ValuePicker => {
            Role::Slider
        }
        Some(_) => accesskit_role(element.role),
        None if element.pane_title.is_some() => Role::Region,
        None if scrolls && element.label.is_empty() && !element.role.is_named_container() => {
            scroll_role(element)
        }
        None if element.password => Role::PasswordInput,
        None if element.role.is_text_field() && holds_lines(element) => Role::MultilineTextInput,
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

/// The accesskit role of each role a reader names.
const ACCESSKIT_ROLES: [(AccessibilityRole, Role); 24] = [
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
fn accesskit_role(role: AccessibilityRole) -> Role {
    accessibility::role_entry(&ACCESSKIT_ROLES, role, Role::Unknown)
}

/// What the control says about itself beyond its name: its value, the state
/// description, and whether it is selected, toggled or disabled.
fn apply_state(node: &mut Node, element: &AccessibilityElement) {
    if element.is_modal {
        node.set_modal();
    }
    if let Some(value) = &element.value {
        node.set_value(value.as_str());
    }
    if let Some(state) = accessibility::state_with_error(element) {
        node.set_description(state.as_str());
    }
    if let Some(language) = &element.language {
        node.set_language(language.as_str());
    }
    if element.error.is_some() {
        node.set_invalid(Invalid::True);
    }
    if let Some(item) = element.collection_item {
        node.set_position_in_set(item.position.saturating_sub(1));
        node.set_size_of_set(item.count);
    }
    if let Some(selected) = element
        .selected
        .filter(|_| element.role != AccessibilityRole::RadioButton)
    {
        node.set_selected(selected);
    }
    if let Some(toggled) = accessibility::checked_state(element) {
        node.set_toggled(if toggled {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    if !element.enabled {
        node.set_disabled();
    }
    if let Some(expanded) = element.expanded {
        node.set_expanded(expanded);
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
    if let Some(expanded) = element.expanded {
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
    if element.text_selection.is_some() {
        node.add_action(Action::SetTextSelection);
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
    apply_row_offset(node, element);
}

/// The rows a screen reader may ask a list for. accesskit 0.24.1 has no
/// scroll-to-index action, so the list carries its rows as the scroll offset
/// of the axis it runs along: the reader reads the range, sets an offset in
/// rows, and the list puts that row at the top.
fn apply_row_offset(node: &mut Node, element: &AccessibilityElement) {
    let count = accessibility::row_count(element);
    if count == 0 {
        return;
    }
    let last = (count - 1) as f64;
    let first_visible = element
        .vertical_scroll
        .or(element.horizontal_scroll)
        .map_or(0.0, |range| f64::from(range.value).clamp(0.0, last));
    if element.vertical_scroll.is_some() {
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

/// The row a screen reader named through an accesskit scroll offset. The list
/// publishes its rows on one axis and leaves the other at zero, so the larger
/// of the two is the row the reader asked for.
fn row_offset(point: &Point) -> Option<usize> {
    let row = point.x.max(point.y);
    (row.is_finite() && row >= 0.0).then(|| row.round() as usize)
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

/// Whether a field's text runs over more than one line.
fn holds_lines(element: &AccessibilityElement) -> bool {
    element
        .value
        .as_deref()
        .is_some_and(|value| value.contains('\n'))
}

/// One stretch of a field's text that accesskit reads as a text run: where it
/// starts and ends in bytes, and where it starts in characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextRunSpan {
    start_byte: usize,
    end_byte: usize,
    start_char: usize,
}

/// The text runs of a field: one per line, with the line break at the end of
/// the line it ends, and a line longer than [`TEXT_RUN_CHARS`] broken at the
/// last space before that, because accesskit counts the words of a run in a
/// byte. An empty text is one empty run, so a caret has a place to sit.
fn text_runs(text: &str) -> Vec<TextRunSpan> {
    let mut runs = Vec::new();
    let mut start_byte = 0;
    let mut start_char = 0;
    let mut chars = 0;
    let mut last_space: Option<(usize, usize)> = None;
    for (byte, character) in text.char_indices() {
        chars += 1;
        let end = byte + character.len_utf8();
        if character == '\n' {
            runs.push(TextRunSpan {
                start_byte,
                end_byte: end,
                start_char,
            });
            start_byte = end;
            start_char += chars;
            chars = 0;
            last_space = None;
            continue;
        }
        if character.is_whitespace() {
            last_space = Some((end, chars));
        }
        if chars >= TEXT_RUN_CHARS {
            let (cut_byte, cut_chars) = last_space.unwrap_or((end, chars));
            runs.push(TextRunSpan {
                start_byte,
                end_byte: cut_byte,
                start_char,
            });
            start_byte = cut_byte;
            start_char += cut_chars;
            chars -= cut_chars;
            last_space = None;
        }
    }
    if chars > 0 || runs.is_empty() {
        runs.push(TextRunSpan {
            start_byte,
            end_byte: text.len(),
            start_char,
        });
    }
    runs
}

/// Where the words of a run start, in characters: after each stretch of
/// spaces. The first character starts a word on its own, so it is left out.
fn word_starts(run: &str) -> Vec<u8> {
    let mut starts = Vec::new();
    let mut after_space = false;
    for (index, character) in run.chars().enumerate() {
        if index > 0 && after_space && !character.is_whitespace() {
            starts.push(index as u8);
        }
        after_space = character.is_whitespace();
    }
    starts
}

/// The accesskit id of one text run of a field.
fn text_run_id(field_id: i32, run_index: usize) -> NodeId {
    NodeId(TEXT_RUN_BIT | ((run_index as u64) << 32) | field_id as u64)
}

/// The field and the run index behind the accesskit id of a text run, or
/// nothing for the id of a control.
fn text_run_owner(id: NodeId) -> Option<(i32, usize)> {
    if id.0 & TEXT_RUN_BIT == 0 {
        return None;
    }
    let field_id = (id.0 & 0xffff_ffff) as i32;
    let run_index = ((id.0 >> 32) & 0xff) as usize;
    Some((field_id, run_index))
}

/// The text runs under an editable field, each with the text it holds, the
/// length of each character in bytes and where its words start, so a screen
/// reader walks the text by character, word and line and puts the caret
/// where the user asks. A control that is not a field, or one that holds a
/// secret, has none.
fn text_run_nodes(field_id: i32, element: &AccessibilityElement) -> Vec<(NodeId, Node)> {
    let (Some(value), Some(_)) = (&element.value, element.text_selection) else {
        return Vec::new();
    };
    text_runs(value)
        .into_iter()
        .enumerate()
        .take(0xff)
        .map(|(index, span)| {
            let text = &value[span.start_byte..span.end_byte];
            let mut run = Node::new(Role::TextRun);
            run.set_value(text);
            run.set_character_lengths(
                text.chars()
                    .map(|character| character.len_utf8() as u8)
                    .collect::<Vec<_>>(),
            );
            run.set_word_starts(word_starts(text));
            run.set_text_direction(TextDirection::LeftToRight);
            run.set_bounds(Rect {
                x0: element.bounds.x as f64,
                y0: element.bounds.y as f64,
                x1: (element.bounds.x + element.bounds.width) as f64,
                y1: (element.bounds.y + element.bounds.height) as f64,
            });
            (text_run_id(field_id, index), run)
        })
        .collect()
}

/// The run and the character inside it that a character offset into a
/// field's text falls on. An offset right after a line break belongs to the
/// start of the next line.
fn text_position(field_id: i32, value: &str, runs: &[TextRunSpan], chars: usize) -> TextPosition {
    let chars = chars.min(value.chars().count());
    let mut run_index = runs
        .iter()
        .rposition(|run| run.start_char <= chars)
        .unwrap_or(0);
    if run_index + 1 < runs.len()
        && runs[run_index + 1].start_char == chars
        && value[..runs[run_index].end_byte].ends_with('\n')
    {
        run_index += 1;
    }
    TextPosition {
        node: text_run_id(field_id, run_index.min(0xfe)),
        character_index: chars - runs[run_index].start_char,
    }
}

/// Puts the caret, or the picked stretch of text, on the accesskit node of an
/// editable field, in the characters of its text runs.
fn apply_text_selection(node: &mut Node, field_id: i32, element: &AccessibilityElement) {
    let (Some(value), Some((anchor, focus))) = (&element.value, element.text_selection) else {
        return;
    };
    let runs = text_runs(value);
    node.set_text_selection(TextSelection {
        anchor: text_position(
            field_id,
            value,
            &runs,
            accessibility::char_offset(value, anchor),
        ),
        focus: text_position(
            field_id,
            value,
            &runs,
            accessibility::char_offset(value, focus),
        ),
    });
}

/// The live node of the field a screen reader set a selection on, with the
/// two ends counted in characters of the whole text, or nothing when the
/// selection names a run that is not in the published tree.
fn selection_chars(
    elements: &[AccessibilityElement],
    target: NodeId,
    selection: &TextSelection,
) -> Option<(cranpose_core::NodeId, usize, usize)> {
    let (field_id, _) = text_run_owner(selection.anchor.node)
        .or_else(|| text_run_owner(target))
        .unwrap_or((target.0 as i32, 0));
    let ids = accessibility::element_ids(elements);
    let element = elements.get(ids.iter().position(|id| *id == field_id)?)?;
    let runs = text_runs(element.value.as_deref()?);
    let offset = |position: &TextPosition| -> Option<usize> {
        let (owner, run_index) = text_run_owner(position.node)?;
        (owner == field_id).then_some(())?;
        Some(runs.get(run_index)?.start_char + position.character_index)
    };
    Some((
        element.node_id,
        offset(&selection.anchor)?,
        offset(&selection.focus)?,
    ))
}

#[cfg(test)]
#[path = "tests/desktop_accessibility.rs"]
mod tests;
