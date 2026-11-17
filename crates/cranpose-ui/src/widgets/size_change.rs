//! Expand and shrink transitions: the size of the content of an
//! [`AnimatedVisibility`](crate::AnimatedVisibility) follows its progress.
//!
//! Mirrors Jetpack Compose's `expandIn`, `expandHorizontally`,
//! `expandVertically`, `shrinkOut`, `shrinkHorizontally` and
//! `shrinkVertically`. Content of a size that changes lays out at its full
//! size inside a box of the size that the transition gives it, so a parent
//! lays out again while the transition runs and its other children move with
//! the box.

use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState,
};
use cranpose_ui_graphics::Size;
use cranpose_ui_layout::{
    Alignment, HorizontalAlignment, LayoutDirection, LayoutModifierMeasureResult,
    VerticalAlignment, bias_offset, round_to_px,
};

use crate::{
    modifier::{Modifier, inspector_metadata},
    modifier_nodes::impl_layout_modifier_node,
    widgets::{
        animated_content::EntryMotion,
        animated_visibility::{EnterTransition, ExitTransition, TransitionEffects, enter, exit},
    },
};

// The axes along which a transition changes the size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axes {
    Both,
    Horizontal,
    Vertical,
}

impl Axes {
    // `full` with the extent along these axes taken away.
    fn collapse(self, full: Size) -> Size {
        match self {
            Self::Both => Size::ZERO,
            Self::Horizontal => Size::new(0.0, full.height),
            Self::Vertical => Size::new(full.width, 0.0),
        }
    }
}

// The size content has where its transition starts or ends, as a function
// of its full size.
#[derive(Clone)]
enum Away {
    // No extent along the axes, Compose's default of `{ 0 }`.
    Collapsed(Axes),
    Custom(Rc<dyn Fn(Size) -> Size>),
}

