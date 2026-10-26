use cranpose_ui_layout::{Alignment, HorizontalAlignment, IntrinsicSize, VerticalAlignment};

use super::{Color, EdgeInsets, Point, RoundedCornerShape};

/// A background fill: its color and, when the corners are rounded, their shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedBackground {
    pub(super) color: Color,
    pub(super) shape: Option<RoundedCornerShape>,
}

impl ResolvedBackground {
    /// Creates a background of `color` with the corner `shape`, or square corners when `shape` is
    /// `None`.
    pub fn new(color: Color, shape: Option<RoundedCornerShape>) -> Self {
        Self { color, shape }
    }

    /// Returns the fill color.
    pub fn color(&self) -> Color {
        self.color
    }

    /// Returns the corner shape, or `None` for square corners.
    pub fn shape(&self) -> Option<RoundedCornerShape> {
        self.shape
    }

    /// Replaces the corner shape. `None` gives square corners.
    pub fn set_shape(&mut self, shape: Option<RoundedCornerShape>) {
        self.shape = shape;
    }
}

/// The padding, layout properties and offset that a modifier chain resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ResolvedModifiers {
    pub(super) padding: EdgeInsets,
    pub(super) layout: LayoutProperties,
    pub(super) offset: Point,
}

impl ResolvedModifiers {
    /// Returns the padding on each edge, in logical pixels.
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Returns the size, alignment and weight settings of the chain.
    pub fn layout_properties(&self) -> LayoutProperties {
        self.layout
    }

    /// Returns the offset of the content, in logical pixels.
    pub fn offset(&self) -> Point {
        self.offset
    }

    pub(crate) fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }

    pub(crate) fn set_layout_properties(&mut self, layout: LayoutProperties) {
        self.layout = layout;
    }

    pub(crate) fn set_offset(&mut self, offset: Point) {
        self.offset = offset;
    }

    /// These modifiers with every length on a whole device pixel of
    /// `density`, as Compose's `roundToPx` puts padding, sizes and offsets.
    pub(crate) fn on_device_grid(&self, density: f32) -> Self {
        use cranpose_ui_layout::round_to_px;

        let round = |value: Option<f32>| value.map(|value| round_to_px(value, density));
        let dimension = |dimension: DimensionConstraint| match dimension {
            DimensionConstraint::Points(value) => {
                DimensionConstraint::Points(round_to_px(value, density))
            }
            other => other,
        };
        Self {
            padding: crate::modifier_nodes::device_padding(self.padding, density),
            layout: LayoutProperties {
                padding: crate::modifier_nodes::device_padding(self.layout.padding, density),
                width: dimension(self.layout.width),
                height: dimension(self.layout.height),
                min_width: round(self.layout.min_width),
                min_height: round(self.layout.min_height),
                max_width: round(self.layout.max_width),
                max_height: round(self.layout.max_height),
                ..self.layout
            },
            offset: Point {
                x: round_to_px(self.offset.x, density),
                y: round_to_px(self.offset.y, density),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DimensionConstraint {
    #[default]
    Unspecified,
    Points(f32),
    Fraction(f32),
    Intrinsic(IntrinsicSize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutWeight {
    pub weight: f32,
    pub fill: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutProperties {
    pub(super) padding: EdgeInsets,
    pub(super) width: DimensionConstraint,
    pub(super) height: DimensionConstraint,
    pub(super) min_width: Option<f32>,
    pub(super) min_height: Option<f32>,
    pub(super) max_width: Option<f32>,
    pub(super) max_height: Option<f32>,
    pub(super) weight: Option<LayoutWeight>,
    pub(super) box_alignment: Option<Alignment>,
    pub(super) column_alignment: Option<HorizontalAlignment>,
    pub(super) row_alignment: Option<VerticalAlignment>,
    pub(super) row_baseline: bool,
}

impl LayoutProperties {
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    pub fn width(&self) -> DimensionConstraint {
        self.width
    }

    pub fn height(&self) -> DimensionConstraint {
        self.height
    }

    pub fn min_width(&self) -> Option<f32> {
        self.min_width
    }

    pub fn min_height(&self) -> Option<f32> {
        self.min_height
    }

    pub fn max_width(&self) -> Option<f32> {
        self.max_width
    }

    pub fn max_height(&self) -> Option<f32> {
        self.max_height
    }

    pub fn weight(&self) -> Option<LayoutWeight> {
        self.weight
    }

    pub fn box_alignment(&self) -> Option<Alignment> {
        self.box_alignment
    }

    pub fn column_alignment(&self) -> Option<HorizontalAlignment> {
        self.column_alignment
    }

    pub fn row_alignment(&self) -> Option<VerticalAlignment> {
        self.row_alignment
    }

    /// Whether the parent Row aligns this child's first baseline with its siblings.
    pub fn row_baseline(&self) -> bool {
        self.row_baseline
    }
}
