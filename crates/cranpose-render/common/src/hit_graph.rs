use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_foundation::PointerEvent;
use cranpose_ui::Point;
use cranpose_ui_graphics::{Rect, RoundedCornerShape};

use crate::graph::{LayerNode, ProjectiveTransform, RenderNode};
use crate::primitive_emit::resolve_clip;

pub trait HitGraphSink {
    fn push_hit(
        &mut self,
        node_id: NodeId,
        rect: Rect,
        shape: Option<RoundedCornerShape>,
        click_actions: &[Rc<dyn Fn(Point)>],
        pointer_inputs: &[Rc<dyn Fn(PointerEvent)>],
        hit_clip: Option<Rect>,
    );
}

pub fn collect_hits_from_graph<S: HitGraphSink>(
    layer: &LayerNode,
    parent_transform: ProjectiveTransform,
    sink: &mut S,
    parent_hit_clip: Option<Rect>,
) {
    let transform = layer.transform_to_parent.then(parent_transform);
    let transformed_rect = transform.bounds_for_rect(layer.local_bounds);

    if transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0 {
        return;
    }

    let hit_clip = resolve_clip(
        parent_hit_clip,
        layer
            .clip_rect()
            .map(|clip| transform.bounds_for_rect(clip)),
    );

    if let (Some(node_id), Some(hit)) = (layer.node_id, &layer.hit_test) {
        sink.push_hit(
            node_id,
            transformed_rect,
            hit.shape,
            &hit.click_actions,
            &hit.pointer_inputs,
            hit_clip,
        );
    }

    for child in &layer.children {
        if let RenderNode::Layer(child_layer) = child {
            collect_hits_from_graph(child_layer, transform, sink, hit_clip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CachePolicy, HitTestNode, IsolationReasons};
    use crate::raster_cache::LayerRasterCacheHashes;

    #[derive(Default)]
    struct TestSink {
        hits: Vec<(NodeId, Rect, Option<Rect>)>,
    }

    impl HitGraphSink for TestSink {
        fn push_hit(
            &mut self,
            node_id: NodeId,
            rect: Rect,
            _shape: Option<RoundedCornerShape>,
            _click_actions: &[Rc<dyn Fn(Point)>],
            _pointer_inputs: &[Rc<dyn Fn(PointerEvent)>],
            hit_clip: Option<Rect>,
        ) {
            self.hits.push((node_id, rect, hit_clip));
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
            placement: Point::default(),
            content_offset: Point::default(),
            transform_to_parent,
            graphics_layer: cranpose_ui_graphics::GraphicsLayer::default(),
            clip_to_bounds: true,
            shadow_clip: None,
            hit_test: Some(HitTestNode {
                shape: None,
                click_actions: vec![Rc::new(|_point| {})],
                pointer_inputs: vec![],
                clip: None,
            }),
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            children: vec![],
        }
    }

    #[test]
    fn collect_hits_uses_graph_transform_to_parent() {
        let layer = test_layer(7, ProjectiveTransform::translation(12.0, 9.0));
        let mut sink = TestSink::default();

        collect_hits_from_graph(&layer, ProjectiveTransform::identity(), &mut sink, None);

        assert_eq!(sink.hits.len(), 1);
        let (node_id, rect, clip) = sink.hits[0];
        assert_eq!(node_id, 7);
        assert_eq!(
            rect,
            Rect {
                x: 12.0,
                y: 9.0,
                width: 30.0,
                height: 18.0,
            }
        );
        assert_eq!(clip, Some(rect));
    }

    #[test]
    fn collect_hits_composes_nested_graph_transforms() {
        let child = test_layer(9, ProjectiveTransform::translation(4.0, 3.0));
        let mut parent = test_layer(7, ProjectiveTransform::translation(10.0, 6.0));
        parent.children.push(RenderNode::Layer(Box::new(child)));
        let mut sink = TestSink::default();

        collect_hits_from_graph(&parent, ProjectiveTransform::identity(), &mut sink, None);

        assert_eq!(sink.hits.len(), 2);
        let (_, child_rect, child_clip) = sink.hits[1];
        assert_eq!(
            child_rect,
            Rect {
                x: 14.0,
                y: 9.0,
                width: 30.0,
                height: 18.0,
            }
        );
        assert_eq!(
            child_clip,
            Some(Rect {
                x: 14.0,
                y: 9.0,
                width: 26.0,
                height: 15.0,
            })
        );
    }
}