impl PartialEq for Away {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Collapsed(a), Self::Collapsed(b)) => a == b,
            (Self::Custom(a), Self::Custom(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

// How the size of content changes during a transition: from the size that
// `away` gives to its full size on an enter, and back on an exit.
#[derive(Clone)]
pub(crate) struct SizeChange {
    alignment: Alignment,
    pub(crate) clip: bool,
    away: Away,
}

impl std::fmt::Debug for SizeChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeChange")
            .field("alignment", &self.alignment)
            .field("clip", &self.clip)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SizeChange {
    fn eq(&self, other: &Self) -> bool {
        self.alignment == other.alignment && self.clip == other.clip && self.away == other.away
    }
}

impl SizeChange {
    fn new(alignment: Alignment, away: Away) -> Self {
        Self {
            alignment,
            clip: true,
            away,
        }
    }

    fn custom(alignment: Alignment, away: impl Fn(Size) -> Size + 'static) -> Self {
        Self::new(alignment, Away::Custom(Rc::new(away)))
    }

    // The size of content of size `full` at `progress` of the way to rest, a
    // whole number of device pixels short of `full`: content at rest is
    // exactly `full`.
    pub(crate) fn size_at(&self, full: Size, progress: f32, density: f32) -> Size {
        let away = match &self.away {
            Away::Collapsed(axes) => axes.collapse(full),
            Away::Custom(size) => size(full),
        };
        let between = |away: f32, full: f32| {
            (full - round_to_px((full - away) * (1.0 - progress), density)).max(0.0)
        };
        Size::new(
            between(away.width, full.width),
            between(away.height, full.height),
        )
    }

    // Where content of size `full` lies in a box of `size`: the alignment
    // puts the content's point of it on the box's. In a right-to-left
    // layout the start and the end swap.
    pub(crate) fn offset(
        &self,
        full: Size,
        size: Size,
        direction: LayoutDirection,
        density: f32,
    ) -> (f32, f32) {
        let horizontal = self.alignment.horizontal.bias();
        let horizontal = if direction.is_rtl() {
            -horizontal
        } else {
            horizontal
        };
        (
            bias_offset(horizontal, size.width, full.width, density),
            bias_offset(
                self.alignment.vertical.bias(),
                size.height,
                full.height,
                density,
            ),
        )
    }
}

fn horizontal(alignment: HorizontalAlignment) -> Alignment {
    Alignment::new(alignment, VerticalAlignment::CenterVertically)
}

fn vertical(alignment: VerticalAlignment) -> Alignment {
    Alignment::new(HorizontalAlignment::CenterHorizontally, alignment)
}

fn expand(change: SizeChange) -> EnterTransition {
    enter(TransitionEffects::sized(change))
}

fn shrink(change: SizeChange) -> ExitTransition {
    exit(TransitionEffects::sized(change))
}

/// Expands the content of an [`AnimatedVisibility`](crate::AnimatedVisibility)
/// from nothing to its full size, revealing it from its bottom end corner
/// (the bottom start corner in a right-to-left layout). Other children of
/// the parent move as the size grows.
///
/// Mirrors Jetpack Compose: `expandIn()`, with its defaults. The animation
/// spec is [`EnterTransition::with_animation`] and `clip` is
/// [`EnterTransition::with_clip`].
pub fn expand_in() -> EnterTransition {
    expand(SizeChange::new(
        Alignment::BOTTOM_END,
        Away::Collapsed(Axes::Both),
    ))
}

/// Expands the content from the size `initial_size` returns for its full
/// size to the full size. `expand_from` sets which part of the content shows
/// first.
///
/// Mirrors Jetpack Compose: `expandIn(expandFrom = expand_from, initialSize =
/// initial_size)`.
pub fn expand_in_with(
    expand_from: Alignment,
    initial_size: impl Fn(Size) -> Size + 'static,
) -> EnterTransition {
    expand(SizeChange::custom(expand_from, initial_size))
}

/// Shrinks the content from its full size to nothing, toward its bottom end
/// corner (the bottom start corner in a right-to-left layout), before it
/// leaves. Other children of the parent move as the size falls.
///
/// Mirrors Jetpack Compose: `shrinkOut()`, with its defaults. The animation
/// spec is [`ExitTransition::with_animation`] and `clip` is
/// [`ExitTransition::with_clip`].
pub fn shrink_out() -> ExitTransition {
    shrink(SizeChange::new(
        Alignment::BOTTOM_END,
        Away::Collapsed(Axes::Both),
    ))
}

/// Shrinks the content from its full size to the size `target_size` returns
/// for it. `shrink_towards` sets which part of the content stays in view
/// last.
///
/// Mirrors Jetpack Compose: `shrinkOut(shrinkTowards = shrink_towards,
/// targetSize = target_size)`.
pub fn shrink_out_with(
    shrink_towards: Alignment,
    target_size: impl Fn(Size) -> Size + 'static,
) -> ExitTransition {
    shrink(SizeChange::custom(shrink_towards, target_size))
}

/// Expands the content horizontally from no width to its full width,
/// revealing it from its end edge first.
///
/// Mirrors Jetpack Compose: `expandHorizontally()`, with its defaults.
pub fn expand_horizontally() -> EnterTransition {
    expand(SizeChange::new(
        horizontal(HorizontalAlignment::End),
        Away::Collapsed(Axes::Horizontal),
    ))
}

/// Expands the content horizontally from the width `initial_width` returns
/// for its full width to the full width, revealing it from `expand_from`
/// first.
///
/// Mirrors Jetpack Compose: `expandHorizontally(expandFrom = expand_from,
/// initialWidth = initial_width)`.
pub fn expand_horizontally_with(
    expand_from: HorizontalAlignment,
    initial_width: impl Fn(f32) -> f32 + 'static,
) -> EnterTransition {
    expand(SizeChange::custom(horizontal(expand_from), move |full| {
        Size::new(initial_width(full.width), full.height)
    }))
}

/// Expands the content vertically from no height to its full height,
/// revealing it from its bottom edge first.
///
/// Mirrors Jetpack Compose: `expandVertically()`, with its defaults.
pub fn expand_vertically() -> EnterTransition {
    expand(SizeChange::new(
        vertical(VerticalAlignment::Bottom),
        Away::Collapsed(Axes::Vertical),
    ))
}

/// Expands the content vertically from the height `initial_height` returns
/// for its full height to the full height, revealing it from `expand_from`
/// first.
///
/// Mirrors Jetpack Compose: `expandVertically(expandFrom = expand_from,
/// initialHeight = initial_height)`.
pub fn expand_vertically_with(
    expand_from: VerticalAlignment,
    initial_height: impl Fn(f32) -> f32 + 'static,
) -> EnterTransition {
    expand(SizeChange::custom(vertical(expand_from), move |full| {
        Size::new(full.width, initial_height(full.height))
    }))
}

/// Shrinks the content horizontally from its full width to no width, toward
/// its end edge, before it leaves.
///
/// Mirrors Jetpack Compose: `shrinkHorizontally()`, with its defaults.
pub fn shrink_horizontally() -> ExitTransition {
    shrink(SizeChange::new(
        horizontal(HorizontalAlignment::End),
        Away::Collapsed(Axes::Horizontal),
    ))
}

/// Shrinks the content horizontally from its full width to the width
/// `target_width` returns for it, toward `shrink_towards`.
///
/// Mirrors Jetpack Compose: `shrinkHorizontally(shrinkTowards =
/// shrink_towards, targetWidth = target_width)`.
pub fn shrink_horizontally_with(
    shrink_towards: HorizontalAlignment,
    target_width: impl Fn(f32) -> f32 + 'static,
) -> ExitTransition {
    shrink(SizeChange::custom(
        horizontal(shrink_towards),
        move |full| Size::new(target_width(full.width), full.height),
    ))
}

/// Shrinks the content vertically from its full height to no height, toward
/// its bottom edge, before it leaves.
///
/// Mirrors Jetpack Compose: `shrinkVertically()`, with its defaults.
pub fn shrink_vertically() -> ExitTransition {
    shrink(SizeChange::new(
        vertical(VerticalAlignment::Bottom),
        Away::Collapsed(Axes::Vertical),
    ))
}

/// Shrinks the content vertically from its full height to the height
/// `target_height` returns for it, toward `shrink_towards`.
///
/// Mirrors Jetpack Compose: `shrinkVertically(shrinkTowards = shrink_towards,
/// targetHeight = target_height)`.
pub fn shrink_vertically_with(
    shrink_towards: VerticalAlignment,
    target_height: impl Fn(f32) -> f32 + 'static,
) -> ExitTransition {
    shrink(SizeChange::custom(vertical(shrink_towards), move |full| {
        Size::new(full.width, target_height(full.height))
    }))
}

impl Modifier {
    // Sizes the box of an entry's content by its transition and places the
    // content in it.
    pub(crate) fn size_change(self, motion: Rc<EntryMotion>, direction: LayoutDirection) -> Self {
        let modifier = Self::with_element(SizeChangeElement { motion, direction })
            .with_inspector_metadata(inspector_metadata("sizeChange", |_| {}));
        self.then(modifier)
    }
}

// Equal to another only for the same entry, as its layout direction is.
#[derive(Clone)]
struct SizeChangeElement {
    motion: Rc<EntryMotion>,
    direction: LayoutDirection,
}

impl std::fmt::Debug for SizeChangeElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeChangeElement")
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SizeChangeElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.motion, &other.motion) && self.direction == other.direction
    }
}

impl Eq for SizeChangeElement {}

impl Hash for SizeChangeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.motion).hash(state);
        self.direction.hash(state);
    }
}

impl ModifierNodeElement for SizeChangeElement {
    type Node = SizeChangeNode;

    fn create(&self) -> Self::Node {
        SizeChangeNode {
            motion: Rc::clone(&self.motion),
            direction: self.direction,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.motion = Rc::clone(&self.motion);
        node.direction = self.direction;
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

struct SizeChangeNode {
    motion: Rc<EntryMotion>,
    direction: LayoutDirection,
    state: NodeState,
}

impl DelegatableNode for SizeChangeNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(SizeChangeNode, invalidate = InvalidationKind::Layout);

impl LayoutModifierNode for SizeChangeNode {
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        let placeable = measurable.measure(constraints);
        let full = Size::new(placeable.width(), placeable.height());
        self.motion
            .fit(full, constraints, context.density(), self.direction)
    }
}
