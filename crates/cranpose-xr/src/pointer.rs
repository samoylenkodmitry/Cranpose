//! One pointer over the panels, from the rays and triggers of two controllers.

use crate::{
    Ray,
    space::{Cursor, PanelSlot},
};

// What a controller does in a frame: where it points and whether its trigger is down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Aim {
    pub(crate) ray: Ray,
    pub(crate) select: bool,
}

// What the pointer does to the panels' composition, in the atlas's dp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PointerEvent {
    Move { x: f32, y: f32 },
    Press,
    Release,
    // The pointer left every panel.
    Leave,
}

// A trigger in a frame, compared with the frame before.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Trigger {
    Up,
    // Down now, up in the frame before.
    Pulled,
    Held,
}

impl Trigger {
    fn new(down: bool, was_down: bool) -> Self {
        match (down, was_down) {
            (false, _) => Self::Up,
            (true, false) => Self::Pulled,
            (true, true) => Self::Held,
        }
    }
}

// The pointer. One controller drives it at a time: the one whose trigger went down last, or
// the other one while only that one points at a panel.
#[derive(Debug, Default)]
pub(crate) struct Pointer {
    // The controller that drives the pointer: 0 for the left hand, 1 for the right
    hand: usize,
    // The trigger of each controller in the frame before
    selects: [bool; 2],
    pressed: bool,
    position: Option<(f32, f32)>,
}

impl Pointer {
    // The events of a frame with `aims` over `panels`, and the cursor to show.
    pub(crate) fn update(
        &mut self,
        aims: [Option<Aim>; 2],
        panels: &[PanelSlot],
    ) -> (Vec<PointerEvent>, Option<Cursor>) {
        let hits = aims.map(|aim| aim.and_then(|aim| nearest_hit(&aim.ray, panels)));
        let selects = aims.map(|aim| aim.is_some_and(|aim| aim.select));
        let triggers = [0, 1].map(|hand| Trigger::new(selects[hand], self.selects[hand]));
        self.selects = selects;
        self.choose_hand(&hits, triggers);
        let hit = hits[self.hand];
        let events = self.events(hit, triggers[self.hand]);
        let cursor = hit.map(|(slot, x, y)| Cursor {
            panel: slot.id,
            x,
            y,
            pressed: self.pressed,
        });
        (events, cursor)
    }

    // A trigger that goes down takes the pointer to its controller. While no press is on, a
    // controller that points at a panel takes it from one that points at none. A press keeps
    // its controller until the trigger goes up.
    fn choose_hand(&mut self, hits: &[Option<(PanelSlot, f32, f32)>; 2], triggers: [Trigger; 2]) {
        if self.pressed {
            return;
        }
        let pulled = (0..2).rev().find(|&hand| triggers[hand] == Trigger::Pulled);
        if let Some(hand) = pulled {
            self.hand = hand;
        } else if hits[self.hand].is_none() && hits[1 - self.hand].is_some() {
            self.hand = 1 - self.hand;
        }
    }

    // A press starts in the frame where the trigger goes down over a panel. A trigger that
    // was down before the ray reached a panel does not press.
    fn events(
        &mut self,
        hit: Option<(PanelSlot, f32, f32)>,
        trigger: Trigger,
    ) -> Vec<PointerEvent> {
        let mut events = Vec::new();
        if let Some((slot, x, y)) = hit {
            let position = (slot.atlas[0] + x, slot.atlas[1] + y);
            if self.position != Some(position) {
                events.push(PointerEvent::Move {
                    x: position.0,
                    y: position.1,
                });
                self.position = Some(position);
            }
        }
        match trigger {
            Trigger::Pulled if hit.is_some() => {
                events.push(PointerEvent::Press);
                self.pressed = true;
            }
            Trigger::Up if self.pressed => {
                events.push(PointerEvent::Release);
                self.pressed = false;
            }
            _ => {}
        }
        if hit.is_none() && !self.pressed && self.position.take().is_some() {
            events.push(PointerEvent::Leave);
        }
        events
    }
}

// The panel the ray meets first and the place on it in dp from its top left corner.
fn nearest_hit(ray: &Ray, panels: &[PanelSlot]) -> Option<(PanelSlot, f32, f32)> {
    panels
        .iter()
        .filter_map(|slot| {
            let size = [slot.atlas[2], slot.atlas[3]].map(|dp| dp * crate::METERS_PER_DP);
            ray.hit_panel(&slot.pose, size).map(|hit| (hit, slot))
        })
        .min_by(|a, b| a.0.distance.total_cmp(&b.0.distance))
        .map(|(hit, slot)| (*slot, hit.u * slot.atlas[2], hit.v * slot.atlas[3]))
}

#[cfg(test)]
#[path = "tests/pointer_tests.rs"]
mod tests;
