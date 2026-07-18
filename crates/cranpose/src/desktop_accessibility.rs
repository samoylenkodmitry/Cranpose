//! Native desktop accessibility through AccessKit platform adapters.
#![allow(unsafe_code)]

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};
use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId,
    Rect, Role, Tree, TreeId, TreeUpdate,
};
use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use winit::event::WindowEvent;
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

const ROOT_ID: NodeId = NodeId(u64::MAX);

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
    previous: Vec<AccessibilityElement>,
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
            previous: Vec::new(),
        }
    }

    pub(crate) fn process_event(&mut self, window: &dyn Window, event: &WindowEvent) {
        self.adapter.process_event(window, event);
    }

    pub(crate) fn sync(&mut self, shell: &mut AppShell<WgpuRenderer>) {
        let elements = accessibility::snapshot(shell);
        if elements == self.previous {
            return;
        }
        self.previous.clone_from(&elements);
        let update = tree_update(&elements);
        self.centers = elements
            .iter()
            .map(|element| (NodeId(element.node_id as u64), element.bounds.center()))
            .collect();
        *self
            .initial_tree
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(update.clone());
        self.adapter.update_if_active(|| update);
    }

    pub(crate) fn drain_clicks(&mut self) -> Vec<(f32, f32)> {
        std::mem::take(
            &mut *self
                .actions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
        .into_iter()
        .filter(|request| request.action == Action::Click)
        .filter_map(|request| self.centers.get(&request.target_node).copied())
        .collect()
    }
}

fn tree_update(elements: &[AccessibilityElement]) -> TreeUpdate {
    let children: Vec<NodeId> = elements
        .iter()
        .map(|element| NodeId(element.node_id as u64))
        .collect();
    let mut root = Node::new(Role::Window);
    root.set_label("Cranpose application");
    root.set_children(children);
    let mut nodes = vec![(ROOT_ID, root)];
    nodes.extend(elements.iter().map(|element| {
        let role = match element.role {
            AccessibilityRole::Button => Role::Button,
            AccessibilityRole::StaticText => Role::Label,
            AccessibilityRole::TextField => Role::TextInput,
        };
        let mut node = Node::new(role);
        if element.role == AccessibilityRole::StaticText {
            node.set_value(element.label.as_str());
        } else {
            node.set_label(element.label.as_str());
        }
        if let Some(value) = &element.value {
            node.set_value(value.as_str());
        }
        node.set_bounds(Rect {
            x0: element.bounds.x as f64,
            y0: element.bounds.y as f64,
            x1: (element.bounds.x + element.bounds.width) as f64,
            y1: (element.bounds.y + element.bounds.height) as f64,
        });
        if element.clickable {
            node.add_action(Action::Click);
        }
        (NodeId(element.node_id as u64), node)
    }));
    let mut tree = Tree::new(ROOT_ID);
    tree.toolkit_name = Some("Cranpose".into());
    TreeUpdate {
        nodes,
        tree: Some(tree),
        tree_id: TreeId::ROOT,
        focus: ROOT_ID,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::AccessibilityRect;

    #[test]
    fn desktop_tree_maps_controls_to_native_roles_and_click_actions() {
        let elements = vec![
            AccessibilityElement {
                node_id: 7,
                label: "Items".into(),
                value: None,
                bounds: AccessibilityRect::new(10.0, 20.0, 80.0, 44.0),
                role: AccessibilityRole::Button,
                clickable: true,
            },
            AccessibilityElement {
                node_id: 8,
                label: "Receipts".into(),
                value: None,
                bounds: AccessibilityRect::new(10.0, 70.0, 120.0, 24.0),
                role: AccessibilityRole::StaticText,
                clickable: false,
            },
        ];

        let update = tree_update(&elements);
        assert_eq!(update.tree.as_ref().map(|tree| tree.root), Some(ROOT_ID));
        let button = &update.nodes[1].1;
        assert_eq!(button.role(), Role::Button);
        assert_eq!(button.label(), Some("Items"));
        assert!(button.supports_action(Action::Click));
        let label = &update.nodes[2].1;
        assert_eq!(label.role(), Role::Label);
        assert_eq!(label.value(), Some("Receipts"));
    }
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
        if let WindowEvent::Focused(focused) = event {
            if let Some(events) = self.0.update_view_focus_state(*focused) {
                events.raise();
            }
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
                // Wayland intentionally does not expose absolute window
                // positions. AT-SPI only needs this update on X11, where the
                // outer desktop position is available.
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
