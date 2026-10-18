//! Where the scenes a window shows put their 3D layers, as one map that
//! drawing, picking, window geometry and the eye's walking all read.
//!
//! Inside a scene, a layer takes its 3D frame from its parent's: its place in
//! the parent's layout, turned into 3D on the parent's plane. A mesh surface
//! places its mesh in that frame and its children's frames in its content; a
//! 3D transform moves the frames beneath it; a scene opens a space of its
//! own, behind a doorway at its node's rect. The window's eye stands in one
//! of these spaces and sees the others through the doorways that show on
//! screen, as deep as they show.

use std::sync::Arc;

use cranpose_ui_graphics::{
    Rect, Size,
    space::{Camera, EyeWorld, LayerSpace, Light, Mat4, MeshSurface, Scene3d, Vec3},
};

use crate::graph::{LayerNode, ProjectiveTransform, RenderNode};

/// How far past a doorway's plane, against its distance from the eye's
/// space's origin, the cut lies for a space seen back through the doorway,
/// so the surface the doorway opens in, which lies on the plane, shows a
/// hole.
const BACK_THROUGH_DOORWAY: f32 = 1.0e-4;

/// The smallest area, in logical pixels, a doorway's opening on screen
/// takes for the eye to see through it.
const SMALLEST_OPENING: f32 = 0.25;

/// A space a scene opens.
pub struct Cell<'a> {
    /// The scene's layer.
    pub layer: &'a LayerNode,
    /// The scene.
    pub scene: &'a Scene3d,
    /// The space its doorway lies in; `None` for the window's scene.
    pub parent: Option<usize>,
    /// The surface of the parent space whose content holds the doorway, by
    /// its number among the tree's surfaces, and the doorway's rect in that
    /// content, when the doorway lies flat in it.
    pub hole: Option<(usize, Rect)>,
    /// Places the unit square on the doorway in the parent space, the space
    /// lying beyond it along its z; the identity for the window's scene.
    pub doorway: Mat4,
    /// Maps the space into its parent's.
    pub to_parent: Mat4,
}

/// A mesh surface placed in its space.
pub struct Placed<'a> {
    /// The surface's layer.
    pub layer: &'a LayerNode,
    /// The surface, its mesh placed in its space.
    pub surface: MeshSurface,
    /// Its space.
    pub cell: usize,
    /// The size of its content.
    pub size: Size,
}

/// The surface whose content a layer lies flat in, by its index, and the
/// map from the layer into that content.
type Host = (usize, ProjectiveTransform);

/// The spaces of a window's scene and the scenes behind its doorways, and
/// their mesh surfaces, in the order the composition holds them.
pub struct SpaceTree<'a> {
    /// The spaces; the first is the window scene's own.
    pub cells: Vec<Cell<'a>>,
    /// The mesh surfaces, in draw order.
    pub surfaces: Vec<Placed<'a>>,
}

/// Where the window's eye stands, with its camera, and what it sees from
/// there: the spaces it sees through doorways and their surfaces, all in
/// the coordinates of the space it stands in.
pub struct EyeView<'a> {
    /// The eye's camera.
    pub camera: Camera,
    /// The window scene node's size, which the camera fills.
    pub size: Size,
    /// The space the eye stands in, by its number in the tree.
    pub cell: usize,
    /// The spaces the eye sees: its own first, then each through the
    /// doorway it shows through, before the spaces seen through it.
    pub seen: Vec<SeenSpace>,
    /// The surfaces of the spaces it sees, in draw order.
    pub surfaces: Vec<SeenSurface<'a>>,
}

/// A space the eye sees.
pub struct SeenSpace {
    /// The space, by its number in the tree.
    pub cell: usize,
    /// The seen space whose doorway it shows through; `None` for the eye's
    /// own.
    pub through: Option<usize>,
    /// How many doorways deep it shows: 0 for the eye's own space. The
    /// stencil that marks where each doorway opens counts them in eight
    /// bits, so the eye sees no deeper than 255.
    pub depth: u8,
    /// Maps the space into the eye's.
    pub to_eye: Mat4,
    /// The space's light, in the eye's space.
    pub light: Option<Light>,
    /// The doorway it shows through; `None` for the eye's own space.
    pub door: Option<SeenDoor>,
}

