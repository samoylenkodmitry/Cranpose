use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_ui_graphics::{
    Point, Rect, Size,
    space::{Camera, Mat4, MeshSurface, Ray, SurfaceHit, Vec3},
};

use crate::{
    graph::ProjectiveTransform,
    scene_store::LayerRef,
    space::{EyeView, SpaceTree},
};

const PAST_DOORWAY: f32 = 1.0e-4;

/// Finds which mesh surface of a scene a window point, or any ray, shows,
/// and the point of the surface's content there: the nearest surface along
/// the ray in the space the window's eye stands in, or behind a doorway the
/// ray meets first, in the spaces the eye sees. A layer drawn on several
/// meshes is found by its owner's index, through whichever of its meshes
/// is nearest.
pub struct ScenePick {
    scene: Option<NodeId>,
    camera: Camera,
    size: Size,
    window_to_scene: ProjectiveTransform,
    scene_to_window: ProjectiveTransform,
    surfaces: Vec<PickSurface>,
    seen: Vec<PickSpace>,
    scenes: Vec<NodeId>,
    seen_scenes: Vec<NodeId>,
    last: Cell<Option<LastPick>>,
    holds: SurfaceHolds,
}

/// The mesh and triangle the gesture on each surface layer holds, kept
/// beside a window's hit targets: the targets are collected again whenever
/// a surface moves, and a drag stays on the mesh it started on through that.
/// A clone is a handle to the same holds.
#[derive(Clone, Default)]
pub struct SurfaceHolds(Rc<Holds>);

#[derive(Default)]
struct Holds {
    holds: RefCell<Vec<Hold>>,
    collection: Cell<u32>,
}

#[derive(Clone, Copy)]
struct Hold {
    layer: NodeId,
    mesh: usize,
    triangle: u32,
    // The last collection of the hit targets that found the layer.
    collection: u32,
}

impl SurfaceHolds {
    /// Starts a new collection of the hit targets: the holds on layers the
    /// last one did not find go.
    pub fn collect_again(&self) {
        let last = self.0.collection.get();
        self.0
            .holds
            .borrow_mut()
            .retain(|hold| hold.collection == last);
        self.0.collection.set(last.wrapping_add(1));
    }

    // The collection finds `layer`: its hold stays.
    fn keep(&self, layer: NodeId) {
        let now = self.0.collection.get();
        if let Some(hold) = self
            .0
            .holds
            .borrow_mut()
            .iter_mut()
            .find(|hold| hold.layer == layer)
        {
            hold.collection = now;
        }
    }

    fn of(&self, layer: NodeId) -> Option<Hold> {
        self.0
            .holds
            .borrow()
            .iter()
            .find(|hold| hold.layer == layer)
            .copied()
    }

    fn hold(&self, layer: NodeId, mesh: usize, triangle: u32) {
        let hold = Hold {
            layer,
            mesh,
            triangle,
            collection: self.0.collection.get(),
        };
        let mut holds = self.0.holds.borrow_mut();
        match holds.iter_mut().find(|held| held.layer == layer) {
            Some(held) => *held = hold,
            None => holds.push(hold),
        }
    }
}

struct PickSpace {
    through: Option<usize>,
    to_door: Mat4,
}

impl PickSpace {
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

type LastPick = (Point, Option<(usize, SurfaceHit)>);

struct PickSurface {
    node_id: Option<NodeId>,
    surface: MeshSurface,
    // Which of its layer's meshes it is; the owner, 0, comes first.
    mesh: usize,
    // The rect of its content in its layer, which the mesh's texture
    // spans. The pick's content points lie in the layer.
    content: Rect,
    seen: usize,
}

impl PickSurface {
    // The content point `point`, from the corner of the texture, in the
    // surface's layer.
    fn in_layer(&self, point: Point) -> Point {
        Point::new(point.x + self.content.x, point.y + self.content.y)
    }

    // Where `ray` meets the surface, its content point in the layer.
    fn cast(&self, ray: Ray) -> Option<SurfaceHit> {
        let mut hit = self.surface.cast(ray, self.content.size())?;
        hit.point = self.in_layer(hit.point);
        Some(hit)
    }
}

impl ScenePick {
    /// The pick of the scene that `layer` shows, with `to_window` mapping
    /// the layer to the window; `None` when the layer shows no scene. Its
    /// surfaces are those the window's eye sees. The gestures on them keep
    /// the meshes they hold in `holds`, or in the pick's own store when
    /// `holds` is `None`.
    pub fn of(
        layer: LayerRef<'_>,
        to_window: ProjectiveTransform,
        holds: Option<&SurfaceHolds>,
    ) -> Option<Self> {
        // A hit walk asks every layer: one that opens no scene stops here,
        // before any transform work.
        let tree = SpaceTree::of(layer)?;
        let holds = holds.cloned().unwrap_or_default();
        // The scene fills its layer's bounds.
        let bounds = layer.local_bounds();
        let to_window = ProjectiveTransform::translation(bounds.x, bounds.y).then(to_window);
        let window_to_scene = to_window.inverse()?;
        let view = EyeView::of(&tree, bounds.size());
        let seen_scenes = view
            .seen
            .iter()
            .filter_map(|seen| tree.cells[seen.cell].layer.node_id())
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
        let surfaces = view
            .surfaces
            .into_iter()
            .map(|seen| {
                let node_id = seen.layer.node_id();
                if let Some(layer) = node_id.filter(|_| seen.mesh == 0) {
                    holds.keep(layer);
                }
                PickSurface {
                    node_id,
                    surface: seen.surface,
                    mesh: seen.mesh,
                    content: seen.content,
                    seen: seen.seen,
                }
            })
            .collect();
        Some(Self {
            scene: layer.node_id(),
            camera: view.camera,
            size: view.size,
            window_to_scene,
            scene_to_window: to_window,
            surfaces,
            seen,
            scenes: tree
                .cells
                .iter()
                .filter_map(|cell| cell.layer.node_id())
                .collect(),
            seen_scenes,
            last: Cell::new(None),
            holds,
        })
    }

