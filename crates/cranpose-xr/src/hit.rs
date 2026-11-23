//! Where a ray meets the panels.

use crate::{
    Hand as Side, Ray,
    content_ray::ContentRay,
    space::{Cursor, PanelKind, PanelSlot, Ride},
    surface_input::RayHit,
    volume_pick::{MeshPick, PickMeshes},
};

// Where a hand's pointer events go: the app's composition, which one atlas holds for every panel
// with content, or one surface or volume, which take the events as a callback of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Atlas,
    Surface(u64),
}

// Where a ray meets a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hit {
    pub(crate) slot: PanelSlot,
    // The place on the panel in dp from its top left corner. On a volume with meshes it is the place
    // of the mesh's hit projected onto the box's front face.
    pub(crate) x: f32,
    pub(crate) y: f32,
    // How far along the ray, in meters
    pub(crate) distance: f32,
    // The mesh of a volume that the ray meets
    pub(crate) pick: Option<MeshPick>,
}

impl Hit {
    // Whether the panel has Cranpose content that the pointer can move over and press.
    pub(crate) fn on_content(&self) -> bool {
        self.slot.kind == PanelKind::Content
    }

    // Where the pointer's events about this hit go.
    pub(crate) fn target(&self) -> Target {
        match self.slot.kind {
            PanelKind::Content => Target::Atlas,
            PanelKind::Surface(_) | PanelKind::Volume { .. } => Target::Surface(self.slot.id),
        }
    }

    pub(crate) fn position(&self) -> (f32, f32) {
        (self.slot.atlas[0] + self.x, self.slot.atlas[1] + self.y)
    }

    pub(crate) fn cursor(&self, pressed: bool) -> Cursor {
        Cursor {
            panel: self.slot.id,
            x: self.x,
            y: self.y,
            pressed,
        }
    }
}

// The mesh that a pick meets, as a gesture reports it.
pub(crate) fn ray_hit(pick: MeshPick) -> RayHit {
    RayHit {
        mesh: pick.mesh,
        place: Some((pick.triangle, pick.position)),
    }
}

// Whether the ray of the controller in hand `side` can meet the panel. A panel that stands in no
// place shows nowhere and takes no hits. A panel that rides a controller is out of reach of that
// controller's own ray: the left ray passes through the ball above the left controller. A volume
// under a panel is out of reach of every ray, so the panel in front of it takes the input.
pub(crate) fn reachable(slot: &PanelSlot, side: Side) -> bool {
    if matches!(slot.kind, PanelKind::Volume { under: Some(_), .. }) {
        return false;
    }
    match slot.ride {
        Ride::Free => true,
        Ride::On(rides) => rides != side,
        Ride::Hidden => false,
    }
}

// The panel the ray of `side` meets first and the place on it. A volume whose meshes `picks` holds
// is met where the ray meets one of them, and the volume's box only decides whether to look.
pub(crate) fn nearest_hit(
    ray: &Ray,
    panels: &[PanelSlot],
    side: Side,
    picks: &PickMeshes,
) -> Option<Hit> {
    panels
        .iter()
        .filter(|slot| reachable(slot, side))
        .filter_map(|slot| {
            let size = slot.size();
            let hit = slot.hit(ray)?;
            if matches!(slot.kind, PanelKind::Volume { .. }) && picks.has_meshes(slot.id) {
                let pick = picks.pick(slot.id, &ContentRay::of(ray, &slot.pose), None)?;
                return Some(Hit {
                    slot: *slot,
                    x: pick.position[0] + size[0] / 2.0,
                    y: pick.position[1] + size[1] / 2.0,
                    distance: pick.distance,
                    pick: Some(pick),
                });
            }
            Some(Hit {
                slot: *slot,
                x: hit.u * size[0],
                y: hit.v * size[1],
                distance: hit.distance,
                pick: None,
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}
