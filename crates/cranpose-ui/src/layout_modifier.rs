//! `Modifier::layout`: Compose's `Modifier.layout { measurable, constraints -> }`.

use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState,
};
use cranpose_ui_layout::{LayoutModifierMeasureResult, Placeable};

use crate::{
    modifier::{Modifier, inspector_metadata},
    modifier_nodes::impl_layout_modifier_node,
};

type MeasureBlock = dyn Fn(&dyn Measurable, Constraints) -> LayoutModifierMeasureResult;

impl Modifier {
    /// Measures and places the content this modifier wraps by `measure`:
    /// Compose's `Modifier.layout { measurable, constraints -> }`. `measure`
    /// measures the content with the constraints it chooses and gives the
    /// size this modifier takes and where the content lies in it, in
    /// logical pixels. A state it reads measures it again, and the intrinsic
    /// sizes of this modifier run it on the content's intrinsic sizes, as in
    /// Compose.
    ///
    /// Example: content 20 taller, with 10 above it:
    ///
    /// ```
    /// use cranpose_ui::{LayoutModifierMeasureResult, Modifier, Size};
    ///
    /// let taller = Modifier::empty().layout(|measurable, constraints| {
    ///     let placeable = measurable.measure(constraints);
    ///     LayoutModifierMeasureResult::new(
    ///         Size::new(placeable.width(), placeable.height() + 20.0),
    ///         0.0,
    ///         10.0,
    ///     )
    /// });
    /// ```
    pub fn layout(
        self,
        measure: impl Fn(&dyn Measurable, Constraints) -> LayoutModifierMeasureResult + 'static,
    ) -> Self {
        let modifier = Self::with_element(LayoutElement {
            measure: Rc::new(measure),
        })
        .with_inspector_metadata(inspector_metadata("layout", |info| {
            info.add_property("measure", "<closure>");
        }));
        self.then(modifier)
    }
}

// Compose's `LayoutElement`: equal to another only for the same closure.
#[derive(Clone)]
struct LayoutElement {
    measure: Rc<MeasureBlock>,
}

impl std::fmt::Debug for LayoutElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutElement").finish_non_exhaustive()
    }
}

impl PartialEq for LayoutElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.measure, &other.measure)
    }
}

impl Eq for LayoutElement {}

impl Hash for LayoutElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.measure).cast::<()>().hash(state);
    }
}

impl ModifierNodeElement for LayoutElement {
    type Node = LayoutModifierImpl;

    fn create(&self) -> Self::Node {
        LayoutModifierImpl {
            measure: Rc::clone(&self.measure),
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.measure = Rc::clone(&self.measure);
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// Compose's `LayoutModifierImpl`, the node of a `LayoutElement`.
struct LayoutModifierImpl {
    measure: Rc<MeasureBlock>,
    state: NodeState,
}

impl DelegatableNode for LayoutModifierImpl {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(LayoutModifierImpl, invalidate = InvalidationKind::Layout);

impl LayoutModifierNode for LayoutModifierImpl {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        (self.measure)(measurable, constraints)
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, _density: f32) -> f32 {
        self.intrinsic(measurable, Intrinsic::MinWidth, height)
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, _density: f32) -> f32 {
        self.intrinsic(measurable, Intrinsic::MaxWidth, height)
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        self.intrinsic(measurable, Intrinsic::MinHeight, width)
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        self.intrinsic(measurable, Intrinsic::MaxHeight, width)
    }
}

impl LayoutModifierImpl {
    // The intrinsic size `asked`, across `extent`: Compose's
    // `MeasuringIntrinsics`, which measures with the content standing in at
    // its intrinsic size.
    fn intrinsic(&self, content: &dyn Measurable, asked: Intrinsic, extent: f32) -> f32 {
        let stand_in = IntrinsicStandIn { content, asked };
        if asked.is_width() {
            (self.measure)(&stand_in, Constraints::loose(f32::INFINITY, extent))
                .size
                .width
        } else {
            (self.measure)(&stand_in, Constraints::loose(extent, f32::INFINITY))
                .size
                .height
        }
    }
}

#[derive(Clone, Copy)]
enum Intrinsic {
    MinWidth,
    MaxWidth,
    MinHeight,
    MaxHeight,
}

impl Intrinsic {
    fn is_width(self) -> bool {
        matches!(self, Self::MinWidth | Self::MaxWidth)
    }
}

// The content as it measures for an intrinsic size: Compose's
// `DefaultIntrinsicMeasurable`, whose placeable is the content's intrinsic
// size along the asked axis and the constraints' bound along the other.
struct IntrinsicStandIn<'a> {
    content: &'a dyn Measurable,
    asked: Intrinsic,
}

impl Measurable for IntrinsicStandIn<'_> {
    fn measure(&self, constraints: Constraints) -> Placeable {
        let (width, height) = match self.asked {
            Intrinsic::MinWidth => (
                self.content.min_intrinsic_width(constraints.max_height),
                constraints.max_height,
            ),
            Intrinsic::MaxWidth => (
                self.content.max_intrinsic_width(constraints.max_height),
                constraints.max_height,
            ),
            Intrinsic::MinHeight => (
                constraints.max_width,
                self.content.min_intrinsic_height(constraints.max_width),
            ),
            Intrinsic::MaxHeight => (
                constraints.max_width,
                self.content.max_intrinsic_height(constraints.max_width),
            ),
        };
        Placeable::value(width, height, NodeId::default())
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.content.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.content.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.content.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.content.max_intrinsic_height(width)
    }

    fn parent_data(&self) -> cranpose_ui_layout::ParentData {
        self.content.parent_data()
    }
}
