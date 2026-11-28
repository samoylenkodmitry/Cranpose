// The rotary input of a surface: a watch's crown or bezel, which reaches the focused node's chain
// or the chain under the cursor, and then the window's own handler.

use std::{fmt::Debug, rc::Rc};

use cranpose_core::{NodeId, enter_event_handler_scope, run_in_mutable_snapshot};
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene, Renderer};
use cranpose_ui::RotaryScrollEvent;
use cranpose_ui_graphics::Point;

use super::Stop;
use crate::{SurfaceMut, hit_path_tracker};

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R> SurfaceMut<'_, R>
where
    R: Renderer,
    R::Error: Debug,
{
    /// Installs the window-level rotary (Wear OS crown / rotating bezel)
    /// handler — the low-level escape hatch.
    ///
    /// The handler runs only after the routed modifier chain has declined the
    /// event (see [`rotary_scrolled`](Self::rotary_scrolled)), so an app that
    /// draws everything into a single canvas receives every rotary delta
    /// without registering a focus target or a modifier. Returning `true`
    /// reports the event as consumed to the platform.
    ///
    /// Passing a new handler replaces the previous one.
    pub fn set_on_rotary_scroll<F>(&mut self, handler: F)
    where
        F: Fn(RotaryScrollEvent) -> bool + 'static,
    {
        self.surface_mut().on_rotary_scroll = Some(Rc::new(handler));
    }

    /// Removes the window-level rotary handler, if one is installed.
    pub fn clear_on_rotary_scroll(&mut self) {
        self.surface_mut().on_rotary_scroll = None;
    }

    /// Dispatches a rotary scroll expressed in raw detents (Android
    /// `AXIS_SCROLL`), converting to pixels with the configured scroll factor.
    ///
    /// Applies Compose's sign convention: a positive detent value (crown turned
    /// up/away) produces a negative `vertical_scroll_pixels`.
    pub fn rotary_scrolled_by_detents(&mut self, detents: f32, uptime_millis: u64) -> bool {
        let factor = self.shell.app.rotary_scroll_factor;
        self.rotary_scrolled(RotaryScrollEvent::from_detents(
            detents,
            factor,
            factor,
            uptime_millis,
        ))
    }

    /// Dispatches a rotary scroll event (Wear OS crown, Galaxy Watch bezel, or
    /// a desktop mouse wheel standing in for one during development).
    ///
    /// Routing mirrors Compose's `RotaryInputModifierNode` contract:
    ///
    /// 1. Resolve the target chain. When a focus target is registered
    ///    (`cranpose_ui::focus_dispatch::active_focus_target`) and still
    ///    exists in the current scene, rotary uses the focus target's capture
    ///    path. Otherwise, rotary uses the hit-test chain under the cursor.
    /// 2. **Capture pass**, root to leaf, invoking `on_pre_rotary_scroll_event`
    ///    handlers.
    /// 3. **Bubble pass**, leaf to root, invoking `on_rotary_scroll_event`
    ///    handlers.
    /// 4. If still unconsumed, the window-level handler installed by
    ///    [`set_on_rotary_scroll`](Self::set_on_rotary_scroll).
    ///
    /// The first handler returning `true` consumes the event and stops every
    /// remaining step. Returns `true` when the event was consumed.
    pub fn rotary_scrolled(&mut self, event: RotaryScrollEvent) -> bool {
        let _event_handler = enter_event_handler_scope();
        let result = {
            let _app_context = self.shell.app.app_context.enter_scope();
            run_in_mutable_snapshot(|| self.rotary_scrolled_inner(event)).unwrap_or(false)
        };
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "rotary_scrolled v={:.2} h={:.2} uptime={} -> {result}",
            event.vertical_scroll_pixels,
            event.horizontal_scroll_pixels,
            event.uptime_millis,
        );
        result
    }

    fn rotary_scrolled_inner(&mut self, rotary: RotaryScrollEvent) -> bool {
        if rotary.is_empty() {
            return false;
        }

        let bubble_order = self.rotary_dispatch_order();
        let position = Point {
            x: self.surface().cursor.0,
            y: self.surface().cursor.1,
        };

        if !bubble_order.is_empty() {
            let capture_targets = bubble_order
                .iter()
                .rev()
                .filter_map(|&node_id| self.surface().renderer.scene().find_target(node_id))
                .collect::<Vec<_>>();
            let mut capture_event =
                PointerEvent::rotary(PointerEventKind::RotaryScrollPre, rotary, position);
            capture_event.modifiers = self.shell.app.modifiers;
            self.dispatch_targets(capture_targets, capture_event.clone(), Stop::AtConsumer);
            if capture_event.is_consumed() {
                return true;
            }

            let bubble_targets = bubble_order
                .iter()
                .filter_map(|&node_id| self.surface().renderer.scene().find_target(node_id))
                .collect::<Vec<_>>();
            let mut bubble_event =
                PointerEvent::rotary(PointerEventKind::RotaryScroll, rotary, position);
            bubble_event.modifiers = self.shell.app.modifiers;
            self.dispatch_targets(bubble_targets, bubble_event.clone(), Stop::AtConsumer);
            if bubble_event.is_consumed() {
                return true;
            }
        }

        if let Some(handler) = self.surface().on_rotary_scroll.clone() {
            return handler(rotary);
        }

        false
    }

    fn rotary_dispatch_order(&self) -> Vec<NodeId> {
        if let Some(focused) = cranpose_ui::active_focus_target()
            && let Some(target) = self.surface().renderer.scene().find_target(focused)
        {
            let path = target.capture_path();
            if !path.is_empty() {
                return hit_path_tracker::dispatch_order_for_paths(&[path]);
            }
        }

        let hits = self
            .surface()
            .renderer
            .scene()
            .hit_test(self.surface().cursor.0, self.surface().cursor.1);
        if hits.is_empty() {
            return Vec::new();
        }
        let capture_paths = hits
            .iter()
            .map(HitTestTarget::capture_path)
            .collect::<Vec<_>>();
        hit_path_tracker::dispatch_order_for_paths(&capture_paths)
    }
}