/// A doorway as the eye sees through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeenDoor {
    /// Places the unit square on the doorway, in the eye's space.
    pub quad: Mat4,
    /// Which way along the quad's z points away from the eye: 1 or -1.
    pub away: f32,
    /// The plane past which the space behind the doorway shows, in the eye's
    /// space: a point `p` shows where `normal . p + offset >= 0`.
    pub clip: [f32; 4],
    /// The bounds of the doorway's opening on screen, within those of the
    /// doorways it shows through, in the window scene node's coordinates.
    pub opening: Rect,
}

/// A surface the eye sees.
pub struct SeenSurface<'a> {
    /// The surface's layer.
    pub layer: &'a LayerNode,
    /// The surface, its mesh placed in the eye's space.
    pub surface: MeshSurface,
    /// The size of its content.
    pub size: Size,
    /// The seen space it lies in, by its number among [`EyeView::seen`].
    pub seen: usize,
}

impl<'a> SpaceTree<'a> {
    /// The spaces and surfaces of the scene `scene_layer` shows; `None`
    /// when it shows none.
    pub fn of(scene_layer: &'a LayerNode) -> Option<Self> {
        let scene = scene_of(scene_layer)?;
        let mut tree = Self {
            cells: Vec::new(),
            surfaces: Vec::new(),
        };
        tree.open(scene_layer, scene, None);
        Some(tree)
    }

    /// Whether `layer` is one of the tree's scenes.
    pub fn holds_scene(&self, layer: &LayerNode) -> bool {
        layer.node_id.is_some()
            && self
                .cells
                .iter()
                .any(|cell| cell.layer.node_id == layer.node_id)
    }

    /// The space an eye that walked through the doorways of the scene nodes
    /// `path` stands in: as far along the path as its doorways still lead.
    pub fn cell_along(&self, path: &[usize]) -> usize {
        let mut cell = 0;
        for &node in path {
            let next = self
                .cells
                .iter()
                .position(|inner| inner.parent == Some(cell) && inner.layer.node_id == Some(node));
            match next {
                Some(next) => cell = next,
                None => break,
            }
        }
        cell
    }

    /// Maps space `cell` into the window scene's.
    pub fn to_root(&self, cell: usize) -> Mat4 {
        let mut transform = Mat4::IDENTITY;
        let mut at = Some(cell);
        while let Some(index) = at {
            let cell = &self.cells[index];
            transform = transform.then(&cell.to_parent);
            at = cell.parent;
        }
        transform
    }

    /// Opens the space the scene `layer` shows: `place` holds the space its
    /// doorway lies in, the frame of `layer` there, and the surface whose
    /// content holds the doorway with the map from `layer` into that
    /// content.
    fn open(
        &mut self,
        layer: &'a LayerNode,
        scene: &'a Scene3d,
        place: Option<(usize, Mat4, Option<Host>)>,
    ) {
        let index = self.cells.len();
        let cell = match place {
            None => Cell {
                layer,
                scene,
                parent: None,
                hole: None,
                doorway: Mat4::IDENTITY,
                to_parent: Mat4::IDENTITY,
            },
            Some((parent, frame, host)) => {
                let rect = layer.node_rect();
                Cell {
                    layer,
                    scene,
                    parent: Some(parent),
                    hole: host
                        .map(|(surface, to_content)| (surface, to_content.bounds_for_rect(rect))),
                    doorway: Mat4::scaling(Vec3::new(rect.width, rect.height, 1.0))
                        .then(&Mat4::translation(Vec3::new(rect.x, rect.y, 0.0)))
                        .then(&frame),
                    to_parent: scene.camera.doorway(rect).then(&frame),
                }
            }
        };
        self.cells.push(cell);
        self.children(layer, &Mat4::IDENTITY, (index, None));
    }

    /// Visits the children of `layer`, whose frame `frame` maps it into
    /// space `cell`; `host` holds the surface whose content `layer` lies flat
    /// in, with the map from `layer` into that content.
    fn children(
        &mut self,
        layer: &'a LayerNode,
        frame: &Mat4,
        (cell, host): (usize, Option<Host>),
    ) {
        for child in &layer.children {
            let RenderNode::Layer(child) = child else {
                continue;
            };
            if !child.has_space() {
                continue;
            }
            let child_frame = embed(child.transform_to_parent).then(frame);
            let child_host = host
                .map(|(surface, to_content)| (surface, child.transform_to_parent.then(to_content)));
            match child.graphics_layer.space.as_deref() {
                Some(LayerSpace::Scene(scene)) => {
                    self.open(child, scene, Some((cell, child_frame, child_host)));
                }
                Some(LayerSpace::Surface(surface)) => {
                    self.surface(child, surface, &child_frame, cell);
                }
                Some(LayerSpace::Transform(transform)) => {
                    self.children(child, &transform.then(&child_frame), (cell, None));
                }
                None => self.children(child, &child_frame, (cell, child_host)),
            }
        }
    }

