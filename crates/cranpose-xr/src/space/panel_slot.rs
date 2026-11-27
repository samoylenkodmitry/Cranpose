//! A panel as the headset's backend sees it: what it shows, where it stands and how it moves.

use crate::{Hand, METERS_PER_DP, PanelHit, Pose, Ray, ResizeLimits, StereoMode};

// What a panel shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PanelKind {
    // Cranpose content, drawn into the atlas.
    Content,
    // A surface that another producer writes into. It has no part of the atlas, and no
    // composition: a ray stops on it and presses nothing.
    Surface(StereoMode),
    // 3D content that the scene of the volumes draws. The panel's pose is the center of its box,
    // its size is the box's width and height in dp, and `depth` is the box's depth in dp. It has no
    // part of the atlas: a ray stops on the box. A volume `under` a panel of content stands where
    // that panel stands and takes no ray: the panel in front of it takes the input.
    Volume { depth: f32, under: Option<u64> },
}

// A panel as the backend sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelSlot {
    pub(crate) id: u64,
    pub(crate) kind: PanelKind,
    // The panel's part of the atlas: left, top, width and height in dp. A surface or a volume holds
    // its size here and no room in the atlas.
    pub(crate) atlas: [f32; 4],
    // The panel's center and turn in the room.
    pub(crate) pose: Pose,
    // Whether the user may carry the panel with a controller.
    pub(crate) mobility: Mobility,
    // The shape the panel shows on. A surface panel is flat.
    pub(crate) shape: Shape,
    // How strongly the panel holds a layer of the compositor of its own
    pub(crate) priority: i32,
    // The sizes that the user may give the panel, if the content set a resize.
    pub(crate) resize: Option<ResizeLimits>,
    // The width and height in dp of the room that the panel holds in the atlas and in its image:
    // its size, or more while the user resizes it.
    pub(crate) room: [f32; 2],
    // Whether a controller places the panel
    pub(crate) ride: Ride,
    // The space whose newest pose the compositor places the panel's layers with, or `None` for
    // the room's space
    pub(crate) tracked: Option<TrackedSpace>,
}

impl PanelSlot {
    // The panel's width and height in dp.
    pub(crate) fn size(&self) -> [f32; 2] {
        [self.atlas[2], self.atlas[3]]
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The pose of the flat face that the panel's handles stand at and that a resize measures on:
    // the panel's own pose, or for a volume the pose of its front face, which lies half the box's
    // depth toward the viewer. A handle at the center plane of a deep box would lie behind a ray's
    // meeting with the box.
    pub(crate) fn face(&self) -> Pose {
        match self.kind {
            PanelKind::Volume { depth, .. } => {
                self.pose
                    .then(&Pose::at([0.0, 0.0, depth * METERS_PER_DP / 2.0]))
            }
            PanelKind::Content | PanelKind::Surface(_) => self.pose,
        }
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The sizes that the user may give the panel with its corner handle, or `None` for a panel
    // that has none. A panel on a curve has none: its corner does not lie on its tangent plane.
    pub(crate) fn resize_limits(&self) -> Option<ResizeLimits> {
        self.resize.filter(|_| self.shape == Shape::Flat)
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Where `ray` meets the panel as it shows: flat or on its curve.
    pub(crate) fn hit(&self, ray: &Ray) -> Option<PanelHit> {
        let size = self.size().map(|dp| dp * METERS_PER_DP);
        match (self.kind, self.shape) {
            (PanelKind::Volume { depth, .. }, _) => {
                ray.hit_box(&self.pose, [size[0], size[1], depth * METERS_PER_DP])
            }
            (_, Shape::Flat) => ray.hit_panel(&self.pose, size),
            (_, Shape::Curved { radius }) => ray.hit_cylinder(&self.pose, size, radius),
        }
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Where the point `at` of the room stands against the panel's face, flat or on its curve. A
    // volume has no face that a finger touches: its box has depth, and its meshes take a ray.
    pub(crate) fn face_place(&self, at: [f32; 3]) -> Option<FacePlace> {
        if matches!(self.kind, PanelKind::Volume { .. }) {
            return None;
        }
        let local = self.pose.unrotate([
            at[0] - self.pose.position[0],
            at[1] - self.pose.position[1],
            at[2] - self.pose.position[2],
        ]);
        let (across, height) = match self.shape {
            Shape::Flat => (local[0], local[2]),
            // The cylinder's axis stands `radius` in front of the panel's center, so a point
            // nearer the axis than the radius stands in front of the face
            Shape::Curved { radius } => {
                let from_axis = local[0].hypot(local[2] - radius);
                (
                    local[0].atan2(radius - local[2]) * radius,
                    radius - from_axis,
                )
            }
        };
        let [width, tall] = self.size();
        Some(FacePlace {
            x: across / METERS_PER_DP + width / 2.0,
            y: tall / 2.0 - local[1] / METERS_PER_DP,
            height,
        })
    }
}

// Where a point stands against a panel's face: its place across the face in dp from the panel's
// top left corner, which may lie outside the panel, and its height above the face toward the
// panel's front in meters, below zero behind the face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FacePlace {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) height: f32,
}

impl FacePlace {
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Whether the place lies within the edges of `slot`'s panel.
    pub(crate) fn within(&self, slot: &PanelSlot) -> bool {
        let [width, height] = slot.size();
        (0.0..=width).contains(&self.x) && (0.0..=height).contains(&self.y)
    }
}

// The shape a panel shows on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    Flat,
    // A cylinder of `radius` meters. Its upright axis stands that far from the panel's center
    // along the panel's +z, and the panel's width is arc length.
    Curved { radius: f32 },
}

// How a following subspace places a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ride {
    // No controller places the panel.
    Free,
    // The panel stands where the controller of this hand puts it.
    On(Hand),
    // The panel's subspace has no place: it follows a controller that the runtime does not
    // track. The panel shows nowhere and takes no hits.
    Hidden,
}

// A tracked space that the panels of a following subspace ride in, when the subspace follows every
// move of its target. The headset hands their layers to the compositor in that space, at the
// panel's pose relative to the space. The compositor then places them with the newest pose of the
// head or the controller, at the time the frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrackedSpace {
    // The head's own space
    View,
    // The space of the aim pose of the controller in this hand
    Aim(Hand),
}

// Whether the user may carry a panel with a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mobility {
    Fixed,
    Movable,
    // Movable by a handle that the content draws itself, such as a hologram's projector: the
    // handle's sphere shows only while a ray points at it or presses it
    MovableByOwnHandle,
}

impl Mobility {
    pub(crate) fn movable(self) -> bool {
        self != Self::Fixed
    }
}
