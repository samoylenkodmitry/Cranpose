use std::{fmt::Debug, rc::Rc};

use cranpose_core::{NodeId, enter_event_handler_scope, run_in_mutable_snapshot};
use cranpose_foundation::PointerEventKind;
use cranpose_render_common::Renderer;
use cranpose_ui::{Key, KeyEvent, KeyEventType, LayoutBox};
use cranpose_ui_graphics::{Point, Rect};

use crate::{PointerEventTime, SurfaceMut};

// A press a click key holds on a control until the key comes up.
pub(crate) struct KeyPress {
    key: Key,
    target: PressTarget,
}

enum PressTarget {
    Pointer(Press),
    // A control that clicks through a semantics action or by taking focus
    // for editing, which happens on the release.
    Action(Action),
}

enum Action {
    Direct(cranpose_foundation::SemanticsCustomAction),
    Edit(NodeId),
}

// Where a pointer press lands: the position on screen and in the control,
// and the control's pointer handlers.
struct Press {
    position: Point,
    local: Point,
    handlers: Rc<cranpose_ui::ModifierNodeSlices>,
}

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
        match self.press_target(node_id, canvas_key) {
            Some(PressTarget::Pointer(press)) => {
                for kind in [PointerEventKind::Down, PointerEventKind::Up] {
                    self.send_press(&press, kind, event_time);
                }
                true
            }
            Some(PressTarget::Action(target)) => self.run_action(target),
            None => false,
        }
    }

    // Presses the control `node_id` as `key` goes down, as Compose's
    // clickable does: a control a pointer presses takes the press now and
    // the release, which clicks it, when the key comes up, so it shows its
    // pressed look and a held key long-presses it. A control that clicks
    // through a semantics action clicks on the release.
    pub(super) fn press_by_key(&mut self, node_id: NodeId, key: Key) -> bool {
        let event_time = self.shell.app.realtime_pointer_event_time(None);
        let _event_handler = enter_event_handler_scope();
        let _app_context = self.shell.app.app_context.enter_scope();
        run_in_mutable_snapshot(|| {
            self.end_key_press(PointerEventKind::Cancel, event_time);
            let Some(target) = self.press_target(node_id, None) else {
                return false;
            };
            if let PressTarget::Pointer(press) = &target {
                self.send_press(press, PointerEventKind::Down, event_time);
            }
            self.surface_mut().key_press = Some(KeyPress { key, target });
            self.mark_dirty();
            true
        })
        .unwrap_or(false)
    }

    // Releases the press `key` made, which clicks the control. Answers
    // whether `key` held one.
    pub(super) fn release_by_key(&mut self, key: Key) -> bool {
        if self
            .surface()
            .key_press
            .as_ref()
            .is_none_or(|press| press.key != key)
        {
            return false;
        }
        let event_time = self.shell.app.realtime_pointer_event_time(None);
        let _event_handler = enter_event_handler_scope();
        let _app_context = self.shell.app.app_context.enter_scope();
        run_in_mutable_snapshot(|| self.end_key_press(PointerEventKind::Up, event_time))
            .unwrap_or(false)
    }

    fn end_key_press(&mut self, kind: PointerEventKind, event_time: PointerEventTime) -> bool {
        let Some(press) = self.surface_mut().key_press.take() else {
            return false;
        };
        self.mark_dirty();
        match press.target {
            PressTarget::Pointer(press) => {
                self.send_press(&press, kind, event_time);
                true
            }
            PressTarget::Action(target) => kind == PointerEventKind::Up && self.run_action(target),
        }
    }

    fn press_target(&mut self, node_id: NodeId, canvas_key: Option<u64>) -> Option<PressTarget> {
        let target = {
            let (app, surface) = self.parts();
            surface
                .semantics_tree_for_input(app)
                .and_then(|tree| activation_target(tree.root(), node_id, canvas_key))?
        };
        let (target_id, canvas_bounds) = match target {
            ActivationTarget::Pointer(node_id, bounds) => (node_id, bounds),
            ActivationTarget::Direct(action) => {
                return Some(PressTarget::Action(Action::Direct(action)));
            }
            ActivationTarget::Edit(node_id) => {
                return Some(PressTarget::Action(Action::Edit(node_id)));
            }
        };
        let (position, local, handlers) =
            self.with_layout_tree(|tree| activation_input(tree?.root(), target_id, canvas_bounds))?;
        Some(PressTarget::Pointer(Press {
            position,
            local,
            handlers,
        }))
    }

    fn send_press(&self, press: &Press, kind: PointerEventKind, event_time: PointerEventTime) {
        let event = self.pointer_event(kind, press.local, press.position, event_time);
        press.handlers.dispatch_pointer_event(event.clone());
        event.finish_post_dispatch();
    }

    fn run_action(&mut self, action: Action) -> bool {
        match action {
            Action::Direct(action) => {
                action.invoke();
                true
            }
            Action::Edit(node_id) => {
                self.activate();
                cranpose_ui::request_focus_from_platform(node_id)
            }
        }
    }
}
