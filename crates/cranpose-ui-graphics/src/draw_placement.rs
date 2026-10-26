/// Which half of a draw command is drawn, relative to the content of the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrawPlacement {
    /// Drawn behind the node's content.
    Behind,
    /// Drawn over the node's content.
    Overlay,
}
