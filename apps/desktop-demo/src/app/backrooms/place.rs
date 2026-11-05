// Where things go in a room's space: how many doorways deep the room lies
// from the eye's room, how a node lays out its content for a mesh, and how
// a thing turns to face the eye.

use cranpose_ui::{
    space::{Mat4, Vec3},
    Alignment, Modifier, Size,
};

const DOWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);

// A room's place among the rooms the eye sees: how many doorways lie
// between it and the eye's room, and the map from the eye's room's
// coordinates into its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Place {
    pub(super) depth: u8,
    pub(super) from_root: Mat4,
}

impl Place {
    // The eye's room, the window scene's.
    pub(super) const ROOT: Self = Self {
        depth: 0,
        from_root: Mat4::IDENTITY,
    };

    // The room behind a doorway of this one, whose coordinates `to_parent`
    // maps into this room's.
    pub(super) fn beyond(self, to_parent: &Mat4) -> Self {
        Self {
            depth: self.depth.saturating_add(1),
            from_root: self
                .from_root
                .then(&to_parent.inverse().unwrap_or(Mat4::IDENTITY)),
        }
    }
}

// A node whose content of `size` a mesh shows, laid out at its parent's
// corner whatever room the parent leaves, so the content's frame in its
// scene is the parent's: a doorway is smaller than the things in the room
// behind it.
pub(super) fn spot(size: Size) -> Modifier {
    Modifier::empty()
        .wrap_content_size(Alignment::TOP_START, true)
        .size(size)
}

// Places a mesh scaled by `scale` at `center`, turned so the middle of its
// content, which faces its -z, faces `toward`.
pub(super) fn facing(center: Vec3, toward: Vec3, scale: Vec3) -> Mat4 {
    let z = (center - toward)
        .normalized()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let x = DOWN
        .cross(z)
        .normalized()
        .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    let y = z.cross(x);
    Mat4::from_axes(x * scale.x, y * scale.y, z * scale.z, center)
}

// Places the unit plane upright on the floor at `foot`, `size` wide and
// tall, its content turned to `toward` about the room's down.
pub(super) fn upright(foot: Vec3, toward: Vec3, size: Size) -> Mat4 {
    let level = Vec3::new(foot.x - toward.x, 0.0, foot.z - toward.z);
    let z = level.normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let x = DOWN.cross(z);
    Mat4::from_axes(
        x * size.width,
        DOWN * size.height,
        z,
        foot - x * (size.width * 0.5) - DOWN * size.height,
    )
}
