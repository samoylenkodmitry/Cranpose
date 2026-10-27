// A root node a subcompose policy composed, with the size it was measured at.

use cranpose_core::NodeId;

use crate::modifier::Size;

#[derive(Clone, Copy, Debug)]
pub struct SubcomposeChild {
    node_id: NodeId,
    measured_size: Option<Size>,
}

impl SubcomposeChild {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            measured_size: None,
        }
    }

    pub fn with_size(node_id: NodeId, size: Size) -> Self {
        Self {
            node_id,
            measured_size: Some(size),
        }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn size(&self) -> Size {
        self.measured_size.unwrap_or(Size {
            width: 0.0,
            height: 0.0,
        })
    }

    pub fn width(&self) -> f32 {
        self.size().width
    }

    pub fn height(&self) -> f32 {
        self.size().height
    }

    pub fn set_size(&mut self, size: Size) {
        self.measured_size = Some(size);
    }
}

impl PartialEq for SubcomposeChild {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
    }
}
