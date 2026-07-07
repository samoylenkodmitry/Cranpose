use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_foundation::PointerEvent;
use cranpose_ui::Point;
use cranpose_ui_graphics::{Rect, RoundedCornerShape};

use crate::graph::quad_bounds;
use crate::graph::{LayerNode, ProjectiveTransform, RenderNode};
use crate::graph_scene::{HitClip, HitGeometry};
use crate::primitive_emit::resolve_clip;

pub trait HitGraphSink {
    fn push_hit(
        &mut self,
        node_id: NodeId,
        capture_path: &[NodeId],
        geometry: HitGeometry,
        shape: Option<RoundedCornerShape>,
        click_actions: &[Rc<dyn Fn(Point)>],
        pointer_inputs: &[Rc<dyn Fn(PointerEvent)>],
    );
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
    collect_hits_from_graph_inner(
        layer,
        parent_transform,
        sink,
        parent_hit_clip,
        &mut hit_clips,
        &mut pointer_input_ancestors,
    );
}

fn collect_hits_from_graph_inner<S: HitGraphSink>(
    layer: &LayerNode,
    parent_transform: ProjectiveTransform,
    sink: &mut S,
    parent_hit_clip: Option<Rect>,
    hit_clips: &mut Vec<HitClip>,
    pointer_input_ancestors: &mut Vec<NodeId>,
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
        let Some(resolved_clip_bounds) = resolve_clip(parent_hit_clip, Some(clip_bounds)) else {
            return;
        };
        hit_clip_bounds = Some(resolved_clip_bounds);
        hit_clips.push(HitClip {
            quad: clip_quad,
            bounds: clip_bounds,
        });
        pushed_clip = true;
    }

    if let (Some(node_id), Some(hit)) = (layer.node_id, &layer.hit_test) {
        let mut capture_path = Vec::with_capacity(1 + pointer_input_ancestors.len());
        capture_path.push(node_id);
        capture_path.extend(pointer_input_ancestors.iter().rev().copied());
        sink.push_hit(
            node_id,
            &capture_path,
            HitGeometry {
                rect: transformed_rect,
                quad: transformed_quad,
                local_bounds: layer.local_bounds,
                world_to_local,
                hit_clip_bounds,
                hit_clips: hit_clips.to_vec(),
            },
            hit.shape,
            &hit.click_actions,
            &hit.pointer_inputs,
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
mod tests {
    use super::*;
    use crate::graph::{CachePolicy, HitTestNode, IsolationReasons};
    use crate::raster_cache::LayerRasterCacheHashes;

    type RecordedHit = (
        NodeId,
        Vec<NodeId>,
        Rect,
        [[f32; 2]; 4],
        Option<Rect>,
        usize,
    );

    #[derive(Default)]
    struct TestSink {
        hits: Vec<RecordedHit>,
    }

    impl HitGraphSink for TestSink {
        fn push_hit(
            &mut self,
            node_id: NodeId,
            capture_path: &[NodeId],
            geometry: HitGeometry,
            _shape: Option<RoundedCornerShape>,
            _click_actions: &[Rc<dyn Fn(Point)>],
            _pointer_inputs: &[Rc<dyn Fn(PointerEvent)>],
        ) {
            self.hits.push((
                node_id,
                capture_path.to_vec(),
                geometry.rect,
                geometry.quad,
                geometry.hit_clip_bounds,
                geometry.hit_clips.len(),
            ));
        }
    }

    fn test_layer(node_id: NodeId, transform_to_parent: ProjectiveTransform) -> LayerNode {
        LayerNode {
            node_id: Some(node_id),
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 18.0,
            },
            transform_to_parent,
            content_offset: cranpose_ui_graphics::Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: cranpose_ui_graphics::Point::default(),
            scene_children_origin: cranpose_ui_graphics::Point::default(),
            scene_children_layer_translation: cranpose_ui_graphics::Point::default(),
            graphics_layer: cranpose_ui_graphics::GraphicsLayer::default(),
            clip_to_bounds: true,
            shadow_clip: None,
            hit_test: Some(HitTestNode {
                shape: None,
                click_actions: vec![Rc::new(|_point| {})],
                pointer_inputs: vec![],
                clip: None,
            }),
            has_hit_targets: true,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![],
        }
    }

    #[test]
    fn collect_hits_uses_graph_transform_to_parent() {
        let layer = test_layer(7, ProjectiveTransform::translation(12.0, 9.0));
        let mut sink = TestSink::default();

        collect_hits_from_graph(&layer, ProjectiveTransform::identity(), &mut sink, None);

        assert_eq!(sink.hits.len(), 1);
        let (node_id, capture_path, rect, quad, clip, clip_count) = &sink.hits[0];
        assert_eq!(*node_id, 7);
        assert_eq!(capture_path, &vec![7]);
        assert_eq!(
            *rect,
            Rect {
                x: 12.0,
                y: 9.0,
                width: 30.0,
                height: 18.0,
            }
        );
        assert_eq!(
            *quad,
            [[12.0, 9.0], [42.0, 9.0], [12.0, 27.0], [42.0, 27.0]]
        );
        assert_eq!(*clip, Some(*rect));
        assert_eq!(*clip_count, 1);
    }

    #[test]
    fn collect_hits_composes_nested_graph_transforms() {
        let child = test_layer(9, ProjectiveTransform::translation(4.0, 3.0));
        let mut parent = test_layer(7, ProjectiveTransform::translation(10.0, 6.0));
        parent.hit_test.as_mut().expect("hit test").pointer_inputs = vec![Rc::new(|_event| {})];
        parent.children.push(RenderNode::Layer(Box::new(child)));
        let mut sink = TestSink::default();

        collect_hits_from_graph(&parent, ProjectiveTransform::identity(), &mut sink, None);

        assert_eq!(sink.hits.len(), 2);
        let (_, child_capture_path, child_rect, child_quad, child_clip, child_clip_count) =
            &sink.hits[1];
        assert_eq!(child_capture_path, &vec![9, 7]);
        assert_eq!(
            *child_rect,
            Rect {
                x: 14.0,
                y: 9.0,
                width: 30.0,
                height: 18.0,
            }
        );
        assert_eq!(
            *child_quad,
            [[14.0, 9.0], [44.0, 9.0], [14.0, 27.0], [44.0, 27.0]]
        );
        assert_eq!(
            *child_clip,
            Some(Rect {
                x: 14.0,
                y: 9.0,
                width: 26.0,
                height: 15.0,
            })
        );
        assert_eq!(*child_clip_count, 2);
    }

    #[test]
    fn collect_hits_retains_transformed_clip_chain() {
        let mut parent = test_layer(1, ProjectiveTransform::translation(20.0, 10.0));
        let mut child = test_layer(
            2,
            ProjectiveTransform::from_rect_to_quad(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 30.0,
                    height: 18.0,
                },
                [[0.0, 0.0], [30.0, 0.0], [4.0, 18.0], [34.0, 18.0]],
            ),
        );
        child.clip_to_bounds = true;
        parent.children.push(RenderNode::Layer(Box::new(child)));

        let mut sink = TestSink::default();
        collect_hits_from_graph(&parent, ProjectiveTransform::identity(), &mut sink, None);

        let (_, _, _, _, _, child_clip_count) = sink.hits[1];
        assert_eq!(child_clip_count, 2);
    }
}
