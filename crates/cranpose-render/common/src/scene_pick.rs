use std::cell::Cell;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{
    Point, Size,
    space::{Camera, Mat4, MeshSurface, Ray, SurfaceHit, Vec3},
};

use crate::{
    graph::{LayerNode, ProjectiveTransform},
    space::{EyeView, SpaceTree, node_size},
};

/// How much past a doorway a ray that goes through it looks for what lies
/// beyond, against its distance: the surface the doorway lies on is not
/// beyond it.
const PAST_DOORWAY: f32 = 1.0e-4;

/// Finds which mesh surface of a scene a window point, or any ray, shows,
/// and the point of the surface's content there: the nearest surface along
/// the ray in the space the window's eye stands in, or behind a doorway the
/// ray meets first, in the spaces the eye sees.
pub struct ScenePick {
    /// The node of the scene the window shows.
    scene: Option<NodeId>,
    camera: Camera,
    size: Size,
    window_to_scene: ProjectiveTransform,
    scene_to_window: ProjectiveTransform,
    surfaces: Vec<PickSurface>,
    seen: Vec<PickSpace>,
    /// The scene nodes of every space of the scene, seen or not.
    scenes: Vec<NodeId>,
    /// The scene nodes of the spaces the eye sees.
    seen_scenes: Vec<NodeId>,
    /// The last window point picked, and what it met.
    last: Cell<Option<LastPick>>,
}

/// A space the eye sees, as the pick goes into it: the seen space whose
/// doorway it shows through, and the map onto that doorway's unit square.
struct PickSpace {
    through: Option<usize>,
    to_door: Mat4,
}

impl PickSpace {
    /// How far along `ray` it passes through the doorway, if it does.
    fn meet(&self, ray: Ray) -> Option<f32> {
        let origin = self.to_door.transform_point(ray.origin);
        let direction = self.to_door.transform_vector(ray.direction);
        if direction.z.abs() < f32::EPSILON {
            return None;
        }
        let distance = -origin.z / direction.z;
        let at = origin + direction * distance;
        let inside = (0.0..=1.0).contains(&at.x) && (0.0..=1.0).contains(&at.y);
        (inside && distance > 0.0).then_some(distance)
    }
}

/// A window point and the surface it met, if any, with where.
type LastPick = (Point, Option<(usize, SurfaceHit)>);

struct PickSurface {
    node_id: Option<NodeId>,
    surface: MeshSurface,
    size: Size,
    /// The seen space the surface lies in.
    seen: usize,
    /// The triangle the last ray that met this surface met, on whose plane
    /// a drag that leaves the mesh goes on.
    held: Cell<Option<u32>>,
}

impl ScenePick {
    /// The pick of the scene that `layer` shows, with `to_window` mapping
    /// the layer to the window; `None` when the layer shows no scene. Its
    /// surfaces are those the window's eye sees.
    pub fn of(layer: &LayerNode, to_window: ProjectiveTransform) -> Option<Self> {
        let tree = SpaceTree::of(layer)?;
        let view = EyeView::of(&tree, node_size(layer));
        let surfaces = view
            .surfaces
            .iter()
            .map(|seen| PickSurface {
                node_id: seen.layer.node_id,
                surface: seen.surface.clone(),
                size: seen.size,
                seen: seen.seen,
                held: Cell::new(None),
            })
            .collect();
        let seen = view
            .seen
            .iter()
            .map(|space| PickSpace {
                through: space.through,
                to_door: space
                    .door
                    .and_then(|door| door.quad.inverse())
                    .unwrap_or(Mat4::IDENTITY),
            })
            .collect();
        Some(Self {
            scene: layer.node_id,
            camera: view.camera,
            size: view.size,
            window_to_scene: to_window.inverse()?,
            scene_to_window: to_window,
            surfaces,
            seen,
            scenes: tree
                .cells
                .iter()
                .filter_map(|cell| cell.layer.node_id)
                .collect(),
            seen_scenes: view
                .seen
                .iter()
                .filter_map(|seen| tree.cells[seen.cell].layer.node_id)
                .collect(),
            last: Cell::new(None),
        })
    }

    /// The node of the scene the window shows.
    pub fn scene(&self) -> Option<NodeId> {
        self.scene
    }

    /// Whether `layer` shows one of this scene's spaces, a scene behind one
    /// of its doorways, whose surfaces this pick finds.
    pub fn holds_scene(&self, layer: &LayerNode) -> bool {
        layer
            .node_id
            .is_some_and(|node_id| self.scenes.contains(&node_id))
    }

    /// Whether the eye sees the space of the scene `layer` shows, through
    /// the doorways before it.
    pub fn sees_scene(&self, layer: &LayerNode) -> bool {
        layer
            .node_id
            .is_some_and(|node_id| self.seen_scenes.contains(&node_id))
    }

