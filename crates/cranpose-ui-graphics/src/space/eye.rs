//! The eye a window's scene is seen from: where it stands among the scene's
//! space and the spaces behind its doorways, which way it looks, and how it
//! walks among them, stopped by solid surfaces and let through doorways.

use std::{
    ops::Range,
    sync::{Arc, Mutex, PoisonError},
};

use super::{
    math::{Mat4, Vec3},
    mesh::Mesh,
    scene::Camera,
};
use crate::{Point, Rect, Size};

/// The direction a space calls down: its y.
const DOWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);

/// How many doorways one step can take an eye through.
const MAX_CROSSINGS: usize = 4;

/// How many times a step pushes the eye out of the solids it overlaps: more
/// than once lets it settle in a corner between two.
const PUSHES: usize = 3;

/// How many pieces a step is walked in at most, each no longer than half
/// the eye's radius below this many, so it cannot pass through a thin wall.
const MAX_PIECES: f32 = 256.0;

/// How far an eye looks up or down at most, in radians from level.
pub const PITCH_LIMIT: f32 = 1.4;

/// Where an eye stands and which way it looks.
#[derive(Clone, Debug, PartialEq)]
pub struct EyePose {
    /// The scene nodes of the doorways the eye walked through from the
    /// window's scene, outermost first: it stands in the space of the last,
    /// or in the window scene's own when there is none.
    pub doorways: Arc<[usize]>,
    /// Where it stands, in the space it stands in.
    pub position: Vec3,
    /// The direction it looks, of length one.
    pub forward: Vec3,
    /// The direction that shows as down, square to `forward`, of length
    /// one.
    pub down: Vec3,
}

impl EyePose {
    /// An eye in the window scene's own space at `position`, turned `yaw`
    /// radians to the right of the space's z and `pitch` up from level.
    pub fn standing(position: Vec3, yaw: f32, pitch: f32) -> Self {
        let mut pose = Self {
            doorways: Arc::from([]),
            position,
            forward: Vec3::new(0.0, 0.0, 1.0),
            down: DOWN,
        };
        pose.turn(yaw, pitch);
        pose
    }

    /// The camera the eye sees through, with `focal_length`, in the space it
    /// stands in.
    pub fn camera(&self, focal_length: f32) -> Camera {
        Camera::look_at(
            self.position,
            self.position + self.forward,
            self.down,
            focal_length,
        )
    }

    /// How far the eye looks up from level, in radians.
    pub fn pitch(&self) -> f32 {
        (-self.forward.dot(DOWN)).clamp(-1.0, 1.0).asin()
    }

    /// Turns the eye `yaw` radians to the right about its space's down, then
    /// `pitch` up, no further than [`PITCH_LIMIT`] from level.
    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        self.forward = rotated(self.forward, DOWN, yaw);
        self.down = rotated(self.down, DOWN, yaw);
        let level = self.pitch();
        let pitch = (level + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT) - level;
        let right = self.down.cross(self.forward);
        if let Some(right) = right.normalized() {
            self.forward = rotated(self.forward, right, pitch);
            self.down = rotated(self.down, right, pitch);
        }
        self.square_up();
    }

    /// The level directions the eye walks along: ahead, as it looks but
    /// level with its space's floor, and to its right.
    pub fn walking_axes(&self) -> (Vec3, Vec3) {
        let ahead = (self.forward - DOWN * self.forward.dot(DOWN))
            .normalized()
            .unwrap_or(self.forward);
        (ahead, DOWN.cross(ahead))
    }

    /// Keeps `forward` and `down` square to each other and of length one.
    fn square_up(&mut self) {
        let forward = self
            .forward
            .normalized()
            .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        let down = (self.down - forward * self.down.dot(forward))
            .normalized()
            .unwrap_or(DOWN);
        self.forward = forward;
        self.down = down;
    }
}

/// `vector` turned `angle` radians about the unit `axis`, by the right-hand
/// rule.
fn rotated(vector: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    let (sin, cos) = angle.sin_cos();
    vector * cos + axis.cross(vector) * sin + axis * (axis.dot(vector) * (1.0 - cos))
}

/// Where a window's eye stands among the spaces of its scene, as the scene
/// carries it to the renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct EyePlace {
    /// The scene nodes of the doorways the eye walked through, as
    /// [`EyePose::doorways`] holds them.
    pub doorways: Arc<[usize]>,
    /// Where the renderer leaves the eye's world each time it places the
    /// scene.
    pub world: WorldSink,
}

