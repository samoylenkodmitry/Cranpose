//! The surfaces a scene draws and picks, as one walk: its surface
//! children, the surfaces that join it from inside their content, and the
//! spaces its portals show, the scenes inside its surfaces' content, with
//! their surfaces placed behind their windows.

use cranpose_core::NodeId;
use cranpose_ui_graphics::{
    Rect, Size,
    layer_transform::{layer_scene, layer_surface},
    space::{Light, MAX_PORTAL_LEVEL, Mat4, MeshSurface, Vec3},
};

use crate::graph::{LayerNode, ProjectiveTransform, RenderGraph, RenderNode};

/// How many doorways one step of an eye can take it through.
const MAX_CROSSINGS: usize = 4;

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
/// the scene from inside its content and the portals there, down to
/// `limit` portals deep (see [`portal_limit`]).
pub fn for_each_scene_item(
    scene_layer: &LayerNode,
    limit: u8,
    visit: &mut impl FnMut(SceneItem<'_>),
) {
    Walk::new(visit, limit).space(scene_layer, (None, 0), None, true);
}

/// Visits what [`for_each_scene_item`] does but the scene's surface
/// children themselves: what the surfaces' content adds to the scene.
pub fn for_each_joined_item(
    scene_layer: &LayerNode,
    limit: u8,
    visit: &mut impl FnMut(SceneItem<'_>),
) {
    Walk::new(visit, limit).space(scene_layer, (None, 0), None, false);
}

/// How many portals deep a scene shows the spaces inside its surfaces for a
/// viewer who walked through `walked` doorways: [`MAX_PORTAL_LEVEL`] past
/// the viewer's own space.
pub fn portal_limit(walked: usize) -> u8 {
    u8::try_from(walked)
        .unwrap_or(u8::MAX)
        .saturating_add(MAX_PORTAL_LEVEL)
}

struct Walk<'v, F> {
    visit: &'v mut F,
    surfaces: usize,
    portals: usize,
    limit: u8,
}

impl<'v, 'a, F: FnMut(SceneItem<'a>)> Walk<'v, F> {
    fn new(visit: &'v mut F, limit: u8) -> Self {
        Self {
            visit,
            surfaces: 0,
            portals: 0,
            limit,
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
        if level >= self.limit {
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

/// Where the eye of a scene that shows on the window stands among the
/// spaces of its portals.
#[derive(Clone, Debug, PartialEq)]
pub struct Viewer {
    /// The node of the scene.
    pub scene: NodeId,
    /// Where its eye stood at the last update, in the scene.
    pub eye: Vec3,
    /// The scenes of the doorways the eye walked through, from the scene's
    /// own space inward.
    pub path: Vec<NodeId>,
}

/// A portal's doorway, as an eye that walks through it meets it.
struct Doorway {
    node: Option<NodeId>,
    parent: Option<usize>,
    /// The doorway's plane in the scene, positive in the portal's space.
    plane: [f32; 4],
    /// Maps the scene onto the unit square the doorway spans.
    scene_to_door: Mat4,
}

impl Doorway {
    /// Whether the step from `from` to `to` passes through the doorway, into
    /// the portal's space when `inward`, out of it otherwise.
    fn crossed(&self, (from, to): (Vec3, Vec3), inward: bool) -> bool {
        let side = |point: Vec3| {
            self.plane[0] * point.x
                + self.plane[1] * point.y
                + self.plane[2] * point.z
                + self.plane[3]
        };
        let (before, after) = (side(from), side(to));
        let passes = if inward {
            before < 0.0 && after >= 0.0
        } else {
            before >= 0.0 && after < 0.0
        };
        if !passes {
            return false;
        }
        let at = from + (to - from) * (before / (before - after));
        let on_door = self.scene_to_door.transform_point(at);
        (0.0..=1.0).contains(&on_door.x) && (0.0..=1.0).contains(&on_door.y)
    }
}

/// The doorways of the scene `scene_layer` shows, within the depth a viewer
/// who walked through `walked` doorways sees.
fn doorways(scene_layer: &LayerNode, walked: usize) -> Vec<Doorway> {
    let mut doorways = Vec::new();
    for_each_scene_item(scene_layer, portal_limit(walked), &mut |item| {
        if let SceneItem::Portal(portal) = item {
            doorways.push(Doorway {
                node: portal.scene_layer.node_id,
                parent: portal.parent,
                plane: portal.clip,
                scene_to_door: portal.window_to_scene.inverse().unwrap_or(Mat4::IDENTITY),
            });
        }
    });
    doorways
}

/// The portal whose space a viewer who walked through the doorways of
/// `path`'s scenes stands in, by its number among `doorways`, and how many
/// of `path`'s doorways still lead there; `None` for the scene's own space.
fn space_of(doorways: &[Doorway], path: &[NodeId]) -> (Option<usize>, usize) {
    let mut space = None;
    for (walked, node) in path.iter().enumerate() {
        let next = doorways
            .iter()
            .position(|door| door.parent == space && door.node == Some(*node));
        match next {
            Some(next) => space = Some(next),
            None => return (space, walked),
        }
    }
    (space, path.len())
}

/// The portal whose space the viewer of the scene `scene_layer` shows stands
/// in, by its number in a walk of the scene's items within
/// [`portal_limit`] of `path`'s length: `None` for the scene's own.
pub fn viewer_space(scene_layer: &LayerNode, path: &[NodeId]) -> Option<usize> {
    viewer_doorways(scene_layer, path)
        .last()
        .map(|door| door.index)
}

/// A doorway a viewer walked through: its portal's number in a walk of the
/// scene's items within [`portal_limit`] of the viewer's path, and its plane,
/// positive in the portal's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkedDoorway {
    /// The portal's number.
    pub index: usize,
    /// The doorway's plane in the scene.
    pub plane: [f32; 4],
}

/// The doorways the viewer of the scene `scene_layer` shows walked through,
/// along `path`, from the scene's own space inward, as far as they still
/// lead.
pub fn viewer_doorways(scene_layer: &LayerNode, path: &[NodeId]) -> Vec<WalkedDoorway> {
    if path.is_empty() {
        return Vec::new();
    }
    let doorways = doorways(scene_layer, path.len());
    let mut walked = Vec::with_capacity(path.len());
    let mut space = None;
    for node in path {
        let Some(next) = doorways
            .iter()
            .position(|door| door.parent == space && door.node == Some(*node))
        else {
            break;
        };
        walked.push(WalkedDoorway {
            index: next,
            plane: doorways[next].plane,
        });
        space = Some(next);
    }
    walked
}

/// Follows the viewer of each scene that shows on the window through the
/// doorways its eye passed since the last update; true when one entered or
/// left a space. A scene that shows no longer loses its viewer.
pub fn follow_viewers(graph: &mut RenderGraph) -> bool {
    if !graph.root.has_surfaces() {
        graph.viewers.clear();
        return false;
    }
    let mut moved = false;
    let viewers = &mut graph.viewers;
    let mut shown = 0;
    for_each_shown_scene(&graph.root, &mut |scene_layer| {
        let (Some(scene), Some(camera)) = (
            scene_layer.node_id,
            layer_scene(&scene_layer.graphics_layer).map(|scene| scene.camera),
        ) else {
            return;
        };
        let eye = camera.eye();
        let index = match viewers.iter().position(|viewer| viewer.scene == scene) {
            Some(index) => index,
            None => {
                viewers.push(Viewer {
                    scene,
                    eye,
                    path: Vec::new(),
                });
                viewers.len() - 1
            }
        };
        viewers.swap(shown, index);
        moved |= follow(scene_layer, &mut viewers[shown], eye);
        shown += 1;
    });
    viewers.truncate(shown);
    moved
}

/// Moves `viewer`'s eye to `eye`, through the doorways the step passes.
fn follow(scene_layer: &LayerNode, viewer: &mut Viewer, eye: Vec3) -> bool {
    let step = (viewer.eye, eye);
    viewer.eye = eye;
    let doorways = doorways(scene_layer, viewer.path.len() + MAX_CROSSINGS);
    let (mut space, valid) = space_of(&doorways, &viewer.path);
    let mut moved = valid < viewer.path.len();
    viewer.path.truncate(valid);
    for _ in 0..MAX_CROSSINGS {
        let inward = doorways
            .iter()
            .position(|door| door.parent == space && door.crossed(step, true));
        if let Some(entered) = inward
            && let Some(node) = doorways[entered].node
        {
            viewer.path.push(node);
            space = Some(entered);
            moved = true;
            continue;
        }
        let own = space.and_then(|index| doorways.get(index));
        match own {
            Some(door) if door.crossed(step, false) => {
                space = door.parent;
                viewer.path.pop();
                moved = true;
            }
            _ => break,
        }
    }
    moved
}

/// Visits `layer`, or each layer under it, that shows a scene with surfaces
/// and lies in no surface's content: the scenes the window shows.
fn for_each_shown_scene<'a>(layer: &'a LayerNode, visit: &mut impl FnMut(&'a LayerNode)) {
    if layer_scene(&layer.graphics_layer).is_some() {
        visit(layer);
        return;
    }
    if layer_surface(&layer.graphics_layer).is_some() {
        return;
    }
    for child in &layer.children {
        if let RenderNode::Layer(child) = child
            && child.has_surfaces()
        {
            for_each_shown_scene(child, visit);
        }
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