    /// The index among the scene's surfaces of the surface layer `layer`.
    pub fn index_of(&self, layer: &LayerNode) -> Option<usize> {
        let node_id = layer.node_id?;
        self.surfaces
            .iter()
            .position(|surface| surface.node_id == Some(node_id))
    }

    /// The content point of surface `index` that the window point shows,
    /// when that surface is the nearest one there.
    pub fn content_point(&self, window: Point, index: usize) -> Option<Point> {
        let (nearest, hit) = self.nearest(window)?;
        (nearest == index).then_some(hit.point)
    }

    /// The surface `ray`, in the space the window's eye stands in, meets
    /// first, by its index, and where.
    pub fn nearest_along(&self, ray: Ray) -> Option<(usize, SurfaceHit)> {
        self.nearest_in(ray, 0, f32::NEG_INFINITY)
    }

    /// The content point of surface `index` under the window point for a
    /// gesture that surface holds: where the ray meets it, or past its edge,
    /// where the ray meets the plane of the triangle it last met.
    pub fn drag_point(&self, window: Point, index: usize) -> Option<Point> {
        self.drag_point_along(self.ray(window)?, index)
    }

    /// [`Self::drag_point`] for a ray in the space the window's eye stands
    /// in.
    pub fn drag_point_along(&self, ray: Ray, index: usize) -> Option<Point> {
        let target = self.surfaces.get(index)?;
        if let Some(hit) = target.surface.cast(ray, target.size) {
            target.held.set(Some(hit.triangle));
            return Some(hit.point);
        }
        target
            .surface
            .cast_plane(ray, target.held.get()?, target.size)
    }

    /// Where the content point `content` of surface `index` shows in the
    /// window, also for a surface behind a doorway; `None` when it lies
    /// nearer than the camera's near plane or behind the eye.
    pub fn window_point(&self, index: usize, content: Point) -> Option<Point> {
        let target = self.surfaces.get(index)?;
        let at = target.surface.point_at(content, target.size)?;
        let shown = self.camera.project(at, self.size)?;
        Some(self.scene_to_window.map_point(shown))
    }

    /// Where `point`, in the space the window's eye stands in, shows in the
    /// window; `None` behind the eye.
    pub fn window_point_of(&self, point: Vec3) -> Option<Point> {
        let shown = self.camera.project(point, self.size)?;
        Some(self.scene_to_window.map_point(shown))
    }

    fn ray(&self, window: Point) -> Option<Ray> {
        let local = self.window_to_scene.map_point(window);
        self.camera.ray(local, self.size)
    }

    fn nearest(&self, window: Point) -> Option<(usize, SurfaceHit)> {
        if let Some((point, nearest)) = self.last.get()
            && point == window
        {
            return nearest;
        }
        let nearest = self.ray(window).and_then(|ray| self.nearest_along(ray));
        if let Some((index, hit)) = nearest {
            self.surfaces[index].held.set(Some(hit.triangle));
        }
        self.last.set(Some((window, nearest)));
        nearest
    }

    /// The nearest surface of seen space `seen` that `ray` meets past
    /// `beyond`; where it first passes through the doorway of a space seen
    /// through this one, the nearest surface on the doorway's other side,
    /// or this space's own where that side shows none.
    fn nearest_in(&self, ray: Ray, seen: usize, beyond: f32) -> Option<(usize, SurfaceHit)> {
        let surface = self
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, target)| target.seen == seen)
            .filter_map(|(index, target)| {
                target
                    .surface
                    .cast(ray, target.size)
                    .filter(|hit| hit.distance > beyond)
                    .map(|hit| (index, hit))
            })
            .min_by(|(_, a), (_, b)| a.distance.total_cmp(&b.distance));
        let doorway = self
            .seen
            .iter()
            .enumerate()
            .filter(|(_, space)| space.through == Some(seen))
            .filter_map(|(index, space)| {
                space
                    .meet(ray)
                    .filter(|distance| *distance > beyond)
                    .map(|distance| (index, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b));
        let Some((door, distance)) = doorway.filter(|(_, distance)| {
            surface.is_none_or(|(_, hit)| *distance <= hit.distance * (1.0 + PAST_DOORWAY))
        }) else {
            return surface;
        };
        self.nearest_in(ray, door, distance * (1.0 + PAST_DOORWAY))
            .or(surface)
    }
}

/// The mesh surface layer a child of a scene layer is, or holds as the
/// wrapper of its outer draws.
pub fn surface_layer(child: &LayerNode) -> Option<&LayerNode> {
    let layer = match child.wraps {
        None => child,
        Some(node_id) => child.children.iter().find_map(|inner| match inner {
            crate::graph::RenderNode::Layer(inner) if inner.node_id == Some(node_id) => {
                Some(&**inner)
            }
            _ => None,
        })?,
    };
    cranpose_ui_graphics::layer_transform::layer_surface(&layer.graphics_layer).map(|_| layer)
}
