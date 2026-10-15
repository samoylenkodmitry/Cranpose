use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState,
};
use cranpose_ui_layout::LayoutData;

use super::{Modifier, inspector_metadata};

impl Modifier {
    /// Gives the parent layout `data`, which its measure policy reads with
    /// [`cranpose_ui_layout::ParentData::layout_data`]. When a chain has more
    /// than one, the parent sees the first. Compose's `ParentDataModifier`.
    pub fn layout_data(self, data: impl LayoutData) -> Self {
        let data: Rc<dyn LayoutData> = Rc::new(data);
        let inspected = Rc::clone(&data);
        let modifier = Self::with_element(LayoutDataElement { data }).with_inspector_metadata(
            inspector_metadata("layoutData", move |info| {
                info.add_property("data", format!("{inspected:?}"));
            }),
        );
        self.then(modifier)
    }
}

/// Holds the [`LayoutData`] that the chain gives the parent.
#[derive(Debug)]
pub(crate) struct LayoutDataNode {
    data: Rc<dyn LayoutData>,
    state: NodeState,
}

impl LayoutDataNode {
    pub(crate) fn data(&self) -> &Rc<dyn LayoutData> {
        &self.data
    }
}

impl DelegatableNode for LayoutDataNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for LayoutDataNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::Layout);
    }
}

#[derive(Debug)]
struct LayoutDataElement {
    data: Rc<dyn LayoutData>,
}

impl PartialEq for LayoutDataElement {
    fn eq(&self, other: &Self) -> bool {
        self.data.same_as(&*other.data)
    }
}

impl Hash for LayoutDataElement {
    /// Equal data has the same type, so the type is a hash equal elements share.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.data.as_any().type_id().hash(state);
    }
}

impl ModifierNodeElement for LayoutDataElement {
    type Node = LayoutDataNode;

    fn create(&self) -> Self::Node {
        LayoutDataNode {
            data: Rc::clone(&self.data),
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        if !node.data.same_as(&*self.data) {
            node.data = Rc::clone(&self.data);
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}
