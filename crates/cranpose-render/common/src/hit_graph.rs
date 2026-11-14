use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{Rect, layer_transform::layer_surface};
use smallvec::SmallVec;

use crate::{
    graph::{HitTestNode, ProjectiveTransform, quad_bounds},
    graph_scene::{
        HitClip, HitGeometry, HitTargetSpec, HitTargets, Scene, SurfaceGeometry, SurfaceTarget,
    },
    primitive_emit::resolve_clip,
    scene_pick::ScenePick,
    scene_store::LayerRef,
};

/// Receives the hit targets that `collect_hits_from_graph` finds in a scene.
pub trait HitGraphSink {
    /// Records an interactive node. `capture_path` lists the node first and then its pointer-input
    /// ancestors from the nearest outward, and `geometry` is in window space.
    fn push_hit(
        &mut self,
        node_id: NodeId,
        capture_path: &[NodeId],
        geometry: HitGeometry<'_>,
        hit: &HitTestNode,
    );
}

impl HitGraphSink for Scene {
    fn push_hit(
        &mut self,
        node_id: NodeId,
        capture_path: &[NodeId],
        geometry: HitGeometry<'_>,
        hit: &HitTestNode,
    ) {
        Self::push_hit(self, node_id, capture_path, geometry, hit_spec(hit));
    }
}

impl HitGraphSink for HitTargets<'_> {
    fn push_hit(
        &mut self,
        node_id: NodeId,
        capture_path: &[NodeId],
        geometry: HitGeometry<'_>,
        hit: &HitTestNode,
    ) {
        self.push(node_id, capture_path, geometry, hit_spec(hit));
    }
}

fn hit_spec(hit: &HitTestNode) -> HitTargetSpec<'_> {
    HitTargetSpec {
        shape: None,
        handlers: &hit.handlers,
    }
}

/// Walks `layer` and the layers beneath it in draw order and pushes to `sink` a hit target for
/// each node that has a hit test, with its geometry in window space. `parent_transform` maps
/// the parent layer's space to the window, and `parent_hit_clip` is the clip bounds that the
/// layers above impose.
pub fn collect_hits_from_graph<S: HitGraphSink>(
    layer: LayerRef<'_>,
    parent_transform: ProjectiveTransform,
    sink: &mut S,
    parent_hit_clip: Option<Rect>,
) {
    if !layer.has_hit_targets() {
        return;
    }
    let mut walk = HitWalk {
        sink,
        hit_clips: Vec::new(),
        pointer_input_ancestors: Vec::new(),
        capture_path: SmallVec::new(),
        surface: None,
    };
    walk.visit(
        layer,
        parent_transform,
        parent_hit_clip,
        &ParentSpace::default(),
    );
}

struct HitWalk<'s, S> {
    sink: &'s mut S,
    hit_clips: Vec<HitClip>,
    pointer_input_ancestors: Vec<NodeId>,
    capture_path: SmallVec<[NodeId; 8]>,
    surface: Option<SurfaceWalk>,
}

struct SurfaceWalk {
    target: Rc<SurfaceTarget>,
    clips: Vec<HitClip>,
}

#[derive(Default)]
struct ParentSpace {
    scene: Option<Rc<ScenePick>>,
    scene_clips: (Option<Rect>, usize),
    content: Option<ProjectiveTransform>,
    content_clip: Option<Rect>,
    behind_doorway: bool,
    unseen: bool,
}