    /// Places the mesh surface `layer` draws its content on in `frame`, in
    /// space `cell`, and the frames of its children in its content.
    fn surface(&mut self, layer: &'a LayerNode, surface: &MeshSurface, frame: &Mat4, cell: usize) {
        let size = node_size(layer);
        let placed = surface.placed_by(frame);
        let content = placed.content_to_scene(size);
        let index = self.surfaces.len();
        self.surfaces.push(Placed {
            layer,
            surface: placed,
            cell,
            size,
        });
        if let Some(content) = content {
            self.children(
                layer,
                &content,
                (cell, Some((index, ProjectiveTransform::identity()))),
            );
        }
    }
}

/// What the doorway between a space and the next one an eye looks into
/// opens onto.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Through {
    /// A space behind a doorway in the space the eye looks from.
    Inward,
    /// The space the doorway of the space the eye looks from lies in.
    Outward,
}

impl<'a> EyeView<'a> {
    /// What the window's eye sees of `tree`, the scene of a node of `size`:
    /// it stands where the window scene's [`Scene3d::eye`] says, and sees
    /// through each doorway whose opening shows on screen.
    pub fn of(tree: &SpaceTree<'a>, size: Size) -> Self {
        let root = tree.cells[0].scene;
        let cell = root
            .eye
            .as_ref()
            .map_or(0, |eye| tree.cell_along(&eye.doorways));
        let mut view = Self {
            camera: root.camera,
            size,
            cell,
            seen: vec![SeenSpace {
                cell,
                through: None,
                depth: 0,
                to_eye: Mat4::IDENTITY,
                light: tree.cells[cell].scene.light,
                door: None,
            }],
            surfaces: Vec::new(),
        };
        view.look_through(tree, (0, None), Rect::from_size(size));
        let mut seen_of = vec![None; tree.cells.len()];
        for (index, seen) in view.seen.iter().enumerate() {
            seen_of[seen.cell] = Some(index);
        }
        for placed in &tree.surfaces {
            let Some(seen) = seen_of[placed.cell] else {
                continue;
            };
            view.surfaces.push(SeenSurface {
                layer: placed.layer,
                surface: placed.surface.placed_by(&view.seen[seen].to_eye),
                size: placed.size,
                seen,
            });
        }
        view
    }

    /// Looks through the doorways of seen space `seen`, other than the one
    /// into the space `came_from`, within `opening`.
    fn look_through(
        &mut self,
        tree: &SpaceTree<'a>,
        (seen, came_from): (usize, Option<usize>),
        opening: Rect,
    ) {
        let (cell, to_eye) = (self.seen[seen].cell, self.seen[seen].to_eye);
        for (inner, space) in tree.cells.iter().enumerate() {
            if space.parent != Some(cell) || Some(inner) == came_from {
                continue;
            }
            let inner_to_eye = space.to_parent.then(&to_eye);
            let quad = space.doorway.then(&to_eye);
            self.through(
                tree,
                (seen, inner, inner_to_eye),
                (quad, Through::Inward),
                opening,
            );
        }
        let space = &tree.cells[cell];
        if let Some(outer) = space.parent.filter(|outer| Some(*outer) != came_from)
            && let Some(from_parent) = space.to_parent.inverse()
        {
            let outer_to_eye = from_parent.then(&to_eye);
            let quad = space.doorway.then(&outer_to_eye);
            self.through(
                tree,
                (seen, outer, outer_to_eye),
                (quad, Through::Outward),
                opening,
            );
        }
    }

