use std::fmt::Debug;

use cranpose_core::{enter_event_handler_scope, run_in_mutable_snapshot};
use cranpose_foundation::{PointerButton, PointerEventKind, PointerId};
use cranpose_render_common::{HitTestTarget, RenderScene, Renderer};

use crate::{AppShell, PointerEventTime, SurfaceMut};

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R> SurfaceMut<'_, R>
where
    R: Renderer,
    R::Error: Debug,
{
    /// Presses the primary pointer button at the position set by `set_cursor`. Returns `true` when
    /// a control or overlay handled the press.
    pub fn pointer_pressed(&mut self) -> bool {
        self.pointer_pressed_at_time(None)
    }

    /// Like [`pointer_pressed`](Self::pointer_pressed), but carries the
    /// platform input timestamp (milliseconds) of the press sample.
    pub fn pointer_pressed_at_time(&mut self, time_ms: Option<i64>) -> bool {
        let event_time = self.shell.app.realtime_pointer_event_time(time_ms);
        self.pointer_pressed_at_event_time(event_time)
    }

    /// Dispatch primary-button down with an already resolved event timestamp.
    pub fn pointer_pressed_at_event_time(&mut self, event_time: PointerEventTime) -> bool {
        let (cursor_x, cursor_y) = self.surface().cursor;
        if self.inspector_press(cursor_x, cursor_y) {
            return true;
        }
        if self.dev_overlay_press(cursor_x, cursor_y) {
            return true;
        }
        let _event_handler = enter_event_handler_scope();
        let result = {
            let _app_context = self.shell.app.app_context.enter_scope();
            run_in_mutable_snapshot(|| self.pointer_pressed_inner(event_time)).unwrap_or(false)
        };
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "pointer_pressed time_ms={:?} animation_time_nanos={} -> {result}",
            event_time.platform_time_ms,
            event_time.animation_time_nanos,
        );
        result
    }

    fn pointer_pressed_inner(&mut self, event_time: PointerEventTime) -> bool {
        self.activate();
        self.note_focus_moved_by_keyboard(false);
        self.surface_mut()
            .buttons_pressed
            .insert(PointerButton::Primary);

        let (cursor_x, cursor_y) = self.surface().cursor;
        let mut hits = self.surface().renderer.scene().hit_test(cursor_x, cursor_y);
        if self.surface().pointer_source.is_touch_like()
            && let Some(near) = self
                .surface()
                .renderer
                .scene()
                .hit_test_near(cursor_x, cursor_y)
            && hits.iter().all(|hit| {
                hit.node_id() != near.node_id() && near.capture_path().contains(&hit.node_id())
            })
        {
            hits.insert(0, near);
        }
        let event = self.cursor_event(PointerEventKind::Down, event_time);
        self.press_hits(hits, event, PointerId::PRIMARY)
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    /// Primary-surface form of [`SurfaceMut::pointer_pressed`].
    pub fn pointer_pressed(&mut self) -> bool {
        self.primary().pointer_pressed()
    }

    /// Primary-surface form of [`SurfaceMut::pointer_pressed_at_time`].
    pub fn pointer_pressed_at_time(&mut self, time_ms: Option<i64>) -> bool {
        self.primary().pointer_pressed_at_time(time_ms)
    }

    /// Primary-surface form of [`SurfaceMut::pointer_pressed_at_event_time`].
    pub fn pointer_pressed_at_event_time(&mut self, event_time: PointerEventTime) -> bool {
        self.primary().pointer_pressed_at_event_time(event_time)
    }
}
