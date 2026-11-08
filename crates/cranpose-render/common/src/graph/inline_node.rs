// A layout node drawn in its parent's layer.

use cranpose_core::NodeId;
use cranpose_ui::{Point, Size};

use super::RenderNode;

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
    /// The node's draws and text in draw order, in its own space: primitives
    /// and draw runs only.
    pub content: Vec<RenderNode>,
}
