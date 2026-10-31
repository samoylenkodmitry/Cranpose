// A layout node drawn in its parent's layer.

use std::cell::Cell;

use cranpose_core::NodeId;
use cranpose_ui::{Point, Rect, Size};

use super::{
    CONTAINED_DRAW_SLACK, LayerChange, ProjectiveTransform, RenderNode, inflate_rect,
    nodes_draw_within, rect_within,
};

/// A layout node drawn into its parent's layer instead of a layer of its
/// own, as Compose draws a node without a graphics layer into the display
/// list of the layer above it: a leaf with no layer properties, pointer
/// handling or window origins to publish. The scene matches it with its
/// node as it matches a child layer, and a renderer places its content as it
/// would place a child layer that only moves.
#[derive(Clone, Default)]
pub struct InlineNode {
    /// Layout node drawn here.
    pub node_id: NodeId,
    /// The node's top left in its parent's layer: its placement and the
    /// parent's content offset, where a layer of its own would lie.
    pub origin: Point,
    /// The node's size: its rect in its own space starts at the origin.
    pub size: Size,
    /// Whether the node lies in a motion context such as an animated scroll,
    /// where a renderer must not snap its content rigidly to the pixel grid.
    pub motion_context_animated: bool,
    /// The number the node took when it last changed.
    pub change: LayerChange,
    /// The node's draws and text in draw order, in its own space: primitives
    /// and draw runs only.
    pub content: Vec<RenderNode>,
    contained: Cell<Option<bool>>,
    content_hash: Cell<Option<u64>>,
}

impl InlineNode {
    /// The node's rect in its own space.
    pub fn node_rect(&self) -> Rect {
        Rect::from_size(self.size)
    }

    /// Maps the node's space to its parent layer's: a translation to
    /// [`Self::origin`].
    pub fn transform_to_parent(&self) -> ProjectiveTransform {
        ProjectiveTransform::translation(self.origin.x, self.origin.y)
    }

    /// Whether everything the node draws lies within its rect, give or take
    /// [`CONTAINED_DRAW_SLACK`], as [`super::LayerNode::draws_within_bounds`]
    /// says of a layer. Worked out at the first call after a change.
    pub fn draws_within_bounds(&self) -> bool {
        if let Some(contained) = self.contained.get() {
            return contained;
        }
        let contained = nodes_draw_within(
            &self.content,
            inflate_rect(self.node_rect(), CONTAINED_DRAW_SLACK),
        );
        self.contained.set(Some(contained));
        contained
    }

    // Whether the node puts nothing outside `bounds` of its parent's layer.
    pub(crate) fn draws_within_parent(&self, bounds: Rect) -> bool {
        self.draws_within_bounds()
            && rect_within(
                inflate_rect(self.node_rect(), CONTAINED_DRAW_SLACK)
                    .translate(self.origin.x, self.origin.y),
                bounds,
            )
    }

    // The hash of the node's content, worked out at the first call after a
    // change: a layer above it hashes its own content again whenever it
    // changes, as it reads a child layer's kept hash.
    pub(crate) fn content_hash(&self) -> u64 {
        if let Some(hash) = self.content_hash.get() {
            return hash;
        }
        let hash = crate::graph_hash::inline_content_hash(self);
        self.content_hash.set(Some(hash));
        hash
    }

    // Takes a new change number after the node's content or place changed.
    pub(crate) fn note_change(&mut self) {
        self.change = LayerChange::next();
        self.contained.set(None);
        self.content_hash.set(None);
    }
}
