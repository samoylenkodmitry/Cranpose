use cranpose_foundation::PointerSource;
use cranpose_render_common::Renderer;
use cranpose_ui::mouse_input::MouseInputTarget;

use crate::{AppShell, PointerEventTime, RootId};

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R: Renderer> AppShell<R>
where
    R::Error: std::fmt::Debug,
{
    pub(crate) fn dispatch_requested_mouse_moves(&mut self, frame_time: u64) {
        let pending = self.app.app_context.pending_mouse_moves();
        if pending > 0 {
            self.dispatch_mouse_moves(pending, frame_time);
        }
    }

    /// Moves the cursor of each surface that `pending` requested mouse moves
    /// name. Out of line, as most frames have none.
    #[inline(never)]
    fn dispatch_mouse_moves(&mut self, pending: usize, frame_time: u64) {
        for _ in 0..pending {
            let Some(request) = self.app.app_context.take_mouse_move() else {
                break;
            };
            let root = match request.target {
                MouseInputTarget::Primary => RootId::Primary,
                MouseInputTarget::Window(id) => RootId::Window(id),
            };
            if let Some(mut surface) = self.surface(root) {
                surface.set_pointer_source(PointerSource::Mouse);
                surface.set_cursor_at_event_time(request.position.x, request.position.y, PointerEventTime {
                    platform_time_ms: Some((frame_time / 1_000_000).min(i64::MAX as u64) as i64),
                    animation_time_nanos: frame_time,
                });
            }
        }
        // The moves came after the frame's UI tasks ran, and what they
        // change schedules its work as UI tasks, such as the redraw of a node
        // whose draw reads the hover; that work belongs to this frame.
        self.app.runtime.runtime_handle().drain_ui();
    }
}
