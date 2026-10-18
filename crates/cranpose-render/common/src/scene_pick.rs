use std::cell::Cell;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{
    Point, Rect, Size,
    layer_transform::layer_scene,
    space::{Camera, MeshSurface, Ray, SurfaceHit},
};

use crate::{
    graph::{LayerNode, ProjectiveTransform},
    scene_space::{SceneItem, for_each_scene_item, node_size},
};

/// Finds which mesh surface of a scene a window point shows, and the point
/// of the surface's content there: the nearest surface along the camera's
/// ray through the point, through the windows of the portals it meets.
pub struct ScenePick {
    camera: Camera,
    size: Size,
    window_to_scene: ProjectiveTransform,
    surfaces: Vec<PickSurface>,
    portals: Vec<PickPortal>,
    /// The last window point picked, and what it met.
    last: Cell<Option<LastPick>>,
}

/// A portal as the pick sees it: the surface it lies on and its rect there,
/// the portal whose space holds that surface, and the scene it shows.
struct PickPortal {
    host: usize,
    window: Rect,
    parent: Option<usize>,
    scene_node: Option<NodeId>,
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
    pub fn of(layer: &LayerNode, to_window: ProjectiveTransform) -> Option<Self> {
        let scene = layer_scene(&layer.graphics_layer)?;
        let node_rect = layer.node_rect();
        let (mut surfaces, mut portals) = (Vec::new(), Vec::new());
        for_each_scene_item(layer, &mut |item| match item {
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
                host: portal.host,
                window: portal.window,
                parent: portal.parent,
                scene_node: portal.scene_layer.node_id,
            }),
        });
        Some(Self {
            camera: scene.camera,
            size: Size {
                width: node_rect.width,
                height: node_rect.height,
            },
            window_to_scene: to_window.inverse()?,
            surfaces,
            portals,
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
            .and_then(|ray| self.nearest_in(ray, None, f32::NEG_INFINITY));
        if let Some((index, hit)) = nearest {
            self.surfaces[index].held.set(Some(hit.triangle));
        }
        self.last.set(Some((window, nearest)));
        nearest
    }

    /// The nearest surface of the space of `portal` that `ray` meets past
    /// `beyond`: where it meets a surface inside one of the windows on it,
    /// the nearest surface of that portal's space past the window, or the
    /// surface itself where that space shows none.
    fn nearest_in(
        &self,
        ray: Ray,
        portal: Option<usize>,
        beyond: f32,
    ) -> Option<(usize, SurfaceHit)> {
        let (index, hit) = self
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, target)| target.portal == portal)
            .filter_map(|(index, target)| {
                target
                    .surface
                    .cast(ray, target.size)
                    .filter(|hit| hit.distance > beyond)
                    .map(|hit| (index, hit))
            })
            .min_by(|(_, a), (_, b)| a.distance.total_cmp(&b.distance))?;
        let through = self.portals.iter().position(|window| {
            window.host == index
                && window.parent == portal
                && window.window.contains(hit.point.x, hit.point.y)
        });
        through
            .and_then(|inner| self.nearest_in(ray, Some(inner), hit.distance))
            .or(Some((index, hit)))
    }
}

pub use crate::scene_space::surface_layer;
