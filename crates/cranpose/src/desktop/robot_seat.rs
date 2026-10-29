use std::time::Instant;

use cranpose_app_shell::FrameUpdateResult;
use winit::event_loop::ControlFlow;

#[cfg(feature = "robot")]
use super::robot_driver::{RobotController, bound_robot_park};
use crate::{app_launcher::AppSettings, wgpu_surface::PresentStep};

// What the event loop keeps for a robot test driver: its controller and app
// hook, the frames presented and the ones that could not be since the last
// present, and whether the visible surface still waits for a frame. A build
// without the robot feature keeps none of it, and each query answers as no
// robot driving the loop would.
#[derive(Default)]
pub(super) struct RobotSeat {
    #[cfg(feature = "robot")]
    pub(super) controller: Option<RobotController>,
    #[cfg(feature = "robot")]
    pub(super) app_hook: Option<Box<crate::RobotAppHook>>,
    #[cfg(feature = "robot")]
    pub(super) presented_frame_generation: u64,
    #[cfg(feature = "robot")]
    pub(super) unpresentable_frames: u32,
    #[cfg(feature = "robot")]
    pub(super) visible_surface_dirty: bool,
}

impl RobotSeat {
    #[cfg_attr(
        not(feature = "robot"),
        expect(unused_variables, reason = "only a robot build keeps the app hook")
    )]
    pub(super) fn new(settings: &mut AppSettings) -> Self {
        #[cfg(feature = "robot")]
        return Self {
            app_hook: settings.robot_app_hook.take(),
            ..Self::default()
        };
        #[cfg(not(feature = "robot"))]
        Self::default()
    }

    pub(super) fn drives(&self) -> bool {
        #[cfg(feature = "robot")]
        return self.controller.is_some();
        #[cfg(not(feature = "robot"))]
        false
    }

    // Whether the robot holds the primary button down in a gesture it plays.
    pub(super) fn synthetic_primary_down(&self) -> bool {
        #[cfg(feature = "robot")]
        return self
            .controller
            .as_ref()
            .is_some_and(RobotController::synthetic_primary_down);
        #[cfg(not(feature = "robot"))]
        false
    }

    #[cfg_attr(
        not(feature = "robot"),
        expect(unused_variables, reason = "only a robot reads the update result")
    )]
    pub(super) fn record_idle_update_result(&mut self, result: FrameUpdateResult) {
        #[cfg(feature = "robot")]
        if let Some(controller) = &mut self.controller {
            controller.record_idle_update_result(result);
        }
    }

    // Takes the robot's next command into its turn; true when one waits.
    pub(super) fn stage_pending_command(&mut self) -> bool {
        #[cfg(feature = "robot")]
        return self
            .controller
            .as_mut()
            .is_some_and(RobotController::stage_pending_command);
        #[cfg(not(feature = "robot"))]
        false
    }

    pub(super) fn visible_surface_dirty(&self) -> bool {
        #[cfg(feature = "robot")]
        return self.visible_surface_dirty;
        #[cfg(not(feature = "robot"))]
        false
    }

    // Notes a frame the surface had no texture for.
    pub(super) fn note_unpresentable(&mut self) {
        #[cfg(feature = "robot")]
        {
            self.unpresentable_frames = self.unpresentable_frames.saturating_add(1);
        }
    }

    // Notes a presented frame: the visible surface holds what was drawn.
    pub(super) fn note_presented(&mut self) {
        #[cfg(feature = "robot")]
        {
            self.presented_frame_generation = self.presented_frame_generation.saturating_add(1);
            self.unpresentable_frames = 0;
            self.visible_surface_dirty = false;
        }
    }

    // Notes a frame the loop took no image for. One the surface already
    // showed holds what a present would have drawn, so it counts as one: a
    // robot waiting for the next present would otherwise wait for a frame
    // that never comes. Otherwise nothing asked for a frame.
    #[cfg_attr(
        not(feature = "robot"),
        expect(unused_variables, reason = "only a robot tracks the visible surface")
    )]
    pub(super) fn note_skipped_present(&mut self, step: PresentStep) {
        #[cfg(feature = "robot")]
        if step == PresentStep::Shown {
            self.note_presented();
        } else {
            self.visible_surface_dirty = false;
        }
    }

    // Whether the robot waits for progress the loop must poll for.
    pub(super) fn awaiting_progress(&mut self) -> bool {
        #[cfg(feature = "robot")]
        return self
            .controller
            .as_mut()
            .is_some_and(RobotController::awaiting_progress);
        #[cfg(not(feature = "robot"))]
        false
    }

    // The loop's park, bounded while a robot drives it.
    #[cfg_attr(
        not(feature = "robot"),
        expect(unused_variables, reason = "only a robot bounds the park")
    )]
    pub(super) fn bound_park(&self, control_flow: ControlFlow, now: Instant) -> ControlFlow {
        #[cfg(feature = "robot")]
        if self.controller.is_some() {
            return bound_robot_park(control_flow, now);
        }
        control_flow
    }
}
