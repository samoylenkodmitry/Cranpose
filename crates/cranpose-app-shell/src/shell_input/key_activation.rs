use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_ui::{KeyEvent, KeyEventType, LayoutBox};
use cranpose_ui_graphics::{Point, Rect};

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
