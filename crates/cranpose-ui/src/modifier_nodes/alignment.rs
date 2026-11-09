// The alignment modifiers: the node that tells a box, a row or a column
// where to align the content, and its element.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState,
};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, VerticalAlignment};

use super::{hash_alignment, hash_horizontal_alignment, hash_vertical_alignment};

#[derive(Debug)]
pub(crate) struct AlignmentNode {
    box_alignment: Option<Alignment>,
    column_alignment: Option<HorizontalAlignment>,
    row_alignment: Option<VerticalAlignment>,
    row_baseline: bool,
    state: NodeState,
}

impl AlignmentNode {
    pub(crate) fn new(
        box_alignment: Option<Alignment>,
        column_alignment: Option<HorizontalAlignment>,
        row_alignment: Option<VerticalAlignment>,
        row_baseline: bool,
    ) -> Self {
        Self {
            box_alignment,
            column_alignment,
            row_alignment,
            row_baseline,
            state: NodeState::new(),
        }
    }

    pub(crate) fn box_alignment(&self) -> Option<Alignment> {
        self.box_alignment
    }

    pub(crate) fn column_alignment(&self) -> Option<HorizontalAlignment> {
        self.column_alignment
    }

    pub(crate) fn row_alignment(&self) -> Option<VerticalAlignment> {
        self.row_alignment
    }

    pub(crate) fn row_baseline(&self) -> bool {
        self.row_baseline
    }
}

impl DelegatableNode for AlignmentNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for AlignmentNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::Layout);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AlignmentElement {
    box_alignment: Option<Alignment>,
    column_alignment: Option<HorizontalAlignment>,
    row_alignment: Option<VerticalAlignment>,
    row_baseline: bool,
}

impl AlignmentElement {
    pub(crate) fn box_alignment(alignment: Alignment) -> Self {
        Self {
            box_alignment: Some(alignment),
            column_alignment: None,
            row_alignment: None,
            row_baseline: false,
        }
    }

    pub(crate) fn column_alignment(alignment: HorizontalAlignment) -> Self {
        Self {
            box_alignment: None,
            column_alignment: Some(alignment),
            row_alignment: None,
            row_baseline: false,
        }
    }

    pub(crate) fn row_alignment(alignment: VerticalAlignment) -> Self {
        Self {
            box_alignment: None,
            column_alignment: None,
            row_alignment: Some(alignment),
            row_baseline: false,
        }
    }

    pub(crate) fn row_baseline() -> Self {
        Self {
            box_alignment: None,
            column_alignment: None,
            row_alignment: None,
            row_baseline: true,
        }
    }
}

impl Hash for AlignmentElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Some(alignment) = self.box_alignment {
            state.write_u8(1);
            hash_alignment(state, alignment);
        } else {
            state.write_u8(0);
        }
        if let Some(alignment) = self.column_alignment {
            state.write_u8(1);
            hash_horizontal_alignment(state, alignment);
        } else {
            state.write_u8(0);
        }
        if let Some(alignment) = self.row_alignment {
            state.write_u8(1);
            hash_vertical_alignment(state, alignment);
        } else {
            state.write_u8(0);
        }
        self.row_baseline.hash(state);
    }
}

impl ModifierNodeElement for AlignmentElement {
    type Node = AlignmentNode;
    const FROM_NODE: Option<fn(&AlignmentNode) -> Self> = Some(|node| Self {
        box_alignment: node.box_alignment,
        column_alignment: node.column_alignment,
        row_alignment: node.row_alignment,
        row_baseline: node.row_baseline,
    });

    fn create(&self) -> Self::Node {
        AlignmentNode::new(
            self.box_alignment,
            self.column_alignment,
            self.row_alignment,
            self.row_baseline,
        )
    }

    fn update(&self, node: &mut Self::Node) {
        if node.box_alignment != self.box_alignment {
            node.box_alignment = self.box_alignment;
        }
        if node.column_alignment != self.column_alignment {
            node.column_alignment = self.column_alignment;
        }
        if node.row_alignment != self.row_alignment {
            node.row_alignment = self.row_alignment;
        }
        node.row_baseline = self.row_baseline;
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}
