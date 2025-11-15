//! Core layout traits and types shared by Compose UI widgets.

use crate::constraints::Constraints;
use compose_core::NodeId;
use compose_ui_graphics::Size;

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

/// Object capable of measuring a layout child and exposing intrinsic sizes.
pub trait Measurable {
    /// Measures the child with the provided constraints, returning a [`Placeable`].
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable>;

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
}

/// Result of running a measurement pass for a single child.
pub trait Placeable {
    /// Places the child at the provided coordinates relative to its parent.
    fn place(&self, x: f32, y: f32);

    /// Returns the measured width of the child.
    fn width(&self) -> f32;

    /// Returns the measured height of the child.
    fn height(&self) -> f32;

    /// Returns the identifier for the underlying layout node.
    fn node_id(&self) -> NodeId;
}

/// Scope for measurement operations.
pub trait MeasureScope {
    /// Returns the current density for converting Dp to pixels.
    fn density(&self) -> f32 {
        1.0
    }

    /// Returns the current font scale for converting Sp to pixels.
    fn font_scale(&self) -> f32 {
        1.0
    }
}

/// Policy responsible for measuring and placing children.
pub trait MeasurePolicy {
    /// Runs the measurement pass with the provided children and constraints.
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult;

    /// Computes the minimum intrinsic width of this policy.
    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;

    /// Computes the maximum intrinsic width of this policy.
    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32;

    /// Computes the minimum intrinsic height of this policy.
    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;

    /// Computes the maximum intrinsic height of this policy.
    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32;

    /// Returns the text content if this is a text measure policy.
    /// Used for extracting text content for rendering and semantics.
    fn text_content(&self) -> Option<String> {
        None
    }
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
