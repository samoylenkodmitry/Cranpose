//! The surfaces a scene draws and picks, as one walk: its surface
//! children, the surfaces that join it from inside their content, and the
//! spaces its portals show, the scenes inside its surfaces' content, with
//! their surfaces placed behind their windows.

use cranpose_ui_graphics::{
    Rect, Size,
    layer_transform::{layer_scene, layer_surface},
    space::{Light, MAX_PORTAL_LEVEL, Mat4, MeshSurface, Vec3},
};

use crate::graph::{LayerNode, ProjectiveTransform, RenderNode};

/// What a walk over a scene's surfaces meets, in the order a scene draws
/// it.
pub enum SceneItem<'a> {
    /// A mesh surface, placed in the scene, in the space `portal` shows:
    /// `None` for the scene's own space. Surfaces count from zero in the
    /// order the walk meets them.
    Surface {
        /// The surface's layer.
        layer: &'a LayerNode,
        /// Its mesh surface, placed in the scene.
        surface: MeshSurface,
        /// The portal whose space it is in.
        portal: Option<usize>,
    },
    /// A portal, met before the surfaces in its space.
    Portal(ScenePortal<'a>),
}

/// A scene inside the content of a scene's surface, whose space lies behind
/// the surface, seen through the doorway its node takes on the content.
pub struct ScenePortal<'a> {
    /// The portal's number, counting from zero in the order the walk meets
    /// portals.
    pub index: usize,
    /// The portal whose space holds the surface this one lies on; `None` for
    /// a surface of the scene's own space.
    pub parent: Option<usize>,
    /// How many portals deep the space is: 1 behind a surface of the scene's
    /// own space.
    pub level: u8,
    /// The layer of the scene the portal shows.
    pub scene_layer: &'a LayerNode,
    /// The number of the surface the portal lies on.
    pub host: usize,
    /// The portal's rect on the host's content.
    pub window: Rect,
    /// Places the unit square onto the window, in the scene.
    pub window_to_scene: Mat4,
    /// The window's plane in the scene, `(normal, offset)`: a point `p` lies
    /// beyond it, in the portal's space, where `normal . p + offset >= 0`.
    pub clip: [f32; 4],
    /// The portal scene's light, in the scene.
    pub light: Option<Light>,
}