    /// The node of the scene the window shows.
    pub fn scene(&self) -> Option<NodeId> {
        self.scene
    }

    /// Whether `layer` shows one of this scene's spaces, a scene behind one
    /// of its doorways, whose surfaces this pick finds.
    pub fn holds_scene(&self, layer: LayerRef<'_>) -> bool {
        layer
            .node_id()
            .is_some_and(|node_id| self.scenes.contains(&node_id))
    }

    /// Whether the eye sees the space of the scene `layer` shows, through
    /// the doorways before it.
    pub fn sees_scene(&self, layer: LayerRef<'_>) -> bool {
        layer
            .node_id()
            .is_some_and(|node_id| self.seen_scenes.contains(&node_id))
    }

    /// The index among the scene's surfaces of the surface layer `layer`:
    /// of its owner mesh, the first of its meshes.
    pub fn index_of(&self, layer: LayerRef<'_>) -> Option<usize> {
        let node_id = layer.node_id()?;
        self.surfaces
            .iter()
            .position(|surface| surface.node_id == Some(node_id))
    }

    /// The content point of the layer of owner `index` that the window
    /// point shows, when one of its meshes is the nearest surface there.
    pub fn content_point(&self, window: Point, index: usize) -> Option<Point> {
        let (nearest, hit) = self.nearest(window)?;
        (self.owner(nearest) == index).then_some(hit.point)
    }

    /// [`Self::content_point`] for a ray in the space the window's eye
    /// stands in.
    pub fn content_point_along(&self, ray: Ray, index: usize) -> Option<Point> {
        let (nearest, hit) = self.nearest_along(ray)?;
        (self.owner(nearest) == index).then_some(hit.point)
    }

    /// The content point of the layer of owner `index` under the window
    /// point for a gesture the layer holds: where the ray meets the mesh the
    /// gesture started on, or past its edge, where the ray meets the plane
    /// of the triangle it last met there.
    pub fn drag_point(&self, window: Point, index: usize) -> Option<Point> {
        self.drag_point_along(self.ray(window)?, index)
    }

    /// [`Self::drag_point`] for a ray in the space the window's eye stands
    /// in.
    pub fn drag_point_along(&self, ray: Ray, index: usize) -> Option<Point> {
        let layer = self.surfaces.get(index)?.node_id;
        let Some(held) = layer.and_then(|layer| self.holds.of(layer)) else {
            let (target, hit) = self.nearest_mesh(ray, index)?;
            self.hold(target, hit.triangle);
            return Some(hit.point);
        };
        let target = index + held.mesh;
        let surface = self
            .surfaces
            .get(target)
            .filter(|surface| surface.mesh == held.mesh)?;
        if let Some(hit) = surface.cast(ray) {
            self.hold(target, hit.triangle);
            return Some(hit.point);
        }
        surface
            .surface
            .cast_plane(ray, held.triangle, surface.content.size())
            .map(|point| surface.in_layer(point))
    }

    /// Where the content point `content` of the layer of owner `index`
    /// shows in the window, on its owner mesh, also for a surface behind a
    /// doorway; `None` when it lies nearer than the camera's near plane or
    /// behind the eye.
    pub fn window_point(&self, index: usize, content: Point) -> Option<Point> {
        let target = self.surfaces.get(index)?;
        let from_corner = Point::new(content.x - target.content.x, content.y - target.content.y);
        let at = target
            .surface
            .point_at(from_corner, target.content.size())?;
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

    // The owner of the layer of surface `index`.
    fn owner(&self, index: usize) -> usize {
        index - self.surfaces[index].mesh
    }

    fn hold(&self, index: usize, triangle: u32) {
        let surface = &self.surfaces[index];
        if let Some(layer) = surface.node_id {
            self.holds.hold(layer, surface.mesh, triangle);
        }
    }

    // The mesh of the layer of owner `index` that `ray` meets first, by its
    // index, and where.
    fn nearest_mesh(&self, ray: Ray, index: usize) -> Option<(usize, SurfaceHit)> {
        self.surfaces[index..]
            .iter()
            .enumerate()
            .take_while(|(mesh, surface)| surface.mesh == *mesh)
            .filter_map(|(mesh, surface)| surface.cast(ray).map(|hit| (index + mesh, hit)))
            .min_by(|(_, a), (_, b)| a.distance.total_cmp(&b.distance))
    }

    fn nearest(&self, window: Point) -> Option<(usize, SurfaceHit)> {
        if let Some((point, nearest)) = self.last.get()
            && point == window
        {
            return nearest;
        }
        let nearest = self.ray(window).and_then(|ray| self.nearest_along(ray));
        self.last.set(Some((window, nearest)));
        nearest
    }

    // The surface `ray` meets first, by its index, and where; the gesture
    // on its layer holds it from there.
    fn nearest_along(&self, ray: Ray) -> Option<(usize, SurfaceHit)> {
        let nearest = self.nearest_in(ray, 0, f32::NEG_INFINITY);
        if let Some((index, hit)) = nearest {
            self.hold(index, hit.triangle);
        }
        nearest
    }

    fn nearest_in(&self, ray: Ray, seen: usize, beyond: f32) -> Option<(usize, SurfaceHit)> {
        let surface = self
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, target)| target.seen == seen)
            .filter_map(|(index, target)| {
                target
                    .cast(ray)
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
