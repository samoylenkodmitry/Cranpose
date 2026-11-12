// What the shell gives back at rest: the room a burst took in the lists its
// frames fill, such as a first frame's, which names every node, and in its
// context's, its scenes' and its renderers' storage. The shell trims once it
// rested for `REST_BEFORE_TRIM` after its last frame: the platform wakes it
// then for an update that draws nothing.

use std::{cell::Cell, fmt::Debug, time::Duration};

use cranpose_render_common::{RenderScene, Renderer};
use web_time::Instant;

use crate::{AppShell, FrameSchedule};

// The nodes each list keeps room for at rest.
const LIST_ROOM_KEPT: usize = 64;

// How long the shell rests before it trims: a frame within it, such as the
// next frame of a hover, finds the room of the last one.
const REST_BEFORE_TRIM: Duration = Duration::from_millis(500);

// Where the shell stands on its way to a trim at rest.
#[derive(Default)]
pub(crate) struct RestTrim(Cell<Rest>);

#[derive(Clone, Copy, Default)]
enum Rest {
    // A frame or an update is due.
    #[default]
    Busy,
    // The shell rests and trims at this time.
    Due(Instant),
    // The shell trimmed since its last frame.
    Done,
}

impl RestTrim {
    // `schedule`, which asks while the shell rests for a wake at the time of
    // the trim, and once that time passed for an update.
    pub(crate) fn schedule(&self, mut schedule: FrameSchedule) -> FrameSchedule {
        if schedule.needs_frame || schedule.needs_update {
            self.0.set(Rest::Busy);
            return schedule;
        }
        let due = match self.0.get() {
            Rest::Done => return schedule,
            Rest::Due(due) => due,
            Rest::Busy => {
                let due = Instant::now() + REST_BEFORE_TRIM;
                self.0.set(Rest::Due(due));
                due
            }
        };
        if due <= Instant::now() {
            schedule.needs_update = true;
        } else {
            schedule.next_deadline = Some(schedule.next_deadline.map_or(due, |at| at.min(due)));
        }
        schedule
    }

    // Whether the time of the trim passed while the shell rested.
    fn take_due(&self) -> bool {
        let Rest::Due(due) = self.0.get() else {
            return false;
        };
        let passed = due <= Instant::now();
        if passed {
            self.0.set(Rest::Done);
        }
        passed
    }
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    // Trims once the shell rested past the time of the trim.
    #[inline]
    pub(crate) fn trim_at_rest(&mut self) {
        if self.rest.take_due() {
            self.trim();
        }
    }

    // Out of line: few updates end a rest.
    #[cold]
    #[inline(never)]
    fn trim(&mut self) {
        self.app.app_context.trim();
        self.routing_scratch.trim();
        self.pending_dirty_nodes.shrink_to(LIST_ROOM_KEPT);
        self.pending_layer_property_nodes.shrink_to(LIST_ROOM_KEPT);
        let geometry = &mut self.geometry_scene_nodes;
        geometry.reshaped.shrink_to(LIST_ROOM_KEPT);
        geometry.moved.shrink_to(LIST_ROOM_KEPT);
        geometry.unplaced.shrink_to(LIST_ROOM_KEPT);
        self.app.left_nodes.shrink_to(LIST_ROOM_KEPT);
        for surface in &mut self.surfaces {
            for list in [
                &mut surface.scoped_layout_scene_nodes,
                &mut surface.scoped_moved_scene_nodes,
                &mut surface.scoped_draw_nodes,
                &mut surface.scoped_layer_property_nodes,
                &mut surface.structural_scene_nodes,
            ] {
                list.shrink_to(LIST_ROOM_KEPT);
            }
            surface.scene_updates.shrink_to(LIST_ROOM_KEPT);
            surface.renderer.scene_mut().trim();
            surface.renderer.trim();
        }
    }
}