/// Visits each mesh surface and portal of the scene `scene_layer` shows, in
/// draw order: each surface child, and after each the surfaces that join
/// the scene from inside its content and the portals there.
pub fn for_each_scene_item(scene_layer: &LayerNode, visit: &mut impl FnMut(SceneItem<'_>)) {
    Walk::new(visit).space(scene_layer, (None, 0), None, true);
}

/// Visits what [`for_each_scene_item`] does but the scene's surface
/// children themselves: what the surfaces' content adds to the scene.
pub fn for_each_joined_item(scene_layer: &LayerNode, visit: &mut impl FnMut(SceneItem<'_>)) {
    Walk::new(visit).space(scene_layer, (None, 0), None, false);
}

struct Walk<'v, F> {
    visit: &'v mut F,
    surfaces: usize,
    portals: usize,
}

impl<'v, 'a, F: FnMut(SceneItem<'a>)> Walk<'v, F> {
    fn new(visit: &'v mut F) -> Self {
        Self {
            visit,
            surfaces: 0,
            portals: 0,
        }
    }

    /// The surface children of `scene_layer`, in the space of `portal`,
    /// `level` portals deep, placed through `to_scene`; the scene's own
    /// surfaces are visited when `direct` holds.
    fn space(
        &mut self,
        scene_layer: &'a LayerNode,
        (portal, level): (Option<usize>, u8),
        to_scene: Option<&Mat4>,
        direct: bool,
    ) {
        for child in &scene_layer.children {
            let RenderNode::Layer(child) = child else {
                continue;
            };
            let Some((surface_layer, surface)) = layer_and_surface(child) else {
                continue;
            };
            let placed = match to_scene {
                Some(to_scene) => surface.placed_by(to_scene),
                None => surface.clone(),
            };
            self.surface(surface_layer, placed, (portal, level), direct);
        }
    }

    /// Visits a surface and then what its content adds to the scene.
    fn surface(
        &mut self,
        layer: &'a LayerNode,
        surface: MeshSurface,
        (portal, level): (Option<usize>, u8),
        visited: bool,
    ) {
        let host = self.surfaces;
        self.surfaces += 1;
        let inner = content_space(layer, &surface);
        if visited {
            (self.visit)(SceneItem::Surface {
                layer,
                surface,
                portal,
            });
        }
        if let Some(to_scene) = inner {
            let content = ProjectiveTransform::identity();
            self.content(layer, (&to_scene, content), host, (portal, level));
        }
    }

    /// The layers under `layer`, which `to_host` maps into the content of
    /// surface `host`, whose content `to_scene` places in the scene.
    fn content(
        &mut self,
        layer: &'a LayerNode,
        (to_scene, to_host): (&Mat4, ProjectiveTransform),
        host: usize,
        (portal, level): (Option<usize>, u8),
    ) {
        for child in &layer.children {
            let RenderNode::Layer(child) = child else {
                continue;
            };
            if !child.has_surfaces() {
                continue;
            }
            let child_to_host = child.transform_to_parent.then(to_host);
            if layer_scene(&child.graphics_layer).is_some() {
                self.portal(child, (to_scene, child_to_host), host, (portal, level));
            } else if let Some((surface_layer, surface)) = layer_and_surface(child) {
                let placed = surface.placed_by(to_scene);
                self.surface(surface_layer, placed, (portal, level), true);
            } else {
                self.content(child, (to_scene, child_to_host), host, (portal, level));
            }
        }
    }

    /// A scene inside the content of surface `host`, which shows as a portal
    /// within the depth a scene shows portals to, and draws flat on the host
    /// past it.
    fn portal(
        &mut self,
        scene_layer: &'a LayerNode,
        (to_scene, to_host): (&Mat4, ProjectiveTransform),
        host: usize,
        (parent, level): (Option<usize>, u8),
    ) {
        let Some(scene) = layer_scene(&scene_layer.graphics_layer) else {
            return;
        };
        if level >= MAX_PORTAL_LEVEL {
            return;
        }
        let window = to_host.bounds_for_rect(scene_layer.node_rect());
        let behind = scene.camera.doorway(window).then(to_scene);
        let index = self.portals;
        self.portals += 1;
        (self.visit)(SceneItem::Portal(ScenePortal {
            index,
            parent,
            level: level + 1,
            scene_layer,
            host,
            window,
            window_to_scene: Mat4::scaling(Vec3::new(window.width, window.height, 1.0))
                .then(&Mat4::translation(Vec3::new(window.x, window.y, 0.0)))
                .then(to_scene),
            clip: beyond(to_scene, window),
            light: scene.light.map(|light| Light {
                direction: behind.transform_vector(light.direction),
                ..light
            }),
        }));
        self.space(scene_layer, (Some(index), level + 1), Some(&behind), true);
    }
}

/// The plane of the content `to_scene` places, through `window`, as the
/// side behind the content sees it: positive beyond it.
fn beyond(to_scene: &Mat4, window: Rect) -> [f32; 4] {
    let point = to_scene.transform_point(Vec3::new(window.x, window.y, 0.0));
    let normal = to_scene
        .transform_vector(Vec3::new(0.0, 0.0, 1.0))
        .normalized()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    [normal.x, normal.y, normal.z, -normal.dot(point)]
}

/// The space the content of `surface_layer`, whose surface is `surface`, is
/// placed in for what it adds to the scene; `None` when its content holds no
/// surface.
fn content_space(surface_layer: &LayerNode, surface: &MeshSurface) -> Option<Mat4> {
    let holds_surfaces = surface_layer
        .children
        .iter()
        .any(|child| matches!(child, RenderNode::Layer(child) if child.has_surfaces()));
    if !holds_surfaces {
        return None;
    }
    surface.content_to_scene(node_size(surface_layer))
}

fn layer_and_surface(child: &LayerNode) -> Option<(&LayerNode, &MeshSurface)> {
    let layer = surface_layer(child)?;
    layer_surface(&layer.graphics_layer).map(|surface| (layer, surface))
}

pub(crate) fn node_size(layer: &LayerNode) -> Size {
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
