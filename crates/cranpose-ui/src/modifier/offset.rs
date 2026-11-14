use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeContext, ModifierNodeElement, NodeCapabilities,
    NodeState, plain_element,
};

use super::{GraphicsLayer, Modifier, Point, inspector_metadata};
use crate::{
    LayoutDirection,
    modifier_nodes::{FractionalOffsetElement, GraphicsLayerNode, OffsetElement},
};

impl Modifier {
    /// Offset the content by (x, y). The offsets can be positive or negative.
    ///
    /// This modifier is RTL-aware: positive x offsets move content right in LTR
    /// and left in RTL layouts.
    ///
    /// Matches Kotlin: `Modifier.offset(x: Dp, y: Dp)`
    ///
    /// Example: `Modifier::empty().offset(10.0, 20.0)`
    pub fn offset(self, x: f32, y: f32) -> Self {
        let modifier = Self::from_dyn_element(plain_element(OffsetElement::new(x, y, true)))
            .with_inspector_metadata(inspector_metadata("offset", move |info| {
                info.add_offset_components("offsetX", "offsetY", Point { x, y });
            }));
        self.then(modifier)
    }

    /// Offset the content by (x, y) without considering layout direction.
    ///
    /// Positive x always moves content to the right regardless of RTL.
    ///
    /// Matches Kotlin: `Modifier.absoluteOffset(x: Dp, y: Dp)`
    ///
    /// Example: `Modifier::empty().absolute_offset(10.0, 20.0)`
    pub fn absolute_offset(self, x: f32, y: f32) -> Self {
        let modifier = Self::from_dyn_element(plain_element(OffsetElement::new(x, y, false)))
            .with_inspector_metadata(inspector_metadata("absoluteOffset", move |info| {
                info.add_offset_components("absoluteOffsetX", "absoluteOffsetY", Point { x, y });
            }));
        self.then(modifier)
    }

    /// Offset the content by a fraction of its own measured size.
    ///
    /// `x_fraction` / `y_fraction` are multiplied by the measured width /
    /// height of the content when it is placed, so `offset_fraction(0.0,
    /// -0.5)` moves the content up by half its own height. Like
    /// [`Modifier::offset`], this only affects placement, not measurement.
    ///
    /// There is no direct Jetpack Compose equivalent; Compose's
    /// `slideInVertically`/`slideOutVertically` receive the measured size via
    /// a lambda instead. This modifier backs `slide_in_vertically` /
    /// `slide_out_vertically` in `AnimatedVisibility`.
    ///
    /// Example: `Modifier::empty().offset_fraction(0.0, -0.5)`
    pub fn offset_fraction(self, x_fraction: f32, y_fraction: f32) -> Self {
        let modifier = Self::from_dyn_element(plain_element(FractionalOffsetElement::new(
            x_fraction, y_fraction,
        )))
        .with_inspector_metadata(inspector_metadata("offsetFraction", move |info| {
            info.add_property("offsetFractionX", x_fraction.to_string());
            info.add_property("offsetFractionY", y_fraction.to_string());
        }));
        self.then(modifier)
    }

    /// Offsets the content by what `offset` returns, in logical pixels:
    /// Compose's `Modifier.offset { }`. The scene reads `offset` as it
    /// draws, so a state it reads moves the content and where it takes
    /// clicks without composing or measuring it again. A right-to-left
    /// layout direction mirrors `x`.
    ///
    /// Example: `Modifier::empty().offset_with(move || Point::new(drag.get(), 0.0))`
    pub fn offset_with(self, offset: impl Fn() -> Point + 'static) -> Self {
        self.then_offset_px(OffsetPxElement::new(Rc::new(offset), true), "offset")
    }

    /// [`Modifier::offset_with`] without considering layout direction:
    /// Compose's `Modifier.absoluteOffset { }`. Positive `x` always moves the
    /// content to the right.
    pub fn absolute_offset_with(self, offset: impl Fn() -> Point + 'static) -> Self {
        self.then_offset_px(
            OffsetPxElement::new(Rc::new(offset), false),
            "absoluteOffset",
        )
    }

