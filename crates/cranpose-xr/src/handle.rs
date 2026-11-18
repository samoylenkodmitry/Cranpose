//! The spheres at a panel's edges that a controller grabs: one under the bottom edge carries the
//! panel, and one at the bottom right corner resizes it.

use crate::{
    Ray,
    carry::handle_center,
    resize::corner_center,
    space::{Mobility, PanelSlot},
    vector::{dot, sub},
};

// The space between a panel's edge and its handle, in meters.
pub(crate) const HANDLE_GAP: f32 = 0.02;

// The radius of a handle, which is a sphere, in meters.
pub(crate) const HANDLE_RADIUS: f32 = 0.02;

// How far from the handle a ray still grabs it, in meters. The margin stays under the gap, so
// the grab does not reach the panel's edge. A press on that edge still presses.
const HANDLE_MARGIN: f32 = 0.015;

// What a handle does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandleKind {
    // The handle under the bottom edge of a movable panel carries it.
    Move,
    // The handle at the bottom right corner of a resizable panel resizes it.
    Resize,
}

impl HandleKind {
    pub(crate) const ALL: [Self; 2] = [Self::Move, Self::Resize];

    // Whether `slot`'s panel has a handle of this kind.
    pub(crate) fn offered_by(self, slot: &PanelSlot) -> bool {
        match self {
            Self::Move => slot.mobility == Mobility::Movable,
            Self::Resize => slot.resize_limits().is_some(),
        }
    }

    // The center of the handle of this kind at `slot`'s panel.
    pub(crate) fn center(self, slot: &PanelSlot) -> [f32; 3] {
        match self {
            Self::Move => handle_center(slot),
            Self::Resize => corner_center(slot),
        }
    }

    // The distance along `ray` to the handle of this kind at `slot`'s panel, if the ray comes
    // within the margin of it. The ray meets the sphere from the panel's front, from its back or
    // from any other side. A ray that starts inside the margin meets it at distance 0.
    pub(crate) fn hit(self, ray: &Ray, slot: &PanelSlot) -> Option<f32> {
        let reach = HANDLE_RADIUS + HANDLE_MARGIN;
        let from_center = sub(ray.origin, self.center(slot));
        let along = dot(from_center, ray.direction);
        let discriminant = along * along - (dot(from_center, from_center) - reach * reach);
        if discriminant < 0.0 {
            return None;
        }
        let root = discriminant.sqrt();
        (root >= along).then(|| (-along - root).max(0.0))
    }
}
