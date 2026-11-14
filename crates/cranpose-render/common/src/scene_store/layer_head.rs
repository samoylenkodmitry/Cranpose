// The head of a layer record, as the scene builder works it out for a node,
// a wrapper or a draw group, and its write into the store in one call.

use std::rc::Rc;

use cranpose_core::{NodeId, RecordId};
use cranpose_ui::{GraphicsLayer, Point, Rect};

use super::{LayerProps, RecordFlags, RecordKind, SceneStore};
use crate::graph::{
    CachePolicy, HitTestNode, IsolationReasons, PrimitivePhase, ProjectiveTransform,
};

#[derive(Clone, Copy)]
pub(crate) enum HeadKind {
    Node(NodeId),
    Wrapper(NodeId),
    Group { node: NodeId, phase: PrimitivePhase },
}

impl HeadKind {
    pub(crate) fn record(self) -> (RecordKind, NodeId) {
        match self {
            Self::Node(node) => (RecordKind::Layer, node),
            Self::Wrapper(node) => (RecordKind::Wrapper, node),
            Self::Group { node, .. } => (RecordKind::DrawGroup, node),
        }
    }
}

pub(crate) struct LayerHead {
    pub(crate) kind: HeadKind,
    pub(crate) local_bounds: Rect,
    pub(crate) node_bounds: Option<Rect>,
    pub(crate) transform_to_parent: ProjectiveTransform,
    pub(crate) content_offset: Point,
    pub(crate) motion_context_animated: bool,
    pub(crate) translated_content_context: bool,
    pub(crate) translated_content_offset: Point,
    pub(crate) origin_in_parent: Point,
    pub(crate) graphics_layer: Option<GraphicsLayer>,
    pub(crate) clip_to_bounds: bool,
    pub(crate) hit_test: Option<HitTestNode>,
    pub(crate) has_origin_sinks: bool,
    pub(crate) isolation: IsolationReasons,
    pub(crate) cache_policy: CachePolicy,
}

fn flag_if(on: bool, flag: RecordFlags) -> RecordFlags {
    if on { flag } else { RecordFlags::default() }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    // Writes `head` as the head of `record` and notes a property change,
    // unless the head stayed and the update already noted a change of the
    // record.
    pub(crate) fn write_head(&mut self, record: RecordId, head: LayerHead) {
        let LayerHead {
            kind,
            local_bounds,
            node_bounds,
            transform_to_parent,
            content_offset,
            motion_context_animated,
            translated_content_context,
            translated_content_offset,
            origin_in_parent,
            graphics_layer,
            clip_to_bounds,
            hit_test,
            has_origin_sinks,
            isolation,
            cache_policy,
        } = head;
        let (record_kind, node) = kind.record();
        if self.node(record) != Some(node) {
            // A record that draws for another node starts a new run of changes.
            self.strip_props(record);
        }
        let mut changed = self.set_kind(record, record_kind, Some(node));
        changed |= self.set_content_offset(record, content_offset);
        let size = node_bounds.unwrap_or(local_bounds);
        changed |= self.write_size(record, size.width, size.height);
        changed |= self.set_placement(record, origin_in_parent, Some(transform_to_parent));
        changed |= self.write_graphics_layer(record, graphics_layer);
        let layer_bounds = node_bounds.map(|_| local_bounds);
        let props = (layer_bounds, translated_content_offset, &hit_test);
        if !props_stay(self.props(record), props, (isolation, cache_policy)) {
            self.edit_props(record, |props| {
                props.layer_bounds = layer_bounds;
                props.translated_content_offset = translated_content_offset;
                props.hit_test = hit_test;
                props.isolation = isolation;
                props.cache_policy = cache_policy;
            });
            changed = true;
        }
        let after_children = matches!(
            kind,
            HeadKind::Group {
                phase: PrimitivePhase::AfterChildren,
                ..
            }
        );
        let flags = flag_if(clip_to_bounds, RecordFlags::CLIP_TO_BOUNDS)
            | flag_if(motion_context_animated, RecordFlags::MOTION_ANIMATED)
            | flag_if(translated_content_context, RecordFlags::TRANSLATED_CONTENT)
            | flag_if(after_children, RecordFlags::AFTER_CHILDREN)
            | flag_if(has_origin_sinks, RecordFlags::ORIGIN_SINKS);
        changed |= self.write_flags(record, flags);
        // A head that stayed adds nothing to a change the update already
        // noted for the record, as a redraw of its draws notes.
        if changed || self.content_rev(record) != self.update() {
            self.note_property_change(record);
        }
    }
}

// Whether `props`, or the defaults for `None`, already hold the head's
// layer bounds, translated content offset, pointer handling, isolation and
// cache policy. Pointer handling stays while it shares its slices.
fn props_stay(
    props: Option<&LayerProps>,
    (layer_bounds, offset, hit_test): (Option<Rect>, Point, &Option<HitTestNode>),
    (isolation, cache_policy): (IsolationReasons, CachePolicy),
) -> bool {
    let bits = |rect: Option<Rect>| {
        rect.map(|rect| [rect.x, rect.y, rect.width, rect.height].map(f32::to_bits))
    };
    let Some(props) = props else {
        return layer_bounds.is_none()
            && super::records::same_pair([offset.x, offset.y], [0.0; 2])
            && hit_test.is_none()
            && isolation == IsolationReasons::default()
            && cache_policy == CachePolicy::default();
    };
    let held = props.translated_content_offset;
    bits(props.layer_bounds) == bits(layer_bounds)
        && super::records::same_pair([held.x, held.y], [offset.x, offset.y])
        && match (&props.hit_test, hit_test) {
            (Some(held), Some(hit_test)) => Rc::ptr_eq(&held.handlers, &hit_test.handlers),
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        }
        && props.isolation == isolation
        && props.cache_policy == cache_policy
}