    fn then_offset_px(self, element: OffsetPxElement, name: &'static str) -> Self {
        let modifier =
            Self::with_element(element).with_inspector_metadata(inspector_metadata(name, |info| {
                info.add_property("lazy", "true");
            }));
        self.then(modifier)
    }
}

// Moves its content by a closure's offset, which the scene reads as a layer
// translation: Compose's `OffsetPxElement`, which places with a layer.
#[derive(Clone)]
pub(crate) struct OffsetPxElement {
    offset: Rc<dyn Fn() -> Point>,
    rtl_aware: bool,
}

impl OffsetPxElement {
    fn new(offset: Rc<dyn Fn() -> Point>, rtl_aware: bool) -> Self {
        Self { offset, rtl_aware }
    }
}

impl std::fmt::Debug for OffsetPxElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OffsetPxElement")
            .field("rtl_aware", &self.rtl_aware)
            .finish_non_exhaustive()
    }
}

impl PartialEq for OffsetPxElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.offset, &other.offset) && self.rtl_aware == other.rtl_aware
    }
}

impl Eq for OffsetPxElement {}

impl Hash for OffsetPxElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.offset).cast::<()>().hash(state);
        self.rtl_aware.hash(state);
    }
}

impl ModifierNodeElement for OffsetPxElement {
    type Node = OffsetPxNode;

    fn create(&self) -> Self::Node {
        OffsetPxNode {
            layer: GraphicsLayerNode::new_lazy(translation(Rc::clone(&self.offset), false)),
            offset: Rc::clone(&self.offset),
            rtl_aware: self.rtl_aware,
            layout_direction: LayoutDirection::Ltr,
        }
    }

    fn update(&self, node: &mut Self::Node) {
        if !Rc::ptr_eq(&node.offset, &self.offset) || node.rtl_aware != self.rtl_aware {
            node.offset = Rc::clone(&self.offset);
            node.rtl_aware = self.rtl_aware;
            node.resolve_translation();
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }

    fn always_update(&self) -> bool {
        true
    }

    fn auto_invalidate_on_update(&self) -> bool {
        false
    }
}

// The node of an [`OffsetPxElement`]: a lazy graphics layer whose
// translation is the offset, with `x` mirrored right to left when it is
// RTL aware.
pub(crate) struct OffsetPxNode {
    layer: GraphicsLayerNode,
    offset: Rc<dyn Fn() -> Point>,
    rtl_aware: bool,
    layout_direction: LayoutDirection,
}

impl OffsetPxNode {
    pub(crate) fn layer(&self) -> &GraphicsLayerNode {
        &self.layer
    }

    pub(crate) fn set_layout_direction(&mut self, direction: LayoutDirection) {
        if self.layout_direction != direction {
            self.layout_direction = direction;
            self.resolve_translation();
        }
    }

    // Gives the layer a resolver for the current offset and direction, which
    // draws the node again.
    fn resolve_translation(&mut self) {
        let mirror = self.rtl_aware && self.layout_direction.is_rtl();
        self.layer
            .set_lazy(translation(Rc::clone(&self.offset), mirror));
    }
}

fn translation(offset: Rc<dyn Fn() -> Point>, mirror: bool) -> Rc<dyn Fn() -> GraphicsLayer> {
    Rc::new(move || {
        let offset = offset();
        GraphicsLayer {
            translation_x: if mirror { -offset.x } else { offset.x },
            translation_y: offset.y,
            ..GraphicsLayer::default()
        }
    })
}

impl DelegatableNode for OffsetPxNode {
    fn node_state(&self) -> &NodeState {
        self.layer.node_state()
    }
}

impl ModifierNode for OffsetPxNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.layer.on_attach(context);
    }

    fn on_detach(&mut self) {
        self.layer.on_detach();
    }
}
