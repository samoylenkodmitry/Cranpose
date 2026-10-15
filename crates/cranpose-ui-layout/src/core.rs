//! Core layout traits and types shared by Compose UI widgets.

use std::{any::Any, fmt, rc::Rc};

use cranpose_core::NodeId;
use cranpose_ui_graphics::Size;

use crate::{Alignment, HorizontalAlignment, VerticalAlignment, constraints::Constraints};

/// Parent data for flex layouts (Row/Column weights and alignment).
#[derive(Clone, Copy, Debug, Default)]
pub struct FlexParentData {
    /// Weight for distributing remaining space in the main axis.
    /// If > 0.0, this child participates in weighted distribution.
    pub weight: f32,

    /// Whether to fill the allocated space when using weight.
    /// If true, child gets tight constraints; if false, child gets loose constraints.
    pub fill: bool,
}

impl FlexParentData {
    /// A child's share of the room left over, and whether it fills it.
    pub fn new(weight: f32, fill: bool) -> Self {
        Self { weight, fill }
    }

    /// Whether the child takes a share of the room left over.
    pub fn has_weight(&self) -> bool {
        self.weight > 0.0
    }
}

/// Data a child's modifier gives one kind of parent layout, which that
/// layout's measure policy reads by its type: a grid cell, or the data of
/// an app's own layout. Compose's `ParentDataModifierNode` data. Every
/// `'static` type with `Debug` and `PartialEq` is layout data.
pub trait LayoutData: Any + fmt::Debug {
    /// Whether `other` holds the same data, so a parent can keep its last
    /// measure.
    fn same_as(&self, other: &dyn LayoutData) -> bool;