    /// Sees space `cell`, which `to_eye` maps into the eye's, through the
    /// doorway `quad` spans, from seen space `seen`, within `opening`; then
    /// looks on through its doorways.
    fn through(
        &mut self,
        tree: &SpaceTree<'a>,
        (seen, cell, to_eye): (usize, usize, Mat4),
        (quad, through): (Mat4, Through),
        opening: Rect,
    ) {
        let Some(depth) = self.seen[seen].depth.checked_add(1) else {
            return;
        };
        let corner = quad.transform_point(Vec3::ZERO);
        let Some(normal) = quad.transform_vector(Vec3::new(0.0, 0.0, 1.0)).normalized() else {
            return;
        };
        let side = normal.dot(self.camera.eye() - corner);
        // A doorway opens one way: into its space from the side its z leaves,
        // and back out of it from the side its z points to.
        let (facing, away, clip) = match through {
            Through::Inward => (side < 0.0, 1.0, plane(normal, corner, 0.0)),
            Through::Outward => {
                let margin = BACK_THROUGH_DOORWAY * normal.dot(corner).abs().max(1.0);
                (side > 0.0, -1.0, plane(-normal, corner, margin))
            }
        };
        let shown = facing
            .then(|| self.opening(&quad))
            .flatten()
            .and_then(|shown| shown.intersect(opening))
            .filter(|shown| shown.width * shown.height >= SMALLEST_OPENING);
        let Some(shown) = shown else {
            return;
        };
        let light = tree.cells[cell].scene.light.map(|light| Light {
            direction: to_eye.transform_vector(light.direction),
            ..light
        });
        self.seen.push(SeenSpace {
            cell,
            through: Some(seen),
            depth,
            to_eye,
            light,
            door: Some(SeenDoor {
                quad,
                away,
                clip,
                opening: shown,
            }),
        });
        let index = self.seen.len() - 1;
        self.look_through(tree, (index, Some(self.seen[seen].cell)), shown);
    }

    /// The bounds on screen of the unit square `quad` places in the eye's
    /// space, of its part in front of the camera's near plane; `None` when
    /// it lies wholly behind.
    fn opening(&self, quad: &Mat4) -> Option<Rect> {
        let to_camera = quad.then(&self.camera.view);
        let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .map(|(u, v)| to_camera.transform_point(Vec3::new(u, v, 0.0)));
        let near = self.camera.near();
        let (mut low, mut high) = (
            (f32::INFINITY, f32::INFINITY),
            (f32::NEG_INFINITY, f32::NEG_INFINITY),
        );
        let mut any = false;
        let mut show = |point: Vec3| {
            let scale = self.camera.focal_length / point.z;
            let (x, y) = (
                self.size.width * 0.5 + point.x * scale,
                self.size.height * 0.5 + point.y * scale,
            );
            low = (low.0.min(x), low.1.min(y));
            high = (high.0.max(x), high.1.max(y));
            any = true;
        };
        for (index, &from) in corners.iter().enumerate() {
            let to = corners[(index + 1) % corners.len()];
            if from.z >= near {
                show(from);
            }
            if (from.z >= near) != (to.z >= near) {
                show(from + (to - from) * ((near - from.z) / (to.z - from.z)));
            }
        }
        any.then_some(Rect {
            x: low.0,
            y: low.1,
            width: high.0 - low.0,
            height: high.1 - low.1,
        })
    }

    /// Fills `world` with what the eye can bump into and walk through: the
    /// space it stands in, the spaces one doorway from it, their solid
    /// surfaces and the doorways between them, in the eye's space.
    pub fn build_world(&self, tree: &SpaceTree<'_>, world: &mut EyeWorld) {
        let walked = tree.walked_to(self.cell);
        let base = world.add_space(Arc::clone(&walked), Mat4::IDENTITY);
        let mut spaces = vec![(self.cell, base, Mat4::IDENTITY)];
        let own = &tree.cells[self.cell];
        if let Some(outer) = own.parent
            && let Some(from_parent) = own.to_parent.inverse()
        {
            let path: Arc<[usize]> = Arc::from(&walked[..walked.len().saturating_sub(1)]);
            let index = world.add_space(path, from_parent);
            world.add_doorway((index, base), own.doorway.then(&from_parent));
            spaces.push((outer, index, from_parent));
        }
        for (inner, space) in tree.cells.iter().enumerate() {
            let (Some(cell), Some(node)) = (space.parent, space.layer.node_id) else {
                continue;
            };
            if cell != self.cell {
                continue;
            }
            let path: Arc<[usize]> = walked.iter().copied().chain([node]).collect();
            let index = world.add_space(path, space.to_parent);
            world.add_doorway((base, index), space.doorway);
            spaces.push((inner, index, space.to_parent));
        }
        for (surface_index, placed) in tree.surfaces.iter().enumerate() {
            let Some(&(_, space, to_base)) =
                spaces.iter().find(|(cell, _, _)| *cell == placed.cell)
            else {
                continue;
            };
            if !placed.surface.solid {
                continue;
            }
            let holes = tree.cells.iter().filter_map(|cell| {
                cell.hole
                    .filter(|(host, _)| *host == surface_index)
                    .map(|(_, rect)| rect)
            });
            world.add_solid(
                (space, Arc::clone(&placed.surface.mesh)),
                placed.surface.transform.then(&to_base),
                (placed.size, holes),
            );
        }
    }
}

