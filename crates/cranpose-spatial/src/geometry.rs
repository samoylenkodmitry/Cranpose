use std::{
    f32::consts::{PI, TAU},
    ops::{Add, Mul, Sub},
};

use cranpose_ui_graphics::{ProjectiveTransform, Size};

const DEPTH_PER_WIDTH: f32 = 1.6;
const STANDING_DEPTH_SHARE: f32 = 0.2;

/// A position or a direction in a room, in logical pixels: `x` runs to the
/// right, `y` down and `z` away from a viewer who faces into the room.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    /// To the right.
    pub x: f32,
    /// Down.
    pub y: f32,
    /// Away from the viewer.
    pub z: f32,
}

impl Vec3 {
    /// The vector of the three components.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// The dot product.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// The cross product, by the right-hand rule.
    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// The vector `t` of the way from `self` to `other`.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }
}

/// A flat rectangle in a room: its top-left corner and the steps of its own
/// `x` and `y` axes, one logical pixel long each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// The rectangle's top-left corner.
    pub origin: Vec3,
    /// One step along the rectangle's own `x` axis.
    pub right: Vec3,
    /// One step along the rectangle's own `y` axis.
    pub down: Vec3,
}

/// An eye in a room and the picture it sees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Where the eye is.
    pub position: Vec3,
    /// Turn to the right about the vertical axis, in radians. Zero faces
    /// along `z`.
    pub yaw: f32,
    /// Tilt upward, in radians.
    pub pitch: f32,
    /// The distance of the picture plane from the eye, in logical pixels: a
    /// surface this far away that faces the camera shows at its own size.
    pub focal_length: f32,
}

impl Camera {
    /// The direction the camera looks in.
    pub fn forward(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        Vec3::new(sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch)
    }

    /// The direction to the camera's right, level with the floor.
    pub fn right(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        Vec3::new(cos_yaw, 0.0, -sin_yaw)
    }

    /// The map from `plane`'s own coordinates onto a viewport of
    /// `viewport` size whose center the camera looks through. Its `w` is a
    /// point's depth in front of the eye over the focal length, so a layer
    /// drawn with it is cut [`NEAR_W`](cranpose_ui_graphics::transform::NEAR_W)
    /// focal lengths in front of the eye.
    pub fn projection(&self, plane: &Plane, viewport: Size) -> ProjectiveTransform {
        let (forward, right) = (self.forward(), self.right());
        let down = forward.cross(right);
        let view = |v: Vec3| Vec3::new(v.dot(right), v.dot(down), v.dot(forward));
        let (center_x, center_y) = (viewport.width * 0.5, viewport.height * 0.5);
        let focal = self.focal_length;
        let column = |v: Vec3| {
            let v = view(v);
            [
                v.x + center_x * v.z / focal,
                v.y + center_y * v.z / focal,
                v.z / focal,
            ]
        };
        let [a, b, c] = [
            column(plane.right),
            column(plane.down),
            column(plane.origin - self.position),
        ];
        ProjectiveTransform::from_rows([[a[0], b[0], c[0]], [a[1], b[1], c[1]], [a[2], b[2], c[2]]])
    }

    /// The camera `t` of the way from `self` to `other`, turning the short
    /// way round.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let turn = (other.yaw - self.yaw + PI).rem_euclid(TAU) - PI;
        Self {
            position: self.position.lerp(other.position, t),
            yaw: self.yaw + turn * t,
            pitch: self.pitch + (other.pitch - self.pitch) * t,
            focal_length: self.focal_length + (other.focal_length - self.focal_length) * t,
        }
    }
}

/// The focal length that shows `field_of_view` radians across a viewport
/// `viewport_height` tall.
pub fn focal_length_for(viewport_height: f32, field_of_view: f32) -> f32 {
    viewport_height * 0.5 / (field_of_view * 0.5).tan()
}