    /// The data as `Any`, to read it by its type.
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any + fmt::Debug + PartialEq> LayoutData for T {
    fn same_as(&self, other: &dyn LayoutData) -> bool {
        other
            .as_any()
            .downcast_ref::<T>()
            .is_some_and(|other| other == self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Layout metadata supplied by a child for its direct parent.
///
/// Weight/fill apply to Row and Column. Alignment values override the
/// corresponding parent layout's default alignment for this child only.
#[derive(Clone, Debug, Default)]
pub struct ParentData {
    /// The child's share of a Row's or Column's room left over; 0 for none.
    pub weight: f32,
    /// Whether a weighted child fills its share or may be smaller.
    pub fill: bool,
    /// Where the child sits in a Box, over the Box's own alignment.
    pub box_alignment: Option<Alignment>,
    /// Where the child sits across a Row, over the Row's own alignment.
    pub row_alignment: Option<VerticalAlignment>,
    /// Aligns this child's first baseline with other baseline-aligned Row children.
    pub row_baseline: bool,
    /// Where the child sits across a Column, over the Column's own alignment.
    pub column_alignment: Option<HorizontalAlignment>,
    /// The fraction of the parent's width the child's outermost horizontal
    /// fill modifier asks for; 0 without one. A FlexBox reads it as the
    /// child's default grow. Compose's `FillModifierParentData`.
    pub fill_width_fraction: f32,
    /// The same for height.
    pub fill_height_fraction: f32,
    /// The data of the child's outermost [`LayoutData`] modifier, for a
    /// parent that reads it by its type.
    pub layout_data: Option<Rc<dyn LayoutData>>,
}

impl PartialEq for ParentData {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
            && self.fill == other.fill
            && self.box_alignment == other.box_alignment
            && self.row_alignment == other.row_alignment
            && self.row_baseline == other.row_baseline
            && self.column_alignment == other.column_alignment
            && self.fill_width_fraction == other.fill_width_fraction
            && self.fill_height_fraction == other.fill_height_fraction
            && match (&self.layout_data, &other.layout_data) {
                (None, None) => true,
                (Some(data), Some(other)) => Rc::ptr_eq(data, other) || data.same_as(&**other),
                _ => false,
            }
    }
}

impl From<FlexParentData> for ParentData {
    fn from(value: FlexParentData) -> Self {
        Self {
            weight: value.weight,
            fill: value.fill,
            ..Self::default()
        }
    }
}

impl ParentData {
    /// Whether the child takes a share of the room left over.
    pub fn has_weight(&self) -> bool {
        self.weight > 0.0
    }

    /// The child's [`LayoutData`] when it is a `T`.
    pub fn layout_data<T: Any>(&self) -> Option<&T> {
        self.layout_data.as_deref()?.as_any().downcast_ref()
    }
}

/// Object capable of measuring a layout child and exposing intrinsic sizes.
pub trait Measurable {
    /// Measures the child with the provided constraints, returning a [`Placeable`].
    fn measure(&self, constraints: Constraints) -> Placeable;

    /// The size the last measure gave its parent and the incoming
    /// constraints that measurement holds for: see
    /// [`crate::ConstraintsHold`]. `None`, the default, when it cannot tell.
    fn measured_hold(&self) -> Option<(Size, crate::ConstraintsHold)> {
        None
    }

    /// Returns the minimum width achievable for the given height.
    fn min_intrinsic_width(&self, height: f32) -> f32;

    /// Returns the maximum width achievable for the given height.
    fn max_intrinsic_width(&self, height: f32) -> f32;

    /// Returns the minimum height achievable for the given width.
    fn min_intrinsic_height(&self, width: f32) -> f32;

    /// Returns the maximum height achievable for the given width.
    fn max_intrinsic_height(&self, width: f32) -> f32;

    /// Returns flex parent data if this measurable has weight/fill properties.
    /// Default implementation returns None (no weight).
    fn flex_parent_data(&self) -> Option<FlexParentData> {
        None
    }

    /// Returns all metadata consumed by the direct parent layout.
    ///
    /// The default preserves the older `flex_parent_data` customization
    /// point so existing custom measurables continue to provide weights.
    fn parent_data(&self) -> ParentData {
        self.flex_parent_data().map(Into::into).unwrap_or_default()
    }
}

/// Result of running a measurement pass for a single child.
///
/// Carries measurement and alignment lines without allocating a trait object.
pub struct Placeable {
    width: f32,
    height: f32,
    node_id: NodeId,
    content_offset_x: f32,
    content_offset_y: f32,
    alignment_lines: crate::AlignmentLines,
    place_target: Option<Rc<dyn PlaceTarget>>,
}

/// What a node-backed [`Placeable`] tells when its parent places it.
pub trait PlaceTarget {
    /// Places the node at `(x, y)` in its parent.
    fn place(&self, x: f32, y: f32);
}

impl<F: Fn(f32, f32)> PlaceTarget for F {
    fn place(&self, x: f32, y: f32) {
        self(x, y);
    }
}

impl Placeable {
    /// Creates a pure-value placeable with no side effects on `place()`.
    pub fn value(width: f32, height: f32, node_id: NodeId) -> Self {
        Self {
            width,
            height,
            node_id,
            content_offset_x: 0.0,
            content_offset_y: 0.0,
            alignment_lines: crate::AlignmentLines::default(),
            place_target: None,
        }
    }

    /// Creates a pure-value placeable with a content offset.
    pub fn value_with_offset(
        width: f32,
        height: f32,
        node_id: NodeId,
        content_offset: (f32, f32),
    ) -> Self {
        Self {
            width,
            height,
            node_id,
            content_offset_x: content_offset.0,
            content_offset_y: content_offset.1,
            alignment_lines: crate::AlignmentLines::default(),
            place_target: None,
        }
    }

    /// Creates a node-backed placeable whose `place()` tells `target`. The
    /// target is shared, so a node's own measure state can be it without an
    /// allocation per measure.
    pub fn with_place_target(
        width: f32,
        height: f32,
        node_id: NodeId,
        target: Rc<dyn PlaceTarget>,
    ) -> Self {
        Self {
            width,
            height,
            node_id,
            content_offset_x: 0.0,
            content_offset_y: 0.0,
            alignment_lines: crate::AlignmentLines::default(),
            place_target: Some(target),
        }
    }

    /// Places the child at the provided coordinates relative to its parent.
    pub fn place(&self, x: f32, y: f32) {
        if let Some(target) = &self.place_target {
            target.place(x, y);
        }
    }

    /// Returns the measured width of the child.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Returns the measured height of the child.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Returns the identifier for the underlying layout node.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Returns the accumulated content offset from the coordinator chain.
    pub fn content_offset(&self) -> (f32, f32) {
        (self.content_offset_x, self.content_offset_y)
    }

    /// Supplies alignment lines relative to this placeable's top edge.
    pub fn with_alignment_lines(mut self, alignment_lines: crate::AlignmentLines) -> Self {
        self.alignment_lines = alignment_lines;
        self
    }

    /// Returns the text baselines relative to this placeable's top edge.
    pub fn alignment_lines(&self) -> crate::AlignmentLines {
        self.alignment_lines
    }
}

/// Scope for measurement operations.
///
/// This is Compose's `MeasureScope` -- the receiver `MeasurePolicy.measure` runs
/// on, which is what lets a measure pass see the density of the subtree it is
/// measuring rather than some process-wide default. There is no sensible
/// fallback value for either method: a policy that reads density must be given
/// the real grid, so both are required rather than defaulted.
pub trait MeasureScope {
    /// Returns the current density for converting Dp to pixels.
    fn density(&self) -> f32;

    /// Returns the current font scale for converting Sp to pixels.
    fn font_scale(&self) -> f32;
}

/// Policy responsible for measuring and placing children.
pub trait MeasurePolicy {
    /// Runs the measurement pass with the provided children and constraints.
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult;

    /// Runs measurement into caller-owned placement storage.
    ///
    /// The default preserves the public [`MeasurePolicy::measure`] contract for custom
    /// policies. Built-in policies override this to avoid allocating a fresh placement
    /// vector on every measure pass.
    fn measure_into(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> Measurement {
        let result = self.measure(scope, measurables, constraints);
        placements.clear();
        placements.extend(result.placements);
        Measurement {
            size: result.size,
            alignment_lines: result.alignment_lines,
        }
    }

    /// The incoming constraints the measure that sized the layout `size`
    /// under `constraints` holds for, when the policy can tell: see
    /// [`crate::ConstraintsHold`]. `None`, the default, holds for
    /// `constraints` alone.
    fn measure_hold(
        &self,
        _measurables: &[Box<dyn Measurable>],
        _constraints: Constraints,
        _size: Size,
    ) -> Option<crate::ConstraintsHold> {
        None
    }

    /// Computes the minimum intrinsic width of this policy.
    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;

    /// Computes the maximum intrinsic width of this policy.
    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;

    /// Computes the minimum intrinsic height of this policy.
    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;

    /// Computes the maximum intrinsic height of this policy.
    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;
}

/// The size and explicit alignment lines produced by a measure policy.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measurement {
    /// The size occupied by the measured layout.
    pub size: Size,
    /// Explicit baselines in this layout's coordinates; unspecified lines are inherited.
    pub alignment_lines: crate::AlignmentLines,
}

impl From<Size> for Measurement {
    fn from(size: Size) -> Self {
        Self {
            size,
            alignment_lines: crate::AlignmentLines::default(),
        }
    }
}

/// Result of a measurement operation.
#[derive(Clone, Debug)]
pub struct MeasureResult {
    /// The size the layout takes.
    pub size: Size,
    /// Explicit baselines in this layout's coordinates; unspecified lines are inherited.
    pub alignment_lines: crate::AlignmentLines,
    /// Where the children go.
    pub placements: Vec<Placement>,
}

impl MeasureResult {
    /// Creates a result from a size or a measurement that already has alignment lines.
    pub fn new(measurement: impl Into<Measurement>, placements: Vec<Placement>) -> Self {
        let measurement = measurement.into();
        Self {
            size: measurement.size,
            alignment_lines: measurement.alignment_lines,
            placements,
        }
    }

    /// Reports explicit lines relative to this layout's top edge.
    pub fn with_alignment_lines(mut self, alignment_lines: crate::AlignmentLines) -> Self {
        self.alignment_lines = alignment_lines;
        self
    }
}

/// Placement information for a measured child.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// The child placed.
    pub node_id: NodeId,
    /// Its left edge in the parent, in logical pixels.
    pub x: f32,
    /// Its top edge in the parent, in logical pixels.
    pub y: f32,
    /// Its draw order among its siblings: higher draws on top.
    pub z_index: i32,
}

impl Placement {
    /// `node_id` at `(x, y)` in the parent, drawn in `z_index` order.
    pub fn new(node_id: NodeId, x: f32, y: f32, z_index: i32) -> Self {
        Self {
            node_id,
            x,
            y,
            z_index,
        }
    }
}

/// Result of a layout modifier measurement operation.
///
/// Unlike `MeasureResult` which is for `MeasurePolicy` (multiple children),
/// this type is specifically for layout modifiers which wrap a single piece
/// of content and need to specify where that wrapped content should be placed.
#[derive(Clone, Copy, Debug)]
pub struct LayoutModifierMeasureResult {
    /// The size this modifier will occupy.
    pub size: Size,
    /// The offset at which to place the wrapped content relative to
    /// the top-left corner of this modifier's bounds.
    /// For example, PaddingNode returns (padding.left, padding.top) here
    /// to offset the child by the padding amount.
    pub placement_offset_x: f32,
    /// The vertical offset of the wrapped content, as `placement_offset_x`.
    pub placement_offset_y: f32,
    /// Explicit alignment lines. Unspecified lines are inherited from wrapped content.
    pub alignment_lines: crate::AlignmentLines,
}

impl LayoutModifierMeasureResult {
    /// A modifier of `size` that places its content at the offset.
    pub fn new(size: Size, placement_offset_x: f32, placement_offset_y: f32) -> Self {
        Self {
            size,
            placement_offset_x,
            placement_offset_y,
            alignment_lines: crate::AlignmentLines::default(),
        }
    }

    /// Creates a result with zero placement offset (wrapped content placed at 0,0).
    pub fn with_size(size: Size) -> Self {
        Self {
            size,
            placement_offset_x: 0.0,
            placement_offset_y: 0.0,
            alignment_lines: crate::AlignmentLines::default(),
        }
    }

    /// Overrides the wrapped content's alignment lines in this modifier's coordinates.
    pub fn with_alignment_lines(mut self, alignment_lines: crate::AlignmentLines) -> Self {
        self.alignment_lines = alignment_lines;
        self
    }
}

#[cfg(test)]
#[path = "tests/core_tests.rs"]
mod tests;
