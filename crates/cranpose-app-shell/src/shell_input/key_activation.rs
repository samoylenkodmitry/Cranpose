use std::{fmt::Debug, rc::Rc};

use cranpose_core::{NodeId, enter_event_handler_scope, run_in_mutable_snapshot};
use cranpose_foundation::PointerEventKind;
use cranpose_render_common::Renderer;
use cranpose_ui::{KeyEvent, KeyEventType, LayoutBox};
use cranpose_ui_graphics::{Point, Rect};

use crate::{PointerEventTime, SurfaceMut};

pub(super) enum ActivationTarget {
    Direct(cranpose_foundation::SemanticsCustomAction),
    Edit(NodeId),
    Pointer(NodeId, Option<Rect>),
}

pub(super) fn activation_target(
    node: &cranpose_ui::SemanticsNode,
    node_id: NodeId,
    canvas_key: Option<u64>,
) -> Option<ActivationTarget> {
    if node.hidden {
        return None;
    }
    if node.node_id != node_id {
        return node
            .children
            .iter()
            .find_map(|child| activation_target(child, node_id, canvas_key));
    }
    if !node.enabled {
        return None;
    }
    if let Some(key) = canvas_key {
        let child = node
            .details()
            .canvas_children
            .iter()
            .find(|child| child.key == key)?;
        return (child.enabled && child.clickable)
            .then_some(ActivationTarget::Pointer(node_id, Some(child.bounds)));
    }
    if let Some(action) = &node.on_click {
        return Some(ActivationTarget::Direct(action.clone()));
    }
    if node.details().editable_text && node.focusable {
        return Some(ActivationTarget::Edit(node_id));
    }
    node.actions.first().map(|action| match action {
        cranpose_ui::SemanticsAction::Click { handler } => {
            ActivationTarget::Pointer(handler.node_id(), None)
        }
    })
}

pub(super) fn activation_input(
    root: &LayoutBox,
    node_id: NodeId,
    canvas_bounds: Option<Rect>,
) -> Option<(Point, Point, Rc<cranpose_ui::ModifierNodeSlices>)> {
    if root.node_id != node_id {
        return root
            .children
            .iter()
            .find_map(|child| activation_input(child, node_id, canvas_bounds));
    }
    let bounds = canvas_bounds.map_or(root.rect, |bounds| Rect {
        x: root.rect.x + bounds.x,
        y: root.rect.y + bounds.y,
        ..bounds
    });
    let position = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    let local = Point {
        x: position.x - root.rect.x,
        y: position.y - root.rect.y,
    };
    (bounds.width > 0.0 && bounds.height > 0.0 && position.x.is_finite() && position.y.is_finite())
        .then(|| (position, local, Rc::clone(&root.node_data.modifier_slices)))
}

pub(super) fn plain_key_down(event: &KeyEvent) -> bool {
    event.type_() == KeyEventType::KeyDown
        && !event.modifiers().ctrl
        && !event.modifiers().meta
        && !event.modifiers().alt
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R> SurfaceMut<'_, R>
where
    R: Renderer,
    R::Error: Debug,
{
    /// Activates the identified control using its current semantics and geometry.
    ///
    /// Hidden, disabled, removed and modal-background controls reject activation.
    /// `canvas_key` identifies a drawn child of `node_id`. The action bypasses
    /// overlapping controls and developer overlays without changing pointer capture.
    pub fn accessibility_activate(&mut self, node_id: NodeId, canvas_key: Option<u64>) -> bool {
        let event_time = self.shell.app.realtime_pointer_event_time(None);
        let _event_handler = enter_event_handler_scope();
        let result = {
            let _app_context = self.shell.app.app_context.enter_scope();
            run_in_mutable_snapshot(|| self.activate_node(node_id, canvas_key, event_time))
                .unwrap_or(false)
        };
        if result {
            self.mark_dirty();
        }
        result
    }

    fn activate_node(
        &mut self,
        node_id: NodeId,
        canvas_key: Option<u64>,
        event_time: PointerEventTime,
    ) -> bool {
        let target = {
            let (app, surface) = self.parts();
            surface
                .semantics_tree_for_input(app)
                .and_then(|tree| activation_target(tree.root(), node_id, canvas_key))
        };
        let (target_id, canvas_bounds) = match target {
            Some(ActivationTarget::Direct(action)) => {
                action.invoke();
                return true;
            }
            Some(ActivationTarget::Edit(node_id)) => {
                self.activate();
                return cranpose_ui::request_focus_from_platform(node_id);
            }
            Some(ActivationTarget::Pointer(node_id, bounds)) => (node_id, bounds),
            None => return false,
        };
        let position =
            self.with_layout_tree(|tree| activation_input(tree?.root(), target_id, canvas_bounds));
        let Some((position, local, handlers)) = position else {
            return false;
        };
        for kind in [PointerEventKind::Down, PointerEventKind::Up] {
            let event = self.pointer_event(kind, local, position, event_time);
            handlers.dispatch_pointer_event(event.clone());
            event.finish_post_dispatch();
        }
        true
    }
}
