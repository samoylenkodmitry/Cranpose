use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_ui_graphics::Rect;
use smallvec::SmallVec;

use crate::{
    graph::{HitTestNode, LayerNode, ProjectiveTransform, RenderNode, quad_bounds},
    graph_scene::{HitClip, HitGeometry, HitTargetSpec, Scene, SurfaceGeometry, SurfaceTarget},
    primitive_emit::resolve_clip,
    scene_pick::ScenePick,
};

/// Receives the hit targets that `collect_hits_from_graph` finds in a render graph.
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
        Scene::push_hit(
            self,
            node_id,
            capture_path,
            geometry,
            HitTargetSpec {
                shape: None,
                handlers: &hit.handlers,
            },
        );
    }
}

/// Walks `layer` and the layers beneath it in draw order and pushes to `sink` a hit target for
/// each node that has a hit test, with its geometry in window space. `parent_transform` maps
/// the parent layer's space to the window, and `parent_hit_clip` is the clip bounds that the
/// layers above impose.
pub fn collect_hits_from_graph<S: HitGraphSink>(
    layer: &LayerNode,
    parent_transform: ProjectiveTransform,
    sink: &mut S,
    parent_hit_clip: Option<Rect>,
) {
    if !layer.has_hit_targets {
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
    /// The mesh surface the walk is inside, and the clips cutting its
    /// content, in content space.
    surface: Option<SurfaceWalk>,
}

struct SurfaceWalk {
    target: Rc<SurfaceTarget>,
    clips: Vec<HitClip>,
}

/// What a layer's parent places it in: the scene above it, with the clip
/// bounds and the number of window clips that cut the scene's surfaces, and
/// inside a mesh surface, the map from the parent to the surface's content
/// with the content clip above. A surface that joins the scene from inside
/// another's content is cut only where the scene's surfaces are.
#[derive(Default)]
struct ParentSpace {
    scene: Option<Rc<ScenePick>>,
    scene_clips: (Option<Rect>, usize),
    content: Option<ProjectiveTransform>,
    content_clip: Option<Rect>,
}

impl<S: HitGraphSink> HitWalk<'_, S> {
    fn visit(
        &mut self,
        layer: &LayerNode,
        parent_transform: ProjectiveTransform,
        parent_hit_clip: Option<Rect>,
        space: &ParentSpace,
    ) {
        if !layer.has_hit_targets {
            return;
        }
        let transform = layer.transform_to_parent.then(parent_transform);
        // The node's handlers sit on its own rect; its clip, at its layer's
        // bounds, cuts them as it cuts what the layer holds.
        let node_rect = layer.node_rect();
        let transformed_quad = hit_quad(&transform, node_rect);
        let transformed_rect = quad_bounds(transformed_quad);

        if transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0 {
            return;
        }

        let Some(world_to_local) = transform.inverse() else {
            return;
        };

        let entered = self.enter_surface(layer, space);
        let content = match entered {
            Some(_) => Some(ProjectiveTransform::identity()),
            None => space
                .content
                .map(|parent| layer.transform_to_parent.then(parent)),
        };
        let content_clip = self.push_content_clip(layer, content, space.content_clip);

        let mut hit_clip_bounds = parent_hit_clip;
        let mut pushed_clip = false;
        if let Some(local_clip) = layer.clip_rect() {
            let clip_quad = hit_quad(&transform, local_clip);
            let clip_bounds = quad_bounds(clip_quad);
            let resolved_clip_bounds = resolve_clip(parent_hit_clip, Some(clip_bounds));
            if resolved_clip_bounds.is_some_and(|clip| clip.is_empty()) {
                self.leave(entered, content_clip.pushed);
                return;
            }
            hit_clip_bounds = resolved_clip_bounds;
            self.hit_clips.push(HitClip {
                quad: clip_quad,
                bounds: clip_bounds,
            });
            pushed_clip = true;
        }

        if let (Some(node_id), Some(hit)) = (layer.node_id, &layer.hit_test) {
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
            .hit_test
            .as_ref()
            .filter(|hit| !hit.handlers.pointer_inputs().is_empty())
            .and(layer.node_id);
        if let Some(node_id) = pointer_input_ancestor {
            self.pointer_input_ancestors.push(node_id);
        }

        let children_space = ParentSpace {
            content,
            content_clip: content_clip.bounds,
            ..self.children_scene(layer, (transform, hit_clip_bounds), space)
        };
        for child in &layer.children {
            if let RenderNode::Layer(child_layer) = child {
                self.visit(child_layer, transform, hit_clip_bounds, &children_space);
            }
        }

        if pointer_input_ancestor.is_some() {
            let _ = self.pointer_input_ancestors.pop();
        }

        if pushed_clip {
            let _ = self.hit_clips.pop();
        }
        self.leave(entered, content_clip.pushed);
    }

    /// Records a node's hit target with its ancestors' clips and, inside a
    /// surface, where it lies on the surface's content.
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

    /// The scene above `layer`'s children: the one `layer` shows, cut by the
    /// clips above them, or the one above `layer`; the content fields stay
    /// empty.
    fn children_scene(
        &self,
        layer: &LayerNode,
        (transform, hit_clip_bounds): (ProjectiveTransform, Option<Rect>),
        space: &ParentSpace,
    ) -> ParentSpace {
        let shown = match layer.wraps {
            Some(_) => None,
            None => ScenePick::of(layer, transform).map(Rc::new),
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

    /// Starts the surface `layer` is, when the scene above draws it, cut by
    /// the clips that cut the scene's surfaces; returns the surface walk it
    /// replaces.
    fn enter_surface(
        &mut self,
        layer: &LayerNode,
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

    /// Pushes `layer`'s clip in its surface's content space, when the walk
    /// is inside a surface.
    fn push_content_clip(
        &mut self,
        layer: &LayerNode,
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

/// Where a node's own rect lies: in the window, and inside a surface, on
/// the surface's content.
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