/// Where the renderer leaves an eye's [`EyeWorld`] each time it places the
/// scene, and where the eye reads it when it walks.
#[derive(Clone, Debug, Default)]
pub struct WorldSink(Arc<Mutex<Arc<EyeWorld>>>);

impl WorldSink {
    /// Builds the world again with `build`, in the storage of the last one
    /// while no walk holds it.
    pub fn rebuild(&self, build: impl FnOnce(&mut EyeWorld)) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(world) = Arc::get_mut(&mut held) {
            world.clear();
            build(world);
            return;
        }
        let mut world = EyeWorld::default();
        build(&mut world);
        *held = Arc::new(world);
    }

    /// The world the renderer left last.
    pub fn current(&self) -> Arc<EyeWorld> {
        Arc::clone(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl PartialEq for WorldSink {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// What an eye can bump into and walk through around where it stood when
/// the renderer last placed the scene: the space it stood in, the spaces
/// next to it through doorways, their solid surfaces and the doorways
/// between them, all in the coordinates of the space it stood in.
#[derive(Debug, Default)]
pub struct EyeWorld {
    spaces: Vec<WorldSpace>,
    solids: Vec<WorldSolid>,
    holes: Vec<Rect>,
    doorways: Vec<WorldDoorway>,
}

#[derive(Debug)]
struct WorldSpace {
    doorways: Arc<[usize]>,
    to_base: Mat4,
    from_base: Mat4,
    /// How long a unit of the space is in the world's coordinates.
    scale: f32,
}

#[derive(Debug)]
struct WorldSolid {
    space: usize,
    mesh: Arc<Mesh>,
    transform: Mat4,
    size: Size,
    holes: Range<usize>,
    low: Vec3,
    high: Vec3,
}

#[derive(Debug)]
struct WorldDoorway {
    outer: usize,
    inner: usize,
    /// The doorway's plane, positive in the inner space.
    plane: [f32; 4],
    /// Maps the world onto the unit square the doorway spans.
    to_door: Mat4,
}

impl EyeWorld {
    fn clear(&mut self) {
        self.spaces.clear();
        self.solids.clear();
        self.holes.clear();
        self.doorways.clear();
    }

    /// Adds the space an eye that walked through `doorways` stands in, whose
    /// coordinates `to_base` maps into the world's, and returns its number.
    pub fn add_space(&mut self, doorways: Arc<[usize]>, to_base: Mat4) -> usize {
        let axis = |axis: Vec3| to_base.transform_vector(axis).length();
        let scale = (axis(Vec3::new(1.0, 0.0, 0.0))
            * axis(Vec3::new(0.0, 1.0, 0.0))
            * axis(Vec3::new(0.0, 0.0, 1.0)))
        .cbrt();
        self.spaces.push(WorldSpace {
            doorways,
            from_base: to_base.inverse().unwrap_or(Mat4::IDENTITY),
            to_base,
            scale,
        });
        self.spaces.len() - 1
    }

    /// Adds a surface of space `space` that an eye cannot walk through:
    /// `mesh` placed by `transform` in the world's coordinates, carrying
    /// content of `size` in which `holes` are doorways the eye passes.
    pub fn add_solid(
        &mut self,
        (space, mesh): (usize, Arc<Mesh>),
        transform: Mat4,
        (size, holes): (Size, impl IntoIterator<Item = Rect>),
    ) {
        let start = self.holes.len();
        self.holes.extend(holes);
        let (low, high) = mesh.bounds_in(&transform);
        self.solids.push(WorldSolid {
            space,
            mesh,
            transform,
            size,
            holes: start..self.holes.len(),
            low,
            high,
        });
    }

    /// Adds the doorway from space `outer` into space `inner` that `door`
    /// spans: the unit square placed in the world's coordinates, with
    /// `inner` lying beyond it along its z.
    pub fn add_doorway(&mut self, (outer, inner): (usize, usize), door: Mat4) {
        let corner = door.transform_point(Vec3::ZERO);
        let normal = door
            .transform_vector(Vec3::new(0.0, 0.0, 1.0))
            .normalized()
            .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        self.doorways.push(WorldDoorway {
            outer,
            inner,
            plane: [normal.x, normal.y, normal.z, -normal.dot(corner)],
            to_door: door.inverse().unwrap_or(Mat4::IDENTITY),
        });
    }

    /// Where an eye at `from`, a ball of `radius`, ends up when it walks
    /// `step`, both in the units of the space it stands in: through the
    /// doorways the step passes into the spaces behind them, and pushed out
    /// of the solid surfaces of the space it walks in, along which it
    /// slides. Its ball keeps `radius` in the units of each space it walks
    /// into. The step goes in pieces of at most half the radius in the
    /// smallest space, so it cannot pass through a wall. An eye whose space
    /// the world does not hold walks freely.
    pub fn walk(&self, from: &EyePose, step: Vec3, radius: f32) -> EyePose {
        let Some(start) = self
            .spaces
            .iter()
            .position(|space| space.doorways == from.doorways)
        else {
            return EyePose {
                position: from.position + step,
                ..from.clone()
            };
        };
        let base = &self.spaces[start];
        let step = base.to_base.transform_vector(step);
        let smallest = self
            .spaces
            .iter()
            .map(|space| space.scale)
            .fold(f32::INFINITY, f32::min);
        let pieces = (step.length() / (radius * smallest * 0.5).max(f32::EPSILON))
            .ceil()
            .clamp(1.0, MAX_PIECES);
        let piece = step * (1.0 / pieces);
        let mut at = base.to_base.transform_point(from.position);
        let mut space = start;
        for _ in 0..pieces as usize {
            let end = at + piece;
            let mut along = at;
            for _ in 0..MAX_CROSSINGS {
                let crossed = self
                    .doorways
                    .iter()
                    .find_map(|door| door.crossing(space, (along, end)));
                let Some((next, crossing)) = crossed else {
                    break;
                };
                space = next;
                along = crossing;
            }
            at = self.pushed_out(space, end, radius * self.spaces[space].scale);
        }
        let target = &self.spaces[space];
        let turn = base.to_base.then(&target.from_base);
        let mut pose = EyePose {
            doorways: Arc::clone(&target.doorways),
            position: target.from_base.transform_point(at),
            forward: turn.transform_vector(from.forward),
            down: turn.transform_vector(from.down),
        };
        pose.square_up();
        pose
    }

    /// `at` pushed out of the solids of `space` it lies within `radius` of.
    fn pushed_out(&self, space: usize, mut at: Vec3, radius: f32) -> Vec3 {
        for _ in 0..PUSHES {
            let mut pushed = false;
            for solid in self.solids.iter().filter(|solid| solid.space == space) {
                if let Some(push) = solid.push(at, radius, &self.holes[solid.holes.clone()]) {
                    at = at + push;
                    pushed = true;
                }
            }
            if !pushed {
                break;
            }
        }
        at
    }
}

impl WorldSolid {
    /// How far to move a ball of `radius` at `at` so it no longer reaches
    /// into the surface; `None` when it does not, or reaches it where a
    /// doorway in `holes` opens.
    fn push(&self, at: Vec3, radius: f32, holes: &[Rect]) -> Option<Vec3> {
        let outside =
            |value: f32, low: f32, high: f32| value < low - radius || value > high + radius;
        if outside(at.x, self.low.x, self.high.x)
            || outside(at.y, self.low.y, self.high.y)
            || outside(at.z, self.low.z, self.high.z)
        {
            return None;
        }
        let contact = self.mesh.nearest(at, &self.transform)?;
        if contact.distance >= radius {
            return None;
        }
        let content = Point::new(
            contact.uv[0] * self.size.width,
            contact.uv[1] * self.size.height,
        );
        if holes.iter().any(|hole| hole.contains(content.x, content.y)) {
            return None;
        }
        let away = (at - contact.point).normalized()?;
        Some(away * (radius - contact.distance))
    }
}

impl WorldDoorway {
    /// The space a step from `from` to `to` in `space` passes into through
    /// this doorway, and where it passes; `None` when it does not.
    fn crossing(&self, space: usize, (from, to): (Vec3, Vec3)) -> Option<(usize, Vec3)> {
        let (next, inward) = if space == self.outer {
            (self.inner, true)
        } else if space == self.inner {
            (self.outer, false)
        } else {
            return None;
        };
        let [x, y, z, offset] = self.plane;
        let side = |point: Vec3| x * point.x + y * point.y + z * point.z + offset;
        let (before, after) = (side(from), side(to));
        let passes = if inward {
            before < 0.0 && after >= 0.0
        } else {
            before >= 0.0 && after < 0.0
        };
        if !passes {
            return None;
        }
        let at = from + (to - from) * (before / (before - after));
        let on_door = self.to_door.transform_point(at);
        ((0.0..=1.0).contains(&on_door.x) && (0.0..=1.0).contains(&on_door.y)).then_some((next, at))
    }
}
