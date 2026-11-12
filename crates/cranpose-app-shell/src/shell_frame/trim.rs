// What the shell gives back at rest: the room a burst took in the lists its
// frames fill, such as a first frame's, which names every node, and in its
// context's, its scenes' and its renderers' storage. An update that changes
// what the shell shows starts a rest of `REST_BEFORE_TRIM`; the first update
// past its end that changes nothing trims, and draws nothing. The schedule
// asks for a wake at the end of the rest, and the update checks report the
// trim due once it passed.

use std::{cell::Cell, fmt::Debug, time::Duration};

use cranpose_core::collections::growth_slack::ROOM_KEPT_AT_REST;
use cranpose_render_common::{RenderScene, Renderer};
use web_time::Instant;

use crate::{AppShell, FrameSchedule, FrameUpdateResult};

// How long the shell rests before it trims: a frame within it, such as the
// next frame of a hover, finds the room of the last one.
const REST_BEFORE_TRIM: Duration = Duration::from_millis(500);

// When the trim at rest is due: `None` once the shell trimmed.
pub(crate) struct RestTrim(Cell<Option<Instant>>);

impl RestTrim {
    // A rest that starts now, at the first build, a burst of its own.
    pub(crate) fn begun() -> Self {
        Self(Cell::new(Some(Instant::now() + REST_BEFORE_TRIM)))
    }

    // Whether the rest passed its end.
    pub(crate) fn passed(&self) -> bool {
        self.0.get().is_some_and(|due| due <= Instant::now())
    }

    // `schedule`, which asks while a trim waits for a wake at its time, and
    // once that time passed for an update.
    pub(crate) fn schedule(&self, mut schedule: FrameSchedule) -> FrameSchedule {
        let Some(due) = self.0.get() else {
            return schedule;
        };
        if schedule.needs_frame || schedule.needs_update {
            return schedule;
        }
        if due <= Instant::now() {
            schedule.needs_update = true;
        } else {
            schedule.next_deadline = Some(schedule.next_deadline.map_or(due, |at| at.min(due)));
        }
        schedule
    }
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    // Ends the update at `frame_time`, nanoseconds since the shell started,
    // that made `result`: one that changed what the shell shows starts the
    // rest anew, and one that changed nothing trims once the rest passed its
    // end.
    #[inline]
    pub(crate) fn trim_at_rest(&mut self, result: FrameUpdateResult, frame_time: Option<u64>) {
        if result.visual_changed || result.structure_changed {
            let at = frame_time.map_or_else(Instant::now, |nanos| {
                self.app.start_time + Duration::from_nanos(nanos)
            });
            self.rest.0.set(Some(at + REST_BEFORE_TRIM));
        } else if self.rest.passed() {
            self.rest.0.set(None);
            self.trim();
        }
    }

    // Out of line: few updates end a rest.
    #[cold]
    #[inline(never)]
    fn trim(&mut self) {
        self.app.app_context.trim();
        self.routing_scratch.trim();
        self.pending_dirty_nodes.shrink_to(ROOM_KEPT_AT_REST);
        self.pending_layer_property_nodes
            .shrink_to(ROOM_KEPT_AT_REST);
        let geometry = &mut self.geometry_scene_nodes;
        geometry.reshaped.shrink_to(ROOM_KEPT_AT_REST);
        geometry.moved.shrink_to(ROOM_KEPT_AT_REST);
        geometry.unplaced.shrink_to(ROOM_KEPT_AT_REST);
        self.app.left_nodes.shrink_to(ROOM_KEPT_AT_REST);
        for surface in &mut self.surfaces {
            for list in [
                &mut surface.scoped_layout_scene_nodes,
                &mut surface.scoped_moved_scene_nodes,
                &mut surface.scoped_draw_nodes,
                &mut surface.scoped_layer_property_nodes,
                &mut surface.structural_scene_nodes,
            ] {
                list.shrink_to(ROOM_KEPT_AT_REST);
            }
            surface.scene_updates.shrink_to(ROOM_KEPT_AT_REST);
            surface.renderer.scene_mut().trim();
            surface.renderer.trim();
        }
    }
}
