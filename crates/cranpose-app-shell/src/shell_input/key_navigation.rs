use std::fmt::Debug;

use cranpose_core::{enter_event_handler_scope, run_in_mutable_snapshot};
use cranpose_render_common::Renderer;
use cranpose_ui::{FocusDirection, Key, KeyEvent, KeyEventType, request_render_invalidation};

use super::key_activation::plain_key_down;
use crate::SurfaceMut;

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R> SurfaceMut<'_, R>
where
    R: Renderer,
    R::Error: Debug,
{
    pub(super) fn on_focus_key(&mut self, event: &KeyEvent) -> bool {
        if !plain_key_down(event) || event.key() != Key::Tab {
            return false;
        }
        self.on_group_navigation_key(event)
    }

    fn on_group_navigation_key(&mut self, event: &KeyEvent) -> bool {
        let focused = cranpose_ui::active_focus_target();
        let target = self.with_layout_tree(|tree| {
            cranpose_ui::keyboard_focus_target(tree?, focused, event.key(), event.modifiers().shift)
        });
        let Some(target) = target else {
            return false;
        };
        let moved =
            run_in_mutable_snapshot(|| cranpose_ui::request_focus_from_platform(target.node_id))
                .unwrap_or(false);
        if moved {
            self.mark_dirty();
            self.note_focus_moved_by_keyboard(true);
            if target.activate {
                self.on_activation_key(&KeyEvent::key_down(Key::Spacebar, " "));
            }
        }
        moved
    }

    pub(super) fn on_activation_key(&mut self, event: &KeyEvent) -> bool {
        if !plain_key_down(event)
            || !matches!(event.key(), Key::Enter | Key::Spacebar)
            || cranpose_ui::text_field_focus::has_focused_field()
        {
            return false;
        }
        let Some(focused) = cranpose_ui::active_focus_target() else {
            return false;
        };
        let activated = self.accessibility_activate(focused, None);
        self.note_focus_moved_by_keyboard(true);
        activated
    }

    // Takes a key the focused control and its ancestors left: a navigation
    // key moves focus, and any other goes to the `UnhandledKeyEvents`
    // handlers unless a text field takes typing.
    pub(super) fn on_unconsumed_key(&mut self, event: &KeyEvent) -> bool {
        if self.on_navigation_key(event) {
            return true;
        }
        if cranpose_ui::text_field_focus::has_focused_field() {
            return false;
        }
        let handled = run_in_mutable_snapshot(|| cranpose_ui::dispatch_unhandled_key_event(event))
            .unwrap_or(false);
        if handled {
            self.mark_dirty();
            self.shell.app.request_layout_pass();
        }
        handled
    }

    // Moves focus with an arrow, Home or End key that no handler took:
    // inside a radio group, tab bar or menu by its own rules, else an arrow
    // moves to the nearest target that way, first within the focused node's
    // selectable group and then on the whole screen. A held key moves once a
    // frame.
    fn on_navigation_key(&mut self, event: &KeyEvent) -> bool {
        let direction = match event.key() {
            Key::DirectionLeft => Some(FocusDirection::Left),
            Key::DirectionRight => Some(FocusDirection::Right),
            Key::DirectionUp => Some(FocusDirection::Up),
            Key::DirectionDown => Some(FocusDirection::Down),
            Key::MoveHome | Key::MoveEnd => None,
            _ => return false,
        };
        if !plain_key_down(event) || cranpose_ui::text_field_focus::has_focused_field() {
            return false;
        }
        let frame = Some(self.shell.app.last_frame_time_nanos);
        if event.native_key_event().repeat_count > 0 && self.surface().key_moved_focus_at == frame {
            return true;
        }
        let moved = self.on_group_navigation_key(event)
            || direction.is_some_and(|direction| self.move_focus_by_arrow(direction));
        if moved {
            self.surface_mut().key_moved_focus_at = frame;
        }
        moved
    }

    fn move_focus_by_arrow(&mut self, direction: FocusDirection) -> bool {
        let focused = cranpose_ui::active_focus_target();
        let group_order = self.with_layout_tree(|layout_tree| {
            let layout_tree = layout_tree?;
            let group = cranpose_ui::selectable_group_of(layout_tree, focused?)?;
            Some(cranpose_ui::collect_focus_order_under(layout_tree, group))
        });
        group_order.is_some_and(|order| self.move_focus_in_order(order, direction))
            || self.move_focus_in_context(direction)
    }

    pub(super) fn note_focus_moved_by_keyboard(&mut self, keyboard: bool) {
        if cranpose_ui::set_keyboard_focus_visible(keyboard) {
            request_render_invalidation();
            self.mark_dirty();
        }
    }

    pub(super) fn on_escape_key(&mut self, event: &KeyEvent) -> bool {
        if event.type_() != KeyEventType::KeyDown || event.key() != Key::Escape {
            return false;
        }
        self.dismiss_top_modal_in_context()
    }

    /// Asks the innermost open dialog or popup to close, the way the platform
    /// back gesture does. Answers whether one was open to take the request.
    pub fn dismiss_top_modal(&mut self) -> bool {
        let _event_handler = enter_event_handler_scope();
        let _app_context = self.shell.app.app_context.enter_scope();
        self.dismiss_top_modal_in_context()
    }

    fn dismiss_top_modal_in_context(&mut self) -> bool {
        let closed = run_in_mutable_snapshot(|| {
            if cranpose_ui::modal_depth() > 0 {
                cranpose_ui::dispatch_modal_back()
            } else {
                cranpose_ui::dismiss_top_popup()
            }
        })
        .unwrap_or(false);
        if closed {
            self.mark_dirty();
        }
        closed
    }

    /// Publishes the focus order layout left behind and moves focus one step.
    /// Answers whether focus moved.
    pub fn move_focus_in_context(&mut self, direction: FocusDirection) -> bool {
        let order = self.with_layout_tree(|layout_tree| {
            layout_tree
                .map(cranpose_ui::collect_focus_order)
                .unwrap_or_default()
        });
        self.move_focus_in_order(order, direction)
    }

    fn move_focus_in_order(
        &mut self,
        order: Vec<cranpose_ui::FocusEntry>,
        direction: FocusDirection,
    ) -> bool {
        cranpose_ui::set_focus_order(order);
        let moved = run_in_mutable_snapshot(|| cranpose_ui::FocusManager.move_focus(direction))
            .unwrap_or(false);
        if moved {
            self.mark_dirty();
            self.note_focus_moved_by_keyboard(true);
        }
        moved
    }
}
