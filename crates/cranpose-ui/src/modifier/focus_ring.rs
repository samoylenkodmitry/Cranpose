//! The ring a control shows while the keyboard holds focus on it, so a person
//! who works without a pointer sees where the next Enter lands.

use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    DelegatableNode, DrawModifierNode, FocusNode, FocusState, ModifierNode, ModifierNodeContext,
    ModifierNodeElement, NodeCapabilities, NodeDrawClosure, NodeState, impl_draw_node,
    impl_focus_node, modifier_element,
};
use cranpose_ui_graphics::{Brush, Color, DrawScope, DrawScopeDefault, Rect, Size, Stroke};

use super::{
    Modifier,
    focus::{FocusTargetElement, FocusTargetNode},
};
use crate::focus_dispatch::keyboard_focus_visible;

/// The outer line of the ring: a blue that stands out on white and on black.
pub(super) const FOCUS_RING_COLOR: Color = Color::from_rgba_u8(0, 95, 204, 255);
/// The inner line of the ring, so the ring stands out on a blue control too.
pub(super) const FOCUS_RING_INNER_COLOR: Color = Color::from_rgba_u8(255, 255, 255, 255);
const OUTER_WIDTH: f32 = 2.0;
const INNER_WIDTH: f32 = 1.0;

/// The two lines of a ring around content of the given size, outer line
/// first, each with its stroke width. Both lines sit inside the content
/// bounds, so the ring never leaves the control's own area.
pub(super) fn focus_ring_lines(size: Size) -> [(Rect, f32); 2] {
    [
        (inset(size, OUTER_WIDTH / 2.0), OUTER_WIDTH),
        (inset(size, OUTER_WIDTH + INNER_WIDTH / 2.0), INNER_WIDTH),
    ]
}

fn inset(size: Size, by: f32) -> Rect {
    Rect {
        x: by,
        y: by,
        width: (size.width - 2.0 * by).max(0.0),
        height: (size.height - 2.0 * by).max(0.0),
    }
}

fn draw_focus_ring(scope: &mut dyn DrawScope) {
    let [(outer, outer_width), (inner, inner_width)] = focus_ring_lines(scope.size());
    scope.draw_rect_at_stroked(
        outer,
        Brush::solid(FOCUS_RING_COLOR),
        Stroke::new(outer_width),
    );
    scope.draw_rect_at_stroked(
        inner,
        Brush::solid(FOCUS_RING_INNER_COLOR),
        Stroke::new(inner_width),
    );
}

impl Modifier {
    /// A focus target that draws a ring while the keyboard put focus on it.
    ///
    /// The ring shows after Tab or an arrow key moved focus here and goes
    /// away on the next pointer press anywhere, the way a web browser's
    /// `:focus-visible` works. A control that draws its own focus look uses
    /// [`focus_target`](Self::focus_target) instead. Compose's
    /// `Modifier.focusable()`.
    pub fn focusable(self) -> Self {
        let element = FocusRingElement(FocusTargetElement::new());
        self.then(Self::from_dyn_element(modifier_element(element)))
    }
}

// One element for the focus target and its ring, equal from one composition
// to the next, so a recomposition keeps the node as it is.
#[derive(Clone, Debug, PartialEq)]
struct FocusRingElement(FocusTargetElement);

impl Hash for FocusRingElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "focusable".hash(state);
    }
}

impl ModifierNodeElement for FocusRingElement {
    type Node = FocusRingNode;

    fn create(&self) -> Self::Node {
        FocusRingNode {
            target: self.0.create(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        self.0.update(&mut node.target);
    }

    fn inspector_name(&self) -> &'static str {
        "focusable"
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::FOCUS | NodeCapabilities::DRAW
    }
}

// A focus target that draws its ring over its content, reading at draw time
// whether it holds focus.
struct FocusRingNode {
    target: FocusTargetNode,
}

impl DelegatableNode for FocusRingNode {
    fn node_state(&self) -> &NodeState {
        self.target.node_state()
    }
}

impl ModifierNode for FocusRingNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.target.on_attach(context);
    }

    fn on_detach(&mut self) {
        self.target.on_detach();
    }

    impl_draw_node!();
    impl_focus_node!();
}

impl FocusNode for FocusRingNode {
    fn focus_state(&self) -> FocusState {
        self.target.focus_state()
    }

    fn on_focus_changed(&mut self, context: &mut dyn ModifierNodeContext, state: FocusState) {
        self.target.on_focus_changed(context, state);
    }
}

impl DrawModifierNode for FocusRingNode {
    fn create_draw_closure(&self) -> Option<NodeDrawClosure> {
        let target = Rc::clone(self.target.shared());
        Some(Rc::new(move |scope: &mut DrawScopeDefault| {
            if target.is_focused() && keyboard_focus_visible() {
                draw_focus_ring(scope);
            }
        }))
    }
}

#[cfg(test)]
#[path = "tests/focus_ring_tests.rs"]
mod tests;
