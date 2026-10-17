use std::cell::Cell;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{
    Point, Size,
    layer_transform::{layer_scene, layer_surface},
    space::{Camera, MeshSurface, SurfaceHit},
};

use crate::graph::{LayerNode, ProjectiveTransform, RenderNode};

/// Finds which mesh surface of a scene a window point shows, and the point
/// of the surface's content there: the nearest surface along the camera's
/// ray through the point.
pub struct ScenePick {
    camera: Camera,
    size: Size,
    window_to_scene: ProjectiveTransform,
    surfaces: Vec<PickSurface>,
    /// The last window point picked, and what it met.
    last: Cell<Option<LastPick>>,
}

/// A window point and the surface it met, if any, with where.
type LastPick = (Point, Option<(usize, SurfaceHit)>);

struct PickSurface {
    node_id: Option<NodeId>,
    surface: MeshSurface,
    size: Size,
    /// The triangle the last ray that met this surface met, on whose plane
    /// a drag that leaves the mesh goes on.
    held: Cell<Option<u32>>,
}

impl ScenePick {
    /// The pick of the scene that `layer` shows, with `to_window` mapping
    /// the layer to the window; `None` when the layer shows no scene. Its
    /// surfaces are the layer's children that are mesh surfaces, also those
    /// a wrapper of outer draws holds.
    pub fn of(layer: &LayerNode, to_window: ProjectiveTransform) -> Option<Self> {
        let scene = layer_scene(&layer.graphics_layer)?;
        let node_rect = layer.node_rect();
        let surfaces = layer
            .children
            .iter()
            .filter_map(|child| match child {
                RenderNode::Layer(child) => surface_layer(child),
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
            })
            .filter_map(|surface_layer| {
                let surface = layer_surface(&surface_layer.graphics_layer)?;
                let rect = surface_layer.node_rect();
                Some(PickSurface {
                    node_id: surface_layer.node_id,
                    surface: surface.clone(),
                    size: Size {
                        width: rect.width,
                        height: rect.height,
                    },
                    held: Cell::new(None),
                })
            })
            .collect();
        Some(Self {
            camera: scene.camera,
            size: Size {
                width: node_rect.width,
                height: node_rect.height,
            },
            window_to_scene: to_window.inverse()?,
            surfaces,
            last: Cell::new(None),
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
        let nearest = self.ray(window).and_then(|ray| {
            self.surfaces
                .iter()
                .enumerate()
                .filter_map(|(index, target)| {
                    target
                        .surface
                        .cast(ray, target.size)
                        .map(|hit| (index, hit))
                })
                .min_by(|(_, a), (_, b)| a.distance.total_cmp(&b.distance))
        });
        if let Some((index, hit)) = nearest {
            self.surfaces[index].held.set(Some(hit.triangle));
        }
        self.last.set(Some((window, nearest)));
        nearest
    }
}

/// The mesh surface layer a child of a scene layer is, or holds as the
/// wrapper of its outer draws.
pub fn surface_layer(child: &LayerNode) -> Option<&LayerNode> {
    let layer = match child.wraps {
        None => child,
        Some(node_id) => child.children.iter().find_map(|inner| match inner {
            RenderNode::Layer(inner) if inner.node_id == Some(node_id) => Some(&**inner),
            _ => None,
        })?,
    };
    layer_surface(&layer.graphics_layer).map(|_| layer)
}
