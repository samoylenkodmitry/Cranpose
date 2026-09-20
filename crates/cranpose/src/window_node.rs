use std::{cell::Cell, rc::Rc};

use cranpose_foundation::{
    Constraints, DelegatableNode, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState,
};
use cranpose_ui::{Modifier, Size, WindowRootDescriptor, WindowRootNode};
use cranpose_ui_layout::LayoutModifierMeasureResult;

use crate::native_window::{
    NativeWindowOwner, NativeWindowParts, NativeWindowRoot, WindowConfig, WindowId,
    register_native_window, unregister_native_window,
};

pub(crate) fn window(modifier: Modifier, config: WindowConfig) -> Modifier {
    modifier.then(Modifier::with_element(NativeWindowElement { config }))
}

#[derive(Debug, PartialEq)]
pub(crate) struct NativeWindowElement {
    config: WindowConfig,
}

impl std::hash::Hash for NativeWindowElement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.config.title().hash(state);
    }
}

impl ModifierNodeElement for NativeWindowElement {
    type Node = NativeWindowNode;

    fn create(&self) -> NativeWindowNode {
        let parts = self.config.clone().into_parts();
        let surface = Rc::new(NativeWindowRoot::new(Size::new(
            parts.options.width,
            parts.options.height,
        )));
        let descriptor: Rc<dyn WindowRootDescriptor> = surface.clone();
        NativeWindowNode {
            root: WindowRootNode::new(descriptor),
            surface,
            parts,
            key: Cell::new(None),
            owner: Rc::new(()),
        }
    }

    fn update(&self, node: &mut NativeWindowNode) {
        node.parts = self.config.clone().into_parts();
        node.register();
    }

    fn always_update(&self) -> bool {
        true
    }

    fn auto_invalidate_on_update(&self) -> bool {
        false
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT | NodeCapabilities::WINDOW_ROOT
    }

    fn inspector_name(&self) -> &'static str {
        "window"
    }
}

pub(crate) struct NativeWindowNode {
    root: WindowRootNode,
    surface: Rc<NativeWindowRoot>,
    parts: NativeWindowParts,
    key: Cell<Option<WindowId>>,
    owner: NativeWindowOwner,
}

impl NativeWindowNode {
    fn register(&self) {
        let Some(key) = self.key.get() else {
            return;
        };
        register_native_window(
            key,
            self.parts.options.clone(),
            self.parts.events.clone(),
            self.parts.state,
            Rc::clone(&self.surface),
            Rc::clone(&self.owner),
        );
    }
}

impl DelegatableNode for NativeWindowNode {
    fn node_state(&self) -> &NodeState {
        self.root.node_state()
    }
}

impl ModifierNode for NativeWindowNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.root.on_attach(context);
        let Some(node) = context.node_id() else {
            log::error!("window modifier attached to a node without an id; no window requested");
            return;
        };
        self.key.set(Some(WindowId::from_node(node)));
        self.register();
    }

    fn on_detach(&mut self) {
        self.root.on_detach();
        if let Some(key) = self.key.take() {
            unregister_native_window(key, Rc::clone(&self.owner));
        }
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for NativeWindowNode {
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        self.root.measure(context, measurable, constraints)
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        self.root.min_intrinsic_width(measurable, height)
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        self.root.max_intrinsic_width(measurable, height)
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        self.root.min_intrinsic_height(measurable, width)
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        self.root.max_intrinsic_height(measurable, width)
    }
}
