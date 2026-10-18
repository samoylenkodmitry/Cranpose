use std::cell::Cell;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{
    Point, Size,
    layer_transform::layer_scene,
    space::{Camera, Mat4, MeshSurface, Ray, SurfaceHit},
};

use crate::{
    graph::{LayerNode, ProjectiveTransform},
    scene_space::{SceneItem, Viewer, for_each_scene_item, node_size, portal_limit, viewer_space},
};

/// How much past a doorway a ray that goes through it looks for what lies
/// beyond, against its distance: the surface the doorway lies on is not
/// beyond it.
const PAST_DOORWAY: f32 = 1.0e-4;

/// Finds which mesh surface of a scene a window point shows, and the point
/// of the surface's content there: the nearest surface along the camera's
/// ray through the point, from the space the scene's viewer stands in and
/// through the doorways the ray meets.
pub struct ScenePick {
    camera: Camera,
    size: Size,
    window_to_scene: ProjectiveTransform,
    scene_to_window: ProjectiveTransform,
    surfaces: Vec<PickSurface>,
    portals: Vec<PickPortal>,
    /// The portal whose space the viewer stands in; `None` for the scene's
    /// own.
    viewer: Option<usize>,
    /// The last window point picked, and what it met.
    last: Cell<Option<LastPick>>,
}

/// A portal as the pick sees it: the portal whose space holds its doorway,
/// the scene it shows and where the doorway lies.
struct PickPortal {
    parent: Option<usize>,
    scene_node: Option<NodeId>,
    /// Maps the scene onto the unit square the doorway spans.
    scene_to_door: Mat4,
}

impl PickPortal {
    /// How far along `ray` it passes through the doorway, if it does.
    fn meet(&self, ray: Ray) -> Option<f32> {
        let origin = self.scene_to_door.transform_point(ray.origin);
        let direction = self.scene_to_door.transform_vector(ray.direction);
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
    /// The portal whose space the surface is in; `None` for the scene's own.
    portal: Option<usize>,
    /// The triangle the last ray that met this surface met, on whose plane
    /// a drag that leaves the mesh goes on.
    held: Cell<Option<u32>>,
}

impl ScenePick {
    /// The pick of the scene that `layer` shows, with `to_window` mapping
    /// the layer to the window; `None` when the layer shows no scene. Its
    /// surfaces are the layer's children that are mesh surfaces, also those
    /// a wrapper of outer draws holds, the surfaces that join the scene from
    /// inside their content, and the surfaces of the spaces its portals
    /// show.
    pub fn of(
        layer: &LayerNode,
        to_window: ProjectiveTransform,
        viewers: &[Viewer],
    ) -> Option<Self> {
        let scene = layer_scene(&layer.graphics_layer)?;
        let node_rect = layer.node_rect();
        let path = viewers
            .iter()
            .find(|viewer| Some(viewer.scene) == layer.node_id)
            .map_or(&[][..], |viewer| viewer.path.as_slice());
        let (mut surfaces, mut portals) = (Vec::new(), Vec::new());
        for_each_scene_item(layer, portal_limit(path.len()), &mut |item| match item {
            SceneItem::Surface {
                layer,
                surface,
                portal,
            } => surfaces.push(PickSurface {
                node_id: layer.node_id,
                surface,
                size: node_size(layer),
                portal,
                held: Cell::new(None),
            }),
            SceneItem::Portal(portal) => portals.push(PickPortal {
                parent: portal.parent,
                scene_node: portal.scene_layer.node_id,
                scene_to_door: portal.window_to_scene.inverse().unwrap_or(Mat4::IDENTITY),
            }),
        });
        Some(Self {
            camera: scene.camera,
            size: Size {
                width: node_rect.width,
                height: node_rect.height,
            },
            window_to_scene: to_window.inverse()?,
            scene_to_window: to_window,
            surfaces,
            portals,
            viewer: viewer_space(layer, path),
            last: Cell::new(None),
        })
    }

    /// Whether the scene `layer` shows is one of this scene's portals, whose
    /// surfaces this pick finds.
    pub fn shows_through_portal(&self, layer: &LayerNode) -> bool {
        layer.node_id.is_some_and(|node_id| {
            self.portals
                .iter()
                .any(|portal| portal.scene_node == Some(node_id))
        })
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

    /// The content point of surface `index` under the window point for a
    /// gesture that surface holds: where the ray meets it, or past its edge,
    /// where the ray meets the plane of the triangle it last met.
    pub fn drag_point(&self, window: Point, index: usize) -> Option<Point> {
        let target = self.surfaces.get(index)?;
        let ray = self.ray(window)?;
        if let Some(hit) = target.surface.cast(ray, target.size) {
            target.held.set(Some(hit.triangle));
            return Some(hit.point);
        }
        target
            .surface
            .cast_plane(ray, target.held.get()?, target.size)
    }

    /// Where the content point `content` of surface `index` shows in the
    /// window, also for a surface in a portal's space; `None` when it lies
    /// nearer than the camera's near plane or behind the eye.
    pub fn window_point(&self, index: usize, content: Point) -> Option<Point> {
        let target = self.surfaces.get(index)?;
        let at = target.surface.point_at(content, target.size)?;
        let shown = self.camera.project(at, self.size)?;
        Some(self.scene_to_window.map_point(shown))
    }

    fn ray(&self, window: Point) -> Option<cranpose_ui_graphics::space::Ray> {
        let local = self.window_to_scene.map_point(window);
        self.camera.ray(local, self.size)
    }

    fn nearest(&self, window: Point) -> Option<(usize, SurfaceHit)> {
        if let Some((point, nearest)) = self.last.get()
            && point == window
        {
            return nearest;
        }
        let nearest = self
            .ray(window)
            .and_then(|ray| self.nearest_in(ray, (self.viewer, None), f32::NEG_INFINITY));
        if let Some((index, hit)) = nearest {
            self.surfaces[index].held.set(Some(hit.triangle));
        }
        self.last.set(Some((window, nearest)));
        nearest
    }

    /// The nearest surface of `space`, a portal's or the scene's own, that
    /// `ray` meets past `beyond`; where it first passes through a doorway of
    /// the space other than `came_through`, the nearest surface on the
    /// doorway's other side, or the space's own where that side shows none.
    fn nearest_in(
        &self,
        ray: Ray,
        (space, came_through): (Option<usize>, Option<usize>),
        beyond: f32,
    ) -> Option<(usize, SurfaceHit)> {
        let surface = self
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, target)| target.portal == space)
            .filter_map(|(index, target)| {
                target
                    .surface
                    .cast(ray, target.size)
                    .filter(|hit| hit.distance > beyond)
                    .map(|hit| (index, hit))
            })
            .min_by(|(_, a), (_, b)| a.distance.total_cmp(&b.distance));
        let doorway = self
            .portals
            .iter()
            .enumerate()
            .filter(|(index, portal)| {
                Some(*index) != came_through && (portal.parent == space || Some(*index) == space)
            })
            .filter_map(|(index, portal)| {
                portal
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
        let other = if self.portals[door].parent == space {
            Some(door)
        } else {
            self.portals[door].parent
        };
        self.nearest_in(ray, (other, Some(door)), distance * (1.0 + PAST_DOORWAY))
            .or(surface)
    }
}

pub use crate::scene_space::surface_layer;
