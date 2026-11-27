//! The pointers of the two hands over the panels, from the rays and the buttons of two controllers
//! or of two tracked hands.

use std::time::Duration;

use crate::{
    Hand as Side,
    aim::Aim,
    gestures::Gestures,
    handle::HandleFocus,
    pointer::{Frame, Pointer},
    pointer_settings::PointerChoice,
    ray::Beam,
    reach::Scene,
    space::{Cursor, PanelMove, PanelResize, PanelSlot},
    surface_input::{HandGesture, SurfaceGesture},
    volume_pick::PickMeshes,
};

// The pointers of the two hands. Each hand has a pointer of its own, so each hovers, presses,
// captures and drags with its own state, and each reaches the composition under its own pointer id.
// What the hands share is the panels. A panel that one hand holds by a handle has no handle for the
// other hand.
#[derive(Debug)]
pub(crate) struct Pointers {
    sides: [Pointer; 2],
    gestures: Gestures,
    // The time of the frame before
    time: Option<Duration>,
    // Whether the app lets the beams show
    beams_visible: bool,
}

impl Default for Pointers {
    fn default() -> Self {
        Self {
            sides: Side::BOTH.map(Pointer::new),
            gestures: Gestures::default(),
            time: None,
            beams_visible: true,
        }
    }
}

impl Pointers {
    // Reads a frame at `time` with `aims` over `panels`, left hand then right. A volume whose meshes
    // `picks` holds takes the ray where it meets one of them. `choice` says which pointer button
    // each button of a controller presses, whether the app lets the beams show and whether a hand
    // touches the panels. The gestures of the frame are in the accessors.
    pub(crate) fn update(
        &mut self,
        aims: [Option<Aim>; 2],
        time: Duration,
        (panels, picks): (&[PanelSlot], &PickMeshes),
        choice: PointerChoice,
    ) {
        self.gestures.clear();
        self.beams_visible = choice.beams_visible;
        let dt = self
            .time
            .map_or(0.0, |last| time.saturating_sub(last).as_secs_f32());
        self.time = Some(time);
        for side in Side::BOTH {
            // The hand that is not this one: the panel that it holds has no handle here
            let claimed = self.sides[1 - side.index()].held_panel();
            let frame = Frame {
                time,
                dt,
                scene: Scene {
                    panels,
                    picks,
                    claimed,
                },
                mapping: choice.mapping,
                direct_touch: choice.direct_touch,
            };
            self.sides[side.index()].update(aims[side.index()], &frame, &mut self.gestures);
        }
    }

    // The gestures for the app's composition in this frame, in the order that they happened.
    pub(crate) fn atlas(&self) -> &[HandGesture] {
        &self.gestures.atlas
    }

    // The gestures on surfaces and volumes in this frame.
    pub(crate) fn surface_events(&self) -> &[SurfaceGesture] {
        &self.gestures.surfaces
    }

    // The steps of the users' carries in this frame: the grabs, the new pose of each frame in which
    // a panel moved, and the releases.
    pub(crate) fn moves(&self) -> &[PanelMove] {
        &self.gestures.moves
    }

    // The steps of the users' resizes in this frame: the grabs, the new size of each frame in
    // which one changed, and the releases.
    pub(crate) fn resizes(&self) -> &[PanelResize] {
        &self.gestures.resizes
    }

    // The cursor to show on a panel with content for each hand, left then right.
    pub(crate) fn cursors(&self) -> [Option<Cursor>; 2] {
        [0, 1].map(|hand| self.sides[hand].cursor())
    }

    // The surface or volume that each hand's press is on, left then right. It takes that hand's
    // gestures until the release.
    pub(crate) fn pressed_surfaces(&self) -> [Option<u64>; 2] {
        [0, 1].map(|hand| self.sides[hand].pressed_surface())
    }

    // The handle that each hand presses or would grab, left then right.
    pub(crate) fn handles(&self) -> [Option<HandleFocus>; 2] {
        [0, 1].map(|hand| self.sides[hand].handle())
    }

    // The hands whose press took or ended in this frame, left then right. Each gets a light
    // vibration.
    pub(crate) fn ticks(&self) -> [bool; 2] {
        self.gestures.ticks
    }

    // The rays to show, left then right. A ray shows only while the app lets beams show and a panel
    // is in reach of that hand's ray.
    pub(crate) fn beams(&self) -> [Option<Beam>; 2] {
        [0, 1].map(|hand| {
            self.beams_visible
                .then(|| self.sides[hand].beam())
                .flatten()
        })
    }
}

#[cfg(test)]
#[path = "tests/pointer_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/two_pointer_tests.rs"]
mod two_pointer_tests;

#[cfg(test)]
#[path = "tests/press_slop_tests.rs"]
mod press_slop_tests;

#[cfg(test)]
#[path = "tests/direct_touch_tests.rs"]
pub(crate) mod direct_touch_tests;