impl<S: HitGraphSink> HitWalk<'_, S> {
    fn visit(
        &mut self,
        layer: LayerRef<'_>,
        parent_transform: ProjectiveTransform,
        parent_hit_clip: Option<Rect>,
        space: &ParentSpace,
    ) {
        if !layer.has_hit_targets() {
            return;
        }
        // Behind a doorway the eye does not see through, nothing takes input.
        let Some((doorway, unseen)) = Self::seen(layer, space) else {
            return;
        };
        let transform = layer.transform_to_parent().then(parent_transform);
        // The node's handlers sit on its own rect; its clip, at its layer's
        // bounds, cuts them as it cuts what the layer holds.
        let node_rect = layer.node_rect();
        let transformed_quad = hit_quad(&transform, node_rect);
        let transformed_rect = quad_bounds(transformed_quad);

        // Inside a scene the camera's pick decides where the surfaces'
        // content takes input, whatever their flat stand-ins show: an eye
        // behind a wall still sees what lies through its doorway.
        let in_scene = space.scene.is_some();
        if !in_scene && (transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0) {
            return;
        }
        let world_to_local = match transform.inverse() {
            Some(inverse) => inverse,
            None if in_scene => ProjectiveTransform::identity(),
            None => return,
        };

        let (entered, content, content_clip) = self.enter_content(layer, (space, doorway));
        let behind_doorway = space.behind_doorway || doorway;
        let Some((hit_clip_bounds, pushed_clip)) =
            self.push_window_clip(layer, &transform, (parent_hit_clip, in_scene))
        else {
            self.leave(entered, content_clip.pushed);
            return;
        };

        if let (Some(node_id), Some(hit), false) = (
            layer.node_id(),
            layer.hit_test(),
            self.hides_targets(unseen, behind_doorway),
        ) {
            let geometry = PlacedNode {
                rect: transformed_rect,
                quad: transformed_quad,
                local_bounds: node_rect,
                world_to_local,
                hit_clip_bounds,
                content,
                content_clip: content_clip.bounds,
            };
            self.push_node(node_id, hit, geometry);
        }

        let pointer_input_ancestor = layer
            .hit_test()
            .filter(|hit| !hit.handlers.pointer_inputs().is_empty())
            .and_then(|_| layer.node_id());
        if let Some(node_id) = pointer_input_ancestor {
            self.pointer_input_ancestors.push(node_id);
        }

        let children_space = ParentSpace {
            content,
            content_clip: content_clip.bounds,
            behind_doorway,
            unseen,
            ..self.children_scene(layer, (transform, hit_clip_bounds), space)
        };
        for child_layer in layer.children() {
            self.visit(child_layer, transform, hit_clip_bounds, &children_space);
        }

        if pointer_input_ancestor.is_some() {
            let _ = self.pointer_input_ancestors.pop();
        }

        if pushed_clip {
            let _ = self.hit_clips.pop();
        }
        self.leave(entered, content_clip.pushed);
    }

    fn push_node(&mut self, node_id: NodeId, hit: &HitTestNode, node: PlacedNode) {
        self.capture_path.clear();
        self.capture_path.push(node_id);
        self.capture_path
            .extend(self.pointer_input_ancestors.iter().rev().copied());
        let content_to_local = node.content.and_then(ProjectiveTransform::inverse);
        let surface =
            self.surface
                .as_ref()
                .zip(content_to_local)
                .map(|(surface, content_to_local)| SurfaceGeometry {
                    target: &surface.target,
                    content_to_local,
                    clip_bounds: node.content_clip,
                    clips: &surface.clips,
                });
        self.sink.push_hit(
            node_id,
            &self.capture_path,
            HitGeometry {
                rect: node.rect,
                quad: node.quad,
                local_bounds: node.local_bounds,
                world_to_local: node.world_to_local,
                hit_clip_bounds: node.hit_clip_bounds,
                hit_clips: &self.hit_clips,
                surface,
            },
            hit,
        );
    }

    fn children_scene(
        &self,
        layer: LayerRef<'_>,
        (transform, hit_clip_bounds): (ProjectiveTransform, Option<Rect>),
        space: &ParentSpace,
    ) -> ParentSpace {
        let through_portal = space
            .scene
            .as_ref()
            .is_some_and(|scene| scene.holds_scene(layer));
        let shown = match (layer.wraps(), through_portal) {
            (None, false) => ScenePick::of(layer, transform).map(Rc::new),
            _ => None,
        };
        let (scene, scene_clips) = match shown {
            Some(pick) => (Some(pick), (hit_clip_bounds, self.hit_clips.len())),
            None => (space.scene.clone(), space.scene_clips),
        };
        ParentSpace {
            scene,
            scene_clips,
            ..ParentSpace::default()
        }
    }

    fn enter_surface(
        &mut self,
        layer: LayerRef<'_>,
        space: &ParentSpace,
    ) -> Option<Option<SurfaceWalk>> {
        let pick = space.scene.as_ref()?;
        let index = pick.index_of(layer)?;
        let (outer_clip_bounds, outer_clip_count) = space.scene_clips;
        let target = Rc::new(SurfaceTarget {
            pick: Rc::clone(pick),
            index,
            outer_clip_bounds,
            outer_clips: self.hit_clips[..outer_clip_count.min(self.hit_clips.len())].to_vec(),
        });
        Some(self.surface.replace(SurfaceWalk {
            target,
            clips: Vec::new(),
        }))
    }

    fn doorway(layer: LayerRef<'_>, space: &ParentSpace) -> Option<bool> {
        let Some(pick) = space.scene.as_ref().filter(|pick| pick.holds_scene(layer)) else {
            return Some(false);
        };
        pick.sees_scene(layer).then_some(true)
    }

    fn seen(layer: LayerRef<'_>, space: &ParentSpace) -> Option<(bool, bool)> {
        let doorway = Self::doorway(layer, space)?;
        let unseen = !doorway && (space.unseen || Self::unseen_surface(layer, space));
        Some((doorway, unseen))
    }

    fn hides_targets(&self, unseen: bool, behind_doorway: bool) -> bool {
        unseen || (behind_doorway && self.surface.is_none())
    }

    fn unseen_surface(layer: LayerRef<'_>, space: &ParentSpace) -> bool {
        space.scene.as_ref().is_some_and(|pick| {
            layer_surface(layer.graphics_layer()).is_some() && pick.index_of(layer).is_none()
        })
    }

    fn enter_content(
        &mut self,
        layer: LayerRef<'_>,
        (space, doorway): (&ParentSpace, bool),
    ) -> (
        Option<Option<SurfaceWalk>>,
        Option<ProjectiveTransform>,
        ContentClip,
    ) {
        if doorway {
            let unclipped = ContentClip {
                bounds: None,
                pushed: false,
            };
            return (Some(self.surface.take()), None, unclipped);
        }
        let entered = self.enter_surface(layer, space);
        // A surface's content starts a space of its own, which the clips of
        // the content above do not cut.
        let (content, above) = match entered {
            Some(_) => (Some(ProjectiveTransform::identity()), None),
            None => (
                space
                    .content
                    .map(|parent| layer.transform_to_parent().then(parent)),
                space.content_clip,
            ),
        };
        let content_clip = self.push_content_clip(layer, content, above);
        (entered, content, content_clip)
    }

    fn push_window_clip(
        &mut self,
        layer: LayerRef<'_>,
        transform: &ProjectiveTransform,
        (parent_hit_clip, in_scene): (Option<Rect>, bool),
    ) -> Option<(Option<Rect>, bool)> {
        let Some(local_clip) = layer.clip_rect().filter(|_| !in_scene) else {
            return Some((parent_hit_clip, false));
        };
        let quad = hit_quad(transform, local_clip);
        let bounds = quad_bounds(quad);
        let resolved = resolve_clip(parent_hit_clip, Some(bounds));
        if resolved.is_some_and(|clip| clip.is_empty()) {
            return None;
        }
        self.hit_clips.push(HitClip { quad, bounds });
        Some((resolved, true))
    }

    fn push_content_clip(
        &mut self,
        layer: LayerRef<'_>,
        content: Option<ProjectiveTransform>,
        parent_clip: Option<Rect>,
    ) -> ContentClip {
        let unclipped = ContentClip {
            bounds: parent_clip,
            pushed: false,
        };
        let (Some(surface), Some(content)) = (self.surface.as_mut(), content) else {
            return unclipped;
        };
        let Some(local_clip) = layer.clip_rect() else {
            return unclipped;
        };
        let quad = hit_quad(&content, local_clip);
        let bounds = quad_bounds(quad);
        surface.clips.push(HitClip { quad, bounds });
        ContentClip {
            bounds: resolve_clip(parent_clip, Some(bounds)),
            pushed: true,
        }
    }

    fn leave(&mut self, entered: Option<Option<SurfaceWalk>>, pushed_content_clip: bool) {
        if pushed_content_clip && let Some(surface) = self.surface.as_mut() {
            let _ = surface.clips.pop();
        }
        if let Some(previous) = entered {
            self.surface = previous;
        }
    }
}

struct PlacedNode {
    rect: Rect,
    quad: [[f32; 2]; 4],
    local_bounds: Rect,
    world_to_local: ProjectiveTransform,
    hit_clip_bounds: Option<Rect>,
    content: Option<ProjectiveTransform>,
    content_clip: Option<Rect>,
}

struct ContentClip {
    bounds: Option<Rect>,
    pushed: bool,
}

fn hit_quad(transform: &ProjectiveTransform, rect: Rect) -> [[f32; 2]; 4] {
    if transform.rect_in_front(rect) {
        return transform.map_rect(rect);
    }
    let bounds = transform.bounds_for_rect(rect);
    let (right, bottom) = (bounds.x + bounds.width, bounds.y + bounds.height);
    [
        [bounds.x, bounds.y],
        [right, bounds.y],
        [bounds.x, bottom],
        [right, bottom],
    ]
}

#[cfg(test)]
#[path = "tests/hit_graph_tests.rs"]
mod tests;
