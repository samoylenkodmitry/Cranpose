// The layer tree that describes a scene by hand: a renderer's tests write
// their scenes as one, and the store holds it as records.

use cranpose_core::NodeId;
use cranpose_ui::{Point, Rect, Size};

use super::{
    CachePolicy, DrawRunNode, HitTestNode, InlineNode, IsolationReasons, LayerProperties,
    PrimitiveNode, PrimitivePhase, ProjectiveTransform,
};

/// One primitive of a layer together with the phase that orders it against the children.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveEntry {
    /// Whether the primitive draws before or after the children of its layer.
    pub phase: PrimitivePhase,
    /// The primitive.
    pub node: PrimitiveNode,
}

/// A node of the render graph that groups content under one set of layer properties: bounds,
/// transform, clip, graphics layer and children.
#[derive(Clone)]
pub struct LayerNode {
    /// Layout node of this layer, or `None` for the root and the synthetic wrapper layers.
    pub node_id: Option<NodeId>,
    /// Set on the synthetic layer that holds a node's outer draws, those chained
    /// before its graphics layer, around that node's own layer. Scene updates
    /// address the node through this id because the node's layer has none of the
    /// outer draws and the wrapper has no node id of its own.
    pub wraps: Option<NodeId>,
    /// The node's size. Its rect, `(0, 0, size)`, is where its pointer
    /// handlers are and where its draws and children are placed from.
    pub size: Size,
    /// The bounds of the node's layer, where its clip cuts, its transforms
    /// pivot and an offscreen pass draws it, when they differ from the
    /// node's rect: a clip or graphics layer that wraps a coordinator a later
    /// padding or offset moves bounds the layer there, as in Compose. `None`
    /// when the layer bounds are the node's rect.
    pub layer_bounds: Option<Rect>,
    /// Maps the layer's local space to its parent's content space, with the placement, the graphics
    /// layer transform and the parent's content offset applied.
    pub transform_to_parent: ProjectiveTransform,
    /// Offset in logical pixels of the node's content box from the node origin, for example from
    /// padding. Children are placed relative to it.
    pub content_offset: Point,
    /// Whether the layer is inside a motion context such as an animated scroll, where a renderer
    /// must not snap its content rigidly to the pixel grid.
    pub motion_context_animated: bool,
    /// Whether the layer moves its content as one translated block, as a scroll container does,
    /// so a renderer may snap that content to the pixel grid as a whole.
    pub translated_content_context: bool,
    /// Translation in logical pixels of the content when `translated_content_context` is set.
    pub translated_content_offset: Point,
    /// Where layout placed this layer within its parent's content, before the
    /// parent's content offset. Scene updates add these up from the root, with
    /// each layer's content offset and graphics-layer translation, to find the
    /// window origin of a subtree they rebuild.
    pub origin_in_parent: Point,
    /// The graphics layer properties; they read as defaults when none were set.
    pub graphics_layer: LayerProperties,
    /// Whether the layer cuts its content to its layer bounds.
    pub clip_to_bounds: bool,
    /// Pointer handling of the node, or `None` when the node has none.
    pub hit_test: Option<HitTestNode>,
    /// Whether this subtree publishes live window origins (a text field's
    /// popup anchor, a scroll container's viewport rect). Those sinks are
    /// written during a full lowering, so the scroll fast path may translate
    /// a retained subtree in place only when this is false.
    pub has_origin_sinks: bool,
    /// The reasons this layer is drawn through its own offscreen buffer.
    pub isolation: IsolationReasons,
    /// Set on a layer that holds part of one draw command's draws, those a
    /// `DrawScope` transform or clip groups: the phase they paint in among
    /// the parent's own draws, which they belong to. `None` on the layer of
    /// a layout node and on the root.
    pub draw_phase: Option<PrimitivePhase>,
    /// Whether a renderer may keep this layer's pixels between frames.
    pub cache_policy: CachePolicy,
    /// Content in draw order: primitives, draw runs and child layers.
    pub children: Vec<RenderNode>,
}

impl Default for LayerNode {
    fn default() -> Self {
        Self {
            node_id: None,
            wraps: None,
            size: Size::default(),
            layer_bounds: None,
            transform_to_parent: ProjectiveTransform::identity(),
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            origin_in_parent: Point::default(),
            graphics_layer: LayerProperties::default(),
            clip_to_bounds: false,
            hit_test: None,
            has_origin_sinks: false,
            isolation: IsolationReasons::default(),
            draw_phase: None,
            cache_policy: CachePolicy::None,
            children: Vec::new(),
        }
    }
}

/// A child of a layer in the render graph.
#[derive(Clone)]
pub enum RenderNode {
    /// A single draw or text primitive.
    Primitive(PrimitiveEntry),
    /// A whole draw command's primitives as one node. A heavy canvas records
    /// thousands of primitives per frame; wrapping each in its own
    /// [`RenderNode`] made the graph rebuild move every one of them twice and
    /// free seventeen thousand nodes per frame on a stress scene. The run
    /// keeps the recorded vector intact — semantically it is exactly that
    /// many consecutive `Primitive` draw entries with no per-primitive clip.
    DrawRun(DrawRunNode),
    /// A nested layer.
    Layer(Box<LayerNode>),
    /// A layout node drawn in this layer instead of a layer of its own.
    Inline(Box<InlineNode>),
}

impl RenderNode {
    /// The phase the node paints in among its layer's own draws: a
    /// primitive's or run's, or a draw group layer's. `None` for the layer
    /// of a layout node, which is a child of the layer.
    pub fn draw_phase(&self) -> Option<PrimitivePhase> {
        match self {
            Self::Primitive(entry) => Some(entry.phase),
            Self::DrawRun(run) => Some(run.phase),
            Self::Layer(layer) => layer.draw_phase,
            Self::Inline(_) => None,
        }
    }

    fn draws_after_children(&self) -> bool {
        self.draw_phase() == Some(PrimitivePhase::AfterChildren)
    }
}

/// Visits `children`, a layer's child list, in the order they draw: in list
/// order, save that the draws after the children wait until every other
/// node has drawn.
pub fn for_each_in_draw_order(children: &[RenderNode], mut visit: impl FnMut(&RenderNode)) {
    let mut first_deferred = children.len();
    for (index, child) in children.iter().enumerate() {
        if child.draws_after_children() {
            first_deferred = first_deferred.min(index);
        } else {
            visit(child);
        }
    }
    children[first_deferred..]
        .iter()
        .filter(|child| child.draws_after_children())
        .for_each(visit);
}
