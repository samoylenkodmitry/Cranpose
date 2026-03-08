use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_foundation::PointerEvent;
use cranpose_ui::Point;
use cranpose_ui_graphics::{GraphicsLayer, Rect, RoundedCornerShape};

use crate::graph::{LayerNode, RenderNode};
use crate::primitive_emit::resolve_clip;
use crate::style_shared::{apply_layer_to_rect, combine_layers};

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
    parent_layer: GraphicsLayer,
    sink: &mut S,
    parent_hit_clip: Option<Rect>,
    parent_offset: Point,
) {
    let rect = Rect {
        x: parent_offset.x + layer.placement.x,
        y: parent_offset.y + layer.placement.y,
        width: layer.local_bounds.width,
        height: layer.local_bounds.height,
    };

    let node_layer = combine_layers(parent_layer, Some(layer.graphics_layer.clone()));
    let transformed_rect = apply_layer_to_rect(rect, rect, &node_layer);

    if transformed_rect.width <= 0.0 || transformed_rect.height <= 0.0 {
        return;
    }

    let content_clip_to_bounds = layer.clip_to_bounds || node_layer.clip;
    let hit_clip = resolve_clip(
        parent_hit_clip,
        content_clip_to_bounds.then_some(transformed_rect),
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

    let child_offset = Point {
        x: rect.x + layer.content_offset.x,
        y: rect.y + layer.content_offset.y,
    };
    for child in &layer.children {
        if let RenderNode::Layer(child_layer) = child {
            collect_hits_from_graph(
                child_layer,
                node_layer.clone(),
                sink,
                hit_clip,
                child_offset,
            );
        }
    }
}
