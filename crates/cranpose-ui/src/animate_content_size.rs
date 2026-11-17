//! `Modifier::animate_content_size`: a size that follows its content's.
//!
//! Mirrors Jetpack Compose's `Modifier.animateContentSize`. The node
//! measures its content at the constraints it gets, then reports a size that
//! animates toward the content's, so a parent lays out again while the size
//! moves.

use std::{
    cell::RefCell,
    hash::{Hash, Hasher},
};

use cranpose_animation::{Animatable, AnimationType, Spring, spring};
use cranpose_core::{RuntimeHandle, current_runtime_handle};
use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState,
};
use cranpose_ui_graphics::Size;
use cranpose_ui_layout::{LayoutDirection, LayoutModifierMeasureResult, round_to_px};

use crate::{
    modifier::{Modifier, inspector_metadata},
    modifier_nodes::impl_layout_modifier_node,
};

impl Modifier {
    /// Animates the size of this modifier when the size of its content
    /// changes: the parent sees a size that moves to the new one, and the
    /// content is clipped to it. The content shows at the start of the
    /// animating size. The first size the content has appears without an
    /// animation. A change of the content while the size moves retargets the
    /// animation from the size that shows.
    ///
    /// Mirrors Jetpack Compose: `Modifier.animateContentSize()`, whose default
    /// animation is a spring of medium-low stiffness that does not bounce.
    /// The finished listener is absent.
    pub fn animate_content_size(self) -> Self {
        self.animate_content_size_with(spring(
            Spring::DampingRatioNoBouncy,
            Spring::StiffnessMediumLow,
        ))
    }

    /// [`animate_content_size`](Self::animate_content_size) that animates with
    /// `animation`: Compose's `animationSpec` argument.
    pub fn animate_content_size_with(self, animation: AnimationType) -> Self {
        let element = SizeAnimationElement {
            animation,
            direction: crate::layout_direction::layout_direction(),
            runtime: current_runtime_handle(),
        };
        let modifier = Self::with_element(element).with_inspector_metadata(inspector_metadata(
            "animateContentSize",
            move |info| {
                info.add_property("animationSpec", format!("{animation:?}"));
            },
        ));
        self.clip_to_bounds().then(modifier)
    }
}

// Equal to another with the same animation and layout direction: the runtime
// is where the animation runs, which a modifier built in one composition has
// the whole time.
#[derive(Clone)]
struct SizeAnimationElement {
    animation: AnimationType,
    direction: LayoutDirection,
    runtime: Option<RuntimeHandle>,
}

impl std::fmt::Debug for SizeAnimationElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeAnimationElement")
            .field("animation", &self.animation)
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SizeAnimationElement {
    fn eq(&self, other: &Self) -> bool {
        self.animation == other.animation && self.direction == other.direction
    }
}

impl Hash for SizeAnimationElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(&self.animation).hash(state);
        self.direction.hash(state);
    }
}

impl ModifierNodeElement for SizeAnimationElement {
    type Node = SizeAnimationNode;

    fn create(&self) -> Self::Node {
        SizeAnimationNode {
            animation: self.animation,
            direction: self.direction,
            runtime: self.runtime.clone(),
            size: RefCell::new(None),
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.animation = self.animation;
        node.direction = self.direction;
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

struct SizeAnimationNode {
    animation: AnimationType,
    direction: LayoutDirection,
    runtime: Option<RuntimeHandle>,
    // The size that shows, from the first measure on.
    size: RefCell<Option<Animatable<Size>>>,
    state: NodeState,
}

impl SizeAnimationNode {
    // The size that shows for content of size `target`: it starts at the
    // first size the content has and moves to each new one, a whole number of
    // device pixels short of it.
    fn shown(&self, target: Size, density: f32) -> Size {
        let animatable = {
            let mut size = self.size.borrow_mut();
            if size.is_none() {
                let runtime = self.runtime.clone().or_else(current_runtime_handle);
                *size = runtime.map(|runtime| Animatable::new(target, runtime));
            }
            size.clone()
        };
        let Some(mut animatable) = animatable else {
            return target;
        };
        if animatable.target() != target {
            animatable.animateTo(target, self.animation);
        }
        let value = animatable.state().value();
        Size::new(
            target.width - round_to_px(target.width - value.width, density),
            target.height - round_to_px(target.height - value.height, density),
        )
    }
}

impl DelegatableNode for SizeAnimationNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(SizeAnimationNode, invalidate = InvalidationKind::Layout);

impl LayoutModifierNode for SizeAnimationNode {
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        let placeable = measurable.measure(constraints);
        let target = Size::new(placeable.width(), placeable.height());
        let shown = self.shown(target, context.density());
        let (width, height) = constraints.constrain(shown.width, shown.height);
        // The content starts where the layout starts: at the right edge in a
        // right-to-left layout.
        let x = self.direction.place_x(0.0, width, target.width);
        LayoutModifierMeasureResult::new(Size::new(width, height), x, 0.0)
    }
}
