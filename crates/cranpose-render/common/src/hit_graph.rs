use cranpose_core::NodeId;
use cranpose_ui_graphics::Rect;
use smallvec::SmallVec;

use crate::{
    graph::{HitTestNode, LayerNode, ProjectiveTransform, RenderNode, quad_bounds},
    graph_scene::{ClickAction, HitClip, HitGeometry, HitTargetSpec, Scene},
    primitive_emit::resolve_clip,
};

pub trait HitGraphSink {
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
                shape: hit.shape,
                click_actions: hit
                    .click_actions
                    .iter()
                    .cloned()
                    .map(ClickAction::WithPoint),
                pointer_inputs: &hit.pointer_inputs,
                pointer_icon: hit.pointer_icon.as_ref(),
            },
        );
    }
}

pub fn collect_hits_from_graph<S: HitGraphSink>(
    layer: &LayerNode,
    parent_transform: ProjectiveTransform,
    sink: &mut S,
    parent_hit_clip: Option<Rect>,
) {
    if !layer.has_hit_targets {
        return;
    }
    let mut hit_clips = Vec::new();
    let mut pointer_input_ancestors = Vec::new();
    let mut capture_path = SmallVec::<[NodeId; 8]>::new();
    collect_hits_from_graph_inner(
        layer,
        parent_transform,
        sink,
        parent_hit_clip,
        &mut hit_clips,
        &mut pointer_input_ancestors,
        &mut capture_path,
    );
}

fn collect_hits_from_graph_inner<S: HitGraphSink>(
    layer: &LayerNode,
    parent_transform: ProjectiveTransform,
    sink: &mut S,
    parent_hit_clip: Option<Rect>,
    hit_clips: &mut Vec<HitClip>,
    pointer_input_ancestors: &mut Vec<NodeId>,
    capture_path: &mut SmallVec<[NodeId; 8]>,
) {
    if !layer.has_hit_targets {
        return;
    }
    let transform = layer.transform_to_parent.then(parent_transform);
    let transformed_quad = transform.map_rect(layer.local_bounds);
    let transformed_rect = quad_bounds(transformed_quad);

    if transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0 {
        return;
    }

    let Some(world_to_local) = transform.inverse() else {
        return;
    };

    let mut hit_clip_bounds = parent_hit_clip;
    let mut pushed_clip = false;
    if let Some(local_clip) = layer.clip_rect() {
        let clip_quad = transform.map_rect(local_clip);
        let clip_bounds = quad_bounds(clip_quad);
        let resolved_clip_bounds = resolve_clip(parent_hit_clip, Some(clip_bounds));
        if resolved_clip_bounds.is_some_and(|clip| clip.is_empty()) {
            return;
        }
        hit_clip_bounds = resolved_clip_bounds;
        hit_clips.push(HitClip {
            quad: clip_quad,
            bounds: clip_bounds,
        });
        pushed_clip = true;
    }

    if let (Some(node_id), Some(hit)) = (layer.node_id, &layer.hit_test) {
        capture_path.clear();
        capture_path.push(node_id);
        capture_path.extend(pointer_input_ancestors.iter().rev().copied());
        sink.push_hit(
            node_id,
            capture_path,
            HitGeometry {
                rect: transformed_rect,
                quad: transformed_quad,
                local_bounds: layer.local_bounds,
                world_to_local,
                hit_clip_bounds,
                hit_clips,
            },
            hit,
        );
    }

    let pointer_input_ancestor = layer
        .hit_test
        .as_ref()
        .filter(|hit| !hit.pointer_inputs.is_empty())
        .and(layer.node_id);
    if let Some(node_id) = pointer_input_ancestor {
        pointer_input_ancestors.push(node_id);
    }

    for child in &layer.children {
        if let RenderNode::Layer(child_layer) = child {
            collect_hits_from_graph_inner(
                child_layer,
                transform,
                sink,
                hit_clip_bounds,
                hit_clips,
                pointer_input_ancestors,
                capture_path,
            );
        }
    }

    if pointer_input_ancestor.is_some() {
        let _ = pointer_input_ancestors.pop();
    }

    if pushed_clip {
        let _ = hit_clips.pop();
    }
}

#[cfg(test)]
#[path = "tests/hit_graph_tests.rs"]
mod tests;