impl SpaceTree<'_> {
    /// The scene nodes of the doorways from the window's scene to space
    /// `cell`, outermost first.
    fn walked_to(&self, cell: usize) -> Arc<[usize]> {
        let mut path = Vec::new();
        let mut at = cell;
        while let Some(parent) = self.cells[at].parent {
            if let Some(node) = self.cells[at].layer.node_id {
                path.push(node);
            }
            at = parent;
        }
        path.reverse();
        Arc::from(path)
    }
}

/// The plane through `point` square to the unit `normal`, moved `margin`
/// back along it: positive where `normal` points.
fn plane(normal: Vec3, point: Vec3, margin: f32) -> [f32; 4] {
    [normal.x, normal.y, normal.z, -normal.dot(point) - margin]
}

/// A plane map of the frame's plane z = 0 as a 3D map: z stays and
/// distances along it scale as the plane's do not.
pub fn embed(transform: ProjectiveTransform) -> Mat4 {
    let [[a, b, c], [d, e, f], [g, h, i]] = transform.matrix();
    Mat4::from_rows([
        [a, b, 0.0, c],
        [d, e, 0.0, f],
        [0.0, 0.0, 1.0, 0.0],
        [g, h, 0.0, i],
    ])
}

/// The scene a layer shows, or `None`.
pub fn scene_of(layer: &LayerNode) -> Option<&Scene3d> {
    match layer.graphics_layer.space.as_deref()? {
        LayerSpace::Scene(scene) => Some(scene),
        LayerSpace::Surface(_) | LayerSpace::Transform(_) => None,
    }
}

/// The node size of a layer.
pub fn node_size(layer: &LayerNode) -> Size {
    let rect = layer.node_rect();
    Size {
        width: rect.width,
        height: rect.height,
    }
}

/// Visits the space tree of each scene the window shows from `layer` down,
/// what its eye sees of it, and the map from the scene node's own
/// coordinates, its top left at the origin, to the window, given
/// `to_window` for `layer`.
pub fn for_each_window_eye(
    layer: &LayerNode,
    to_window: ProjectiveTransform,
    visit: &mut impl FnMut(&SpaceTree<'_>, &EyeView<'_>, ProjectiveTransform),
) {
    for_each_window_scene(layer, to_window, &mut |scene_layer, to_window| {
        let Some(tree) = SpaceTree::of(scene_layer) else {
            return;
        };
        let view = EyeView::of(&tree, node_size(scene_layer));
        let rect = scene_layer.node_rect();
        visit(
            &tree,
            &view,
            ProjectiveTransform::translation(rect.x, rect.y).then(to_window),
        );
    });
}

impl EyeView<'_> {
    /// The map from the content of seen surface `seen` to the window, as the
    /// eye shows it, given `to_window` for the scene node's own coordinates;
    /// `None` for a surface whose mesh carries no texture plane.
    pub fn content_to_window(
        &self,
        seen: &SeenSurface<'_>,
        to_window: ProjectiveTransform,
    ) -> Option<ProjectiveTransform> {
        seen.surface
            .content_transform(&self.camera, self.size, seen.size)
            .map(|content| content.then(to_window))
    }
}

/// Visits each scene the window shows from `layer` down: a layer that
/// shows a scene outside every other scene's space, with the map from its
/// local space into the window, given `to_window` for `layer`.
pub fn for_each_window_scene<'a>(
    layer: &'a LayerNode,
    to_window: ProjectiveTransform,
    visit: &mut impl FnMut(&'a LayerNode, ProjectiveTransform),
) {
    if scene_of(layer).is_some() {
        visit(layer, to_window);
        return;
    }
    for child in &layer.children {
        if let RenderNode::Layer(child) = child
            && child.has_space()
        {
            for_each_window_scene(child, child.transform_to_parent.then(to_window), visit);
        }
    }
}