/// A face of a box-shaped room.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Wall {
    /// The far wall, which a flat room shows as its screen.
    North,
    /// The wall on the right of a viewer who faces north.
    East,
    /// The wall behind a viewer who faces north.
    South,
    /// The wall on the left of a viewer who faces north.
    West,
    /// The floor.
    Floor,
    /// The ceiling.
    Ceiling,
}

impl Wall {
    /// Every face, the north wall first.
    pub const ALL: [Wall; 6] = [
        Wall::North,
        Wall::East,
        Wall::South,
        Wall::West,
        Wall::Floor,
        Wall::Ceiling,
    ];
}

/// A box-shaped room seen from inside, in logical pixels: `width` along `x`,
/// `height` along `y` and `depth` along `z`. The north wall lies at
/// `z = depth`, the south wall at `z = 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoomShape {
    /// From the west wall to the east wall.
    pub width: f32,
    /// From the ceiling to the floor.
    pub height: f32,
    /// From the south wall to the north wall.
    pub depth: f32,
}

impl RoomShape {
    /// The room whose north wall fills `viewport`, deep enough for a camera
    /// with `focal_length` to show the north wall at its own size from
    /// inside.
    pub fn for_viewport(viewport: Size, focal_length: f32) -> Self {
        let width = viewport.width.max(1.0);
        Self {
            width,
            height: viewport.height.max(1.0),
            depth: (width * DEPTH_PER_WIDTH).max(focal_length * 1.5),
        }
    }

    /// The size of `wall`'s surface.
    pub fn wall_size(&self, wall: Wall) -> Size {
        let (width, height) = match wall {
            Wall::North | Wall::South => (self.width, self.height),
            Wall::East | Wall::West => (self.depth, self.height),
            Wall::Floor | Wall::Ceiling => (self.width, self.depth),
        };
        Size { width, height }
    }

    /// Where `wall` lies, its own axes reading the right way round from
    /// inside the room.
    pub fn plane(&self, wall: Wall) -> Plane {
        let (w, h, d) = (self.width, self.height, self.depth);
        let (origin, right, down) = match wall {
            Wall::North => (Vec3::new(0.0, 0.0, d), Vec3::new(1.0, 0.0, 0.0), DOWN),
            Wall::South => (Vec3::new(w, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), DOWN),
            Wall::West => (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), DOWN),
            Wall::East => (Vec3::new(w, 0.0, d), Vec3::new(0.0, 0.0, -1.0), DOWN),
            Wall::Floor => (
                Vec3::new(0.0, h, d),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, -1.0),
            ),
            Wall::Ceiling => (
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ),
        };
        Plane {
            origin,
            right,
            down,
        }
    }

    /// The camera that shows the north wall at its own size, filling a
    /// viewport as wide and tall as the wall: the flat screen.
    pub fn screen_camera(&self, focal_length: f32) -> Camera {
        Camera {
            position: Vec3::new(
                self.width * 0.5,
                self.height * 0.5,
                self.depth - focal_length,
            ),
            yaw: 0.0,
            pitch: 0.0,
            focal_length,
        }
    }

    /// A camera that stands near the south wall and looks north.
    pub fn standing_camera(&self, focal_length: f32) -> Camera {
        Camera {
            position: Vec3::new(
                self.width * 0.5,
                self.height * 0.5,
                self.depth * STANDING_DEPTH_SHARE,
            ),
            yaw: 0.0,
            pitch: 0.0,
            focal_length,
        }
    }

    /// `position` moved inside the room, at least `margin` away from each
    /// wall it could walk into.
    pub fn keep_inside(&self, position: Vec3, margin: f32) -> Vec3 {
        let clamp = |value: f32, extent: f32| {
            let margin = margin.min(extent * 0.5);
            value.clamp(margin, extent - margin)
        };
        Vec3::new(
            clamp(position.x, self.width),
            position.y,
            clamp(position.z, self.depth),
        )
    }
}

const DOWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);
