use std::cell::Cell;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{
    Point, Size,
    layer_transform::{layer_scene, layer_surface},
    space::{Camera, Mat4, MeshSurface, SurfaceHit},
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
    /// a wrapper of outer draws holds, and the surfaces that join the scene
    /// from inside their content.
    pub fn of(layer: &LayerNode, to_window: ProjectiveTransform) -> Option<Self> {
        let scene = layer_scene(&layer.graphics_layer)?;
        let node_rect = layer.node_rect();
        let mut surfaces = Vec::new();
        for_each_scene_surface(layer, &mut |surface_layer, surface| {
            surfaces.push(PickSurface {
                node_id: surface_layer.node_id,
                surface,
                size: node_size(surface_layer),
                held: Cell::new(None),
            });
        });
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

/// Visits each mesh surface of the scene `scene_layer` shows, in draw order,
/// with its place in the scene: each surface child, and after each the
/// surfaces that join the scene from inside its content.
pub fn for_each_scene_surface(
    scene_layer: &LayerNode,
    visit: &mut impl FnMut(&LayerNode, MeshSurface),
) {
    walk_scene_surfaces(scene_layer, true, visit);
}

/// Visits the mesh surfaces that join the scene `scene_layer` shows from
/// inside the content of its surface children, in draw order, with their
/// places in the scene.
pub fn for_each_joined_surface(
    scene_layer: &LayerNode,
    visit: &mut impl FnMut(&LayerNode, MeshSurface),
) {
    walk_scene_surfaces(scene_layer, false, visit);
}

fn walk_scene_surfaces(
    scene_layer: &LayerNode,
    direct: bool,
    visit: &mut impl FnMut(&LayerNode, MeshSurface),
) {
    for child in &scene_layer.children {
        let RenderNode::Layer(child) = child else {
            continue;
        };
        let Some(surface_layer) = surface_layer(child) else {
            continue;
        };
        let Some(surface) = layer_surface(&surface_layer.graphics_layer) else {
            continue;
        };
        let inner = joined_space(surface_layer, surface);
        if direct {
            visit(surface_layer, surface.clone());
        }
        if let Some(inner) = inner {
            visit_joined_under(surface_layer, &inner, visit);
        }
    }
}

/// The space the mesh surfaces inside the content of `surface_layer`, whose
/// surface is `surface`, are placed in; `None` when it holds none.
fn joined_space(surface_layer: &LayerNode, surface: &MeshSurface) -> Option<Mat4> {
    let holds_surfaces = surface_layer
        .children
        .iter()
        .any(|child| matches!(child, RenderNode::Layer(child) if child.has_surfaces()));
    if !holds_surfaces {
        return None;
    }
    surface.content_to_scene(node_size(surface_layer))
}

/// Visits the mesh surfaces under `layer` that join the scene, placed
/// through `to_scene`; a scene inside keeps the surfaces inside it.
fn visit_joined_under(
    layer: &LayerNode,
    to_scene: &Mat4,
    visit: &mut impl FnMut(&LayerNode, MeshSurface),
) {
    for child in &layer.children {
        let RenderNode::Layer(child) = child else {
            continue;
        };
        if !child.has_surfaces() || layer_scene(&child.graphics_layer).is_some() {
            continue;
        }
        let joined = surface_layer(child).and_then(|surface_layer| {
            layer_surface(&surface_layer.graphics_layer).map(|surface| (surface_layer, surface))
        });
        match joined {
            Some((surface_layer, surface)) => {
                let placed = surface.placed_by(to_scene);
                let inner = joined_space(surface_layer, &placed);
                visit(surface_layer, placed);
                if let Some(inner) = inner {
                    visit_joined_under(surface_layer, &inner, visit);
                }
            }
            None => visit_joined_under(child, to_scene, visit),
        }
    }
}

fn node_size(layer: &LayerNode) -> Size {
    let rect = layer.node_rect();
    Size {
        width: rect.width,
        height: rect.height,
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
