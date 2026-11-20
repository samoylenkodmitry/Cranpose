//! The panels that stand below the panel of the focused text field.
//!
//! The keyboard that a headset draws is such a panel. Its pose is not a state of its composition:
//! the panel of the field can be carried or follow a controller without composing. So the space
//! derives the pose from the field's panel each time it reads the panels, and the keyboard goes
//! where the panel goes.

use super::{Entry, PanelSlot, Ride, Shape};
use crate::{METERS_PER_DP, Pose, dock::Dock};

// The space between the field's panel and the panel below it, in dp.
const GAP: f32 = 24.0;

// The pose of a panel of `size` dp that stands below the panel of `field_size` dp at `field`, in
// the same plane, with the gap between the two.
pub(crate) fn below(field: &Pose, field_size: [f32; 2], size: [f32; 2]) -> Pose {
    let drop = (field_size[1] + size[1]) / 2.0 + GAP;
    field.then(&Pose::at([0.0, -drop * METERS_PER_DP, 0.0]))
}

#[cfg_attr(
    not(any(test, all(feature = "scripted-hand", headset))),
    expect(dead_code, reason = "a scripted hand aims at the keys")
)]
// Where the point `at` lies in the room: dp from the top left corner of the panel of `size` dp at
// `pose`.
pub(crate) fn point_on(pose: &Pose, size: [f32; 2], at: [f32; 2]) -> [f32; 3] {
    let x = (at[0] - size[0] / 2.0) * METERS_PER_DP;
    let y = (size[1] / 2.0 - at[1]) * METERS_PER_DP;
    pose.then(&Pose::at([x, y, 0.0])).position
}

// Puts the panels that stand below the field's panel under it, in `panels`, which holds the
// slots of `entries` in the same order. They take the field panel's ride, and show nowhere while
// no panel holds a focused field.
pub(super) fn dock(entries: &[Entry], field_panel: Option<u64>, panels: &mut [PanelSlot]) {
    let field = field_panel
        .and_then(|id| panels.iter().find(|slot| slot.id == id))
        .copied();
    for (index, entry) in entries.iter().enumerate() {
        if entry.dock != Dock::BelowField {
            continue;
        }
        let slot = &mut panels[index];
        match field {
            Some(field) => {
                slot.pose = below(&field.pose, field.size(), slot.size());
                slot.ride = field.ride;
                slot.shape = Shape::Flat;
            }
            None => slot.ride = Ride::Hidden,
        }
    }
}
