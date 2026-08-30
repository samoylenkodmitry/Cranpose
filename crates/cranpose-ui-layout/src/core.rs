//! Core layout traits and types shared by Compose UI widgets.

use std::rc::Rc;

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
    pub fn new(weight: f32, fill: bool) -> Self {
        Self { weight, fill }
    }

    pub fn has_weight(&self) -> bool {
        self.weight > 0.0
    }
}

/// Layout metadata supplied by a child for its direct parent.
///
/// Weight/fill apply to Row and Column. Alignment values override the
/// corresponding parent layout's default alignment for this child only.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParentData {
    pub weight: f32,
    pub fill: bool,
    pub box_alignment: Option<Alignment>,
    pub row_alignment: Option<VerticalAlignment>,
    pub column_alignment: Option<HorizontalAlignment>,
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
    pub fn has_weight(&self) -> bool {
        self.weight > 0.0
    }
}

/// Object capable of measuring a layout child and exposing intrinsic sizes.
pub trait Measurable {
    /// Measures the child with the provided constraints, returning a [`Placeable`].
    fn measure(&self, constraints: Constraints) -> Placeable;

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
/// Concrete struct replacing the former `dyn Placeable` trait object.
/// This avoids a heap allocation per node per measure pass — the hot
/// coordinator path (16-byte value) now lives entirely on the stack.
pub struct Placeable {
    width: f32,
    height: f32,
    node_id: NodeId,
    content_offset_x: f32,
    content_offset_y: f32,
    place_fn: Option<Rc<dyn Fn(f32, f32)>>,
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
            place_fn: None,
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
            place_fn: None,
        }
    }

    /// Creates a node-backed placeable whose `place()` triggers a side effect.
    pub fn with_place_fn(
        width: f32,
        height: f32,
        node_id: NodeId,
        place_fn: Rc<dyn Fn(f32, f32)>,
    ) -> Self {
        Self {
            width,
            height,
            node_id,
            content_offset_x: 0.0,
            content_offset_y: 0.0,
            place_fn: Some(place_fn),
        }
    }

    /// Places the child at the provided coordinates relative to its parent.
    pub fn place(&self, x: f32, y: f32) {
        if let Some(f) = &self.place_fn {
            f(x, y);
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
    ) -> Size {
        let result = self.measure(scope, measurables, constraints);
        placements.clear();
        placements.extend(result.placements);
        result.size
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

/// Result of a measurement operation.
#[derive(Clone, Debug)]
pub struct MeasureResult {
    pub size: Size,
    pub placements: Vec<Placement>,
}

impl MeasureResult {
    pub fn new(size: Size, placements: Vec<Placement>) -> Self {
        Self { size, placements }
    }
}

/// Placement information for a measured child.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub node_id: NodeId,
    pub x: f32,
    pub y: f32,
    pub z_index: i32,
}

impl Placement {
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
    pub placement_offset_y: f32,
}

impl LayoutModifierMeasureResult {
    pub fn new(size: Size, placement_offset_x: f32, placement_offset_y: f32) -> Self {
        Self {
            size,
            placement_offset_x,
            placement_offset_y,
        }
    }

    /// Creates a result with zero placement offset (wrapped content placed at 0,0).
    pub fn with_size(size: Size) -> Self {
        Self {
            size,
            placement_offset_x: 0.0,
            placement_offset_y: 0.0,
        }
    }
}
