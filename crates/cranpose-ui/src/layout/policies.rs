use cranpose_ui_graphics::whole;
use cranpose_ui_layout::{
    Axis, Constraints, MeasurePolicy, MeasureResult, MeasureScope, ParentData, Placement,
    bias_offset, round_to_px,
};
use smallvec::SmallVec;

use crate::layout::core::{
    Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, VerticalAlignment,
};

/// MeasurePolicy for Box layout - overlays children according to alignment.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxMeasurePolicy {
    /// Where children sit inside the box when they are smaller than it.
    pub content_alignment: Alignment,
    /// Whether the box's minimum constraints pass on to its children. When `false`, children may
    /// measure smaller than the box's minimum size.
    pub propagate_min_constraints: bool,
    /// Direction used to resolve start and end alignment.
    pub layout_direction: crate::LayoutDirection,
}

impl BoxMeasurePolicy {
    /// Creates a policy that places children by `content_alignment` in a left-to-right layout
    /// direction.
    pub fn new(content_alignment: Alignment, propagate_min_constraints: bool) -> Self {
        Self {
            content_alignment,
            propagate_min_constraints,
            layout_direction: crate::LayoutDirection::Ltr,
        }
    }
}

impl MeasurePolicy for BoxMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, measurables, constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    /// A box holds while every child holds under the constraints it hands
    /// on and the largest child, which sizes the box, still fits.
    fn measure_hold(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        size: crate::modifier::Size,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        if measurables.is_empty() {
            return empty_layout_hold(constraints, size);
        }
        let mut width = cranpose_ui_layout::BoundRange::ANY;
        let mut height = cranpose_ui_layout::BoundRange::ANY;
        let mut held = cranpose_ui_layout::ConstraintsHold::sized(size.width, size.height);
        let mut largest = crate::modifier::Size::default();
        for measurable in measurables {
            let (child, hold) = measurable.measured_hold()?;
            if self.propagate_min_constraints {
                held = held.intersect(hold)?;
            } else {
                width = width.intersect(hold.width.max)?;
                height = height.intersect(hold.height.max)?;
            }
            largest.width = largest.width.max(child.width);
            largest.height = largest.height.max(child.height);
        }
        // Bounds that cut or stretched the box size it from themselves.
        if size != largest {
            return None;
        }
        held.width.max = held.width.max.intersect(width)?;
        held.height.max = held.height.max.intersect(height)?;
        Some(held)
    }

    /// A box reads only where each child aligns.
    fn reads_parent_data_change(&self, before: &ParentData, after: &ParentData) -> bool {
        before.box_alignment != after.box_alignment
    }

    /// A box whose constraints fix its size places a child at its top start
    /// at the origin, whatever the child's size.
    fn ignores_child_sizes(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> bool {
        constraints.min_width == constraints.max_width
            && constraints.min_height == constraints.max_height
            && !self.layout_direction.is_rtl()
            && measurables.iter().all(|measurable| {
                measurable
                    .parent_data()
                    .box_alignment
                    .unwrap_or(self.content_alignment)
                    == Alignment::TOP_START
            })
    }

    fn measure_into(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        let child_constraints = if self.propagate_min_constraints {
            constraints
        } else {
            Constraints {
                min_width: 0.0,
                max_width: constraints.max_width,
                min_height: 0.0,
                max_height: constraints.max_height,
            }
        };

        let mut max_width = 0.0_f32;
        let mut max_height = 0.0_f32;
        let mut placeables: SmallVec<[(cranpose_ui_layout::Placeable, Alignment); 8]> =
            SmallVec::new();

        for measurable in measurables {
            let placeable = measurable.measure(child_constraints);
            max_width = max_width.max(placeable.width());
            max_height = max_height.max(placeable.height());
            let alignment = measurable
                .parent_data()
                .box_alignment
                .unwrap_or(self.content_alignment);
            placeables.push((placeable, alignment));
        }

        let width = max_width.clamp(constraints.min_width, constraints.max_width);
        let height = max_height.clamp(constraints.min_height, constraints.max_height);

        placements.reserve(placeables.len());
        // In place: a by-value loop moves the list's inline storage, hundreds
        // of bytes, before its first step.
        for (placeable, alignment) in &placeables {
            let child_width = placeable.width();
            let child_height = placeable.height();

            let x = alignment
                .horizontal
                .align(width, child_width, scope.density());
            let x = self.layout_direction.place_x(x, width, child_width);
            let y = alignment
                .vertical
                .align(height, child_height, scope.density());

            placeable.place(x, y);
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
        }

        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_height(width))
            .fold(0.0, f32::max)
    }
}

/// Unified Flex layout policy that powers both Row and Column.
///
/// This policy implements Jetpack Compose's flex layout semantics:
/// - Measures children with proper loose constraints (min = 0 on both axes)
/// - Supports weighted distribution of remaining space
/// - Handles bounded/unbounded main axis correctly
/// - Implements correct intrinsics for both axes
///
/// ## Overflow Behavior
///
/// Like Jetpack Compose, this policy **allows children to overflow** their container bounds:
/// - Children can be positioned outside the parent's measured size
/// - Overflowing content is rendered (unless clipped by a modifier)
/// - When content overflows, distribution arrangements switch to `Start` to avoid negative spacing
/// - `SpacedBy` keeps its fixed inter-child spacing even when content overflows
///
/// Example: A Row with 300px of content in a 200px container will:
/// 1. Measure children at their natural sizes
/// 2. Detect overflow (300px > 200px)
/// 3. Switch to Start arrangement (pack children at the start)
/// 4. Position last children beyond the 200px boundary
///
/// To prevent overflow:
/// - Use weights for flexible sizing: `.weight(1.0, true)`
/// - Use `fillMaxWidth()`/`fillMaxHeight()` modifiers
/// - Design UI to fit within available space
/// - Add a clip modifier to hide overflowing content
///
/// ## Weighted Children
///
/// When the main axis is bounded and children have weights:
/// 1. Fixed children (no weight) are measured first
/// 2. Remaining space is distributed proportionally to weights
/// 3. Each weighted child gets: `remaining * (weight / total_weight)`
/// 4. If `fill=true`, child gets tight constraints; if `fill=false`, loose constraints
///
/// When the main axis is unbounded, weights are ignored (all children wrap content).
#[derive(Clone, Debug, PartialEq)]
pub struct FlexMeasurePolicy {
    /// Main axis direction (Horizontal for Row, Vertical for Column)
    pub axis: Axis,
    /// Arrangement along the main axis
    pub main_axis_arrangement: LinearArrangement,
    /// Alignment along the cross axis (used as default for children without explicit alignment)
    pub cross_axis_alignment: CrossAxisAlignment,
    /// The device pixel grid children are spaced and placed on, the
    /// composition's density.
    pub density: f32,
    /// Direction used to resolve horizontal order and start/end alignment.
    pub layout_direction: crate::LayoutDirection,
}

/// Compose's weight distribution in whole device pixels: each weighted
/// child's share of the remaining space rounded half up, and the pixels
/// that rounding gained or lost handed back one per child from the first.
pub(crate) struct WeightShares {
    density: f32,
    unit_px: f32,
    remainder_px: f32,
}

impl WeightShares {
    pub(crate) fn new(
        remaining: f32,
        weights: impl Iterator<Item = f32> + Clone,
        density: f32,
    ) -> Self {
        let density = if density > 0.0 && density.is_finite() {
            density
        } else {
            1.0
        };
        let total_weight: f32 = weights.clone().sum();
        let remaining_px = whole::round(remaining * density);
        let unit_px = if total_weight > 0.0 {
            remaining_px / total_weight
        } else {
            0.0
        };
        let rounded_px: f32 = weights
            .map(|weight| whole::floor(unit_px * weight + 0.5))
            .sum();
        Self {
            density,
            unit_px,
            remainder_px: remaining_px - rounded_px,
        }
    }

    /// The main-axis size of the next weighted child, which weighs `weight`.
    pub(crate) fn next_share(&mut self, weight: f32) -> f32 {
        let step = if self.remainder_px > 0.0 {
            1.0
        } else if self.remainder_px < 0.0 {
            -1.0
        } else {
            0.0
        };
        self.remainder_px -= step;
        (whole::floor(self.unit_px * weight + 0.5) + step).max(0.0) / self.density
    }
}

/// Cross-axis alignment for flex layouts.
/// This is axis-agnostic and gets interpreted based on the flex axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrossAxisAlignment {
    /// Align to the start of the cross axis (Top for Row, Start for Column)
    Start,
    /// Align to the center of the cross axis
    Center,
    /// Align to the end of the cross axis (Bottom for Row, End for Column)
    End,
}

impl CrossAxisAlignment {
    /// Where a child sits in `available` space: see [`bias_offset`].
    fn align(&self, available: f32, child: f32, density: f32) -> f32 {
        let bias = match self {
            Self::Start => -1.0,
            Self::Center => 0.0,
            Self::End => 1.0,
        };
        bias_offset(bias, available, child, density)
    }
}

impl From<HorizontalAlignment> for CrossAxisAlignment {
    fn from(alignment: HorizontalAlignment) -> Self {
        match alignment {
            HorizontalAlignment::Start => Self::Start,
            HorizontalAlignment::CenterHorizontally => Self::Center,
            HorizontalAlignment::End => Self::End,
        }
    }
}

impl From<VerticalAlignment> for CrossAxisAlignment {
    fn from(alignment: VerticalAlignment) -> Self {
        match alignment {
            VerticalAlignment::Top => Self::Start,
            VerticalAlignment::CenterVertically => Self::Center,
            VerticalAlignment::Bottom => Self::End,
        }
    }
}

impl FlexMeasurePolicy {
    /// A flex layout along `axis` on the device pixel grid of `density`.
    pub fn new(
        axis: Axis,
        main_axis_arrangement: LinearArrangement,
        cross_axis_alignment: CrossAxisAlignment,
        density: f32,
    ) -> Self {
        Self {
            axis,
            main_axis_arrangement,
            cross_axis_alignment,
            density,
            layout_direction: crate::LayoutDirection::Ltr,
        }
    }

    /// Creates a FlexMeasurePolicy for Row (horizontal main axis).
    pub fn row(
        horizontal_arrangement: LinearArrangement,
        vertical_alignment: VerticalAlignment,
        density: f32,
    ) -> Self {
        Self::new(
            Axis::Horizontal,
            horizontal_arrangement,
            vertical_alignment.into(),
            density,
        )
    }

    /// Creates a FlexMeasurePolicy for Column (vertical main axis).
    pub fn column(
        vertical_arrangement: LinearArrangement,
        horizontal_alignment: HorizontalAlignment,
        density: f32,
    ) -> Self {
        Self::new(
            Axis::Vertical,
            vertical_arrangement,
            horizontal_alignment.into(),
            density,
        )
    }

    fn get_axis_constraints(&self, constraints: Constraints) -> (f32, f32, f32, f32) {
        match self.axis {
            Axis::Horizontal => (
                constraints.min_width,
                constraints.max_width,
                constraints.min_height,
                constraints.max_height,
            ),
            Axis::Vertical => (
                constraints.min_height,
                constraints.max_height,
                constraints.min_width,
                constraints.max_width,
            ),
        }
    }

    fn make_constraints(
        &self,
        min_main: f32,
        max_main: f32,
        min_cross: f32,
        max_cross: f32,
    ) -> Constraints {
        match self.axis {
            Axis::Horizontal => Constraints {
                min_width: min_main,
                max_width: max_main,
                min_height: min_cross,
                max_height: max_cross,
            },
            Axis::Vertical => Constraints {
                min_width: min_cross,
                max_width: max_cross,
                min_height: min_main,
                max_height: max_main,
            },
        }
    }

    fn get_main_axis_size(&self, width: f32, height: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => width,
            Axis::Vertical => height,
        }
    }

    fn get_cross_axis_size(&self, width: f32, height: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => height,
            Axis::Vertical => width,
        }
    }

    /// Measures the unweighted children in order and returns Compose's
    /// `fixedSpace` with the largest cross size. Each child is offered only
    /// the main-axis space the ones before it leave, as
    /// `RowColumnMeasurementHelper` offers it, and the spacing after a child
    /// never runs past the main axis.
    fn measure_fixed_children(
        &self,
        measurables: &[Box<dyn Measurable>],
        children: &mut [FlexChild],
        (max_main, max_cross): (f32, f32),
        spacing: f32,
    ) -> (f32, f32) {
        let main_axis_bounded = max_main.is_finite();
        let mut fixed_space = 0.0_f32;
        let mut max_cross_size = 0.0_f32;
        for (child, measurable) in children.iter_mut().zip(measurables) {
            if child.data.has_weight() {
                continue;
            }
            let available_main = if main_axis_bounded {
                (max_main - fixed_space).max(0.0)
            } else {
                max_main
            };
            let placeable =
                measurable.measure(self.make_constraints(0.0, available_main, 0.0, max_cross));
            let main_size = self.get_main_axis_size(placeable.width(), placeable.height());
            let spacing_after = if main_axis_bounded {
                spacing.min(available_main - main_size).max(0.0)
            } else {
                spacing
            };
            fixed_space += main_size + spacing_after;
            max_cross_size =
                max_cross_size.max(self.get_cross_axis_size(placeable.width(), placeable.height()));
            child.placeable = Some(placeable);
        }
        (fixed_space, max_cross_size)
    }

    /// Compose's intrinsic size along the main axis: the sizes `main_size`
    /// gives of the unweighted children, the space in which each weighted
    /// child gets its own size, and the spacing.
    fn intrinsic_main_size(
        &self,
        measurables: &[Box<dyn Measurable>],
        main_size: impl Fn(&dyn Measurable) -> f32,
    ) -> f32 {
        if measurables.is_empty() {
            return 0.0;
        }
        let mut fixed_space = 0.0_f32;
        let mut weight_unit = 0.0_f32;
        let mut total_weight = 0.0_f32;
        for measurable in measurables {
            let size = main_size(measurable.as_ref());
            match child_weight(measurable.as_ref()) {
                Some(weight) => {
                    total_weight += weight;
                    weight_unit = weight_unit.max(round_to_px(size / weight, self.density));
                }
                None => fixed_space += size,
            }
        }
        round_to_px(weight_unit * total_weight, self.density)
            + fixed_space
            + self.get_spacing() * (measurables.len() - 1) as f32
    }

    /// Compose's intrinsic size across the main axis, in `main_available`
    /// main-axis space: each unweighted child asks for the main-axis size
    /// `main_size` gives it, up to what the children before it leave, and
    /// the weighted children share the rest. `cross_size` gives a child's
    /// cross size in the main-axis space it gets.
    fn intrinsic_cross_size(
        &self,
        measurables: &[Box<dyn Measurable>],
        main_available: f32,
        main_size: impl Fn(&dyn Measurable, f32) -> f32,
        cross_size: impl Fn(&dyn Measurable, f32) -> f32,
    ) -> f32 {
        if measurables.is_empty() {
            return 0.0;
        }
        let mut fixed_space =
            (self.get_spacing() * (measurables.len() - 1) as f32).min(main_available);
        let mut cross = 0.0_f32;
        let mut total_weight = 0.0_f32;
        for measurable in measurables {
            let measurable = measurable.as_ref();
            if let Some(weight) = child_weight(measurable) {
                total_weight += weight;
                continue;
            }
            let space =
                main_size(measurable, f32::INFINITY).min((main_available - fixed_space).max(0.0));
            fixed_space += space;
            cross = cross.max(cross_size(measurable, space));
        }
        if total_weight == 0.0 {
            return cross;
        }
        let weight_unit = if main_available.is_finite() {
            round_to_px(
                (main_available - fixed_space).max(0.0) / total_weight,
                self.density,
            )
        } else {
            f32::INFINITY
        };
        for measurable in measurables {
            let measurable = measurable.as_ref();
            if let Some(weight) = child_weight(measurable) {
                cross = cross.max(cross_size(
                    measurable,
                    round_to_px(weight_unit * weight, self.density),
                ));
            }
        }
        cross
    }

    fn get_spacing(&self) -> f32 {
        self.main_axis_arrangement.spacing(self.density)
    }

    fn measure_weighted_children(
        &self,
        measurables: &[Box<dyn Measurable>],
        children: &mut [FlexChild],
        (max_main, max_cross): (f32, f32),
        (fixed_space, spacing): (f32, f32),
    ) -> f32 {
        let weights = children
            .iter()
            .filter(|child| child.data.has_weight())
            .map(|child| child.data.weight);
        let weighted = weights.clone().count();
        if weighted == 0 {
            return 0.0;
        }
        let remaining_main = (max_main - fixed_space - spacing * (weighted - 1) as f32).max(0.0);
        let mut shares = WeightShares::new(remaining_main, weights, self.density);
        let mut max_cross_size = 0.0_f32;
        for (child, measurable) in children.iter_mut().zip(measurables) {
            let data = &child.data;
            if !data.has_weight() {
                continue;
            }
            let constraints = if max_main.is_finite() {
                let allocated = shares.next_share(data.weight);
                let minimum = if data.fill { allocated } else { 0.0 };
                self.make_constraints(minimum, allocated, 0.0, max_cross)
            } else {
                self.make_constraints(0.0, max_main, 0.0, max_cross)
            };
            let placeable = measurable.measure(constraints);
            max_cross_size =
                max_cross_size.max(self.get_cross_axis_size(placeable.width(), placeable.height()));
            child.placeable = Some(placeable);
        }
        max_cross_size
    }

    fn measured_extents(&self, children: &[FlexChild]) -> (f32, f32, f32) {
        let mut main = 0.0;
        let mut before = 0.0_f32;
        let mut after = 0.0_f32;
        for (placeable, data) in measured(children) {
            main += self.get_main_axis_size(placeable.width(), placeable.height());
            if self.axis == Axis::Horizontal
                && data.row_baseline
                && let Some(baseline) = placeable.alignment_lines().first_baseline()
            {
                before = before.max(baseline);
                after = after.max(placeable.height() - baseline);
            }
        }
        (main, before, after)
    }

    fn cross_axis_position(
        &self,
        placeable: &cranpose_ui_layout::Placeable,
        parent_data: &ParentData,
        container_cross: f32,
        before_baseline: f32,
    ) -> f32 {
        if self.axis == Axis::Horizontal && parent_data.row_baseline {
            return placeable
                .alignment_lines()
                .first_baseline()
                .map_or(0.0, |baseline| before_baseline - baseline);
        }
        let alignment = match self.axis {
            Axis::Horizontal => parent_data
                .row_alignment
                .map_or(self.cross_axis_alignment, Into::into),
            Axis::Vertical => parent_data
                .column_alignment
                .map_or(self.cross_axis_alignment, Into::into),
        };
        let child_cross = self.get_cross_axis_size(placeable.width(), placeable.height());
        alignment.align(container_cross, child_cross, self.density)
    }
}

/// The parent main-axis max range over which a held child keeps its size
/// after `fixed_space` taken by the children before it, with `spacing`
/// still fitting after it.
fn child_main_range(
    child_max: cranpose_ui_layout::BoundRange,
    max_main: f32,
    fixed_space: f32,
    child_main: f32,
    spacing: f32,
) -> Option<cranpose_ui_layout::BoundRange> {
    use cranpose_ui_layout::BoundRange;
    if !max_main.is_finite() {
        return child_max
            .contains(f32::INFINITY)
            .then_some(BoundRange::exactly(f32::INFINITY));
    }
    if (max_main - fixed_space).max(0.0) - child_main < spacing {
        return None;
    }
    child_max
        .outset(fixed_space)
        .intersect(BoundRange::from(fixed_space + child_main + spacing))
}

/// The weight of a child that shares the main axis by weight.
fn child_weight(measurable: &dyn Measurable) -> Option<f32> {
    measurable
        .flex_parent_data()
        .map(|data| data.weight)
        .filter(|weight| *weight > 0.0)
}

/// A child of a row or column while the row measures it: its parent data
/// and, once measured, its placeable.
struct FlexChild {
    data: ParentData,
    placeable: Option<cranpose_ui_layout::Placeable>,
}

/// The measured children with their parent data: every child, once both
/// measuring passes ran, since each child is fixed or weighted.
fn measured(
    children: &[FlexChild],
) -> impl Iterator<Item = (&cranpose_ui_layout::Placeable, &ParentData)> {
    children
        .iter()
        .filter_map(|child| Some((child.placeable.as_ref()?, &child.data)))
}

impl MeasurePolicy for FlexMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, measurables, constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    /// A row or column of unweighted children holds while each child holds
    /// under the main-axis space the ones before it leave, the spacing
    /// after each keeps its full width, and the bounds still fit the sum.
    fn measure_hold(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        size: crate::modifier::Size,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        use cranpose_ui_layout::{AxisHold, BoundRange, ConstraintsHold};
        if measurables.is_empty() {
            return empty_layout_hold(constraints, size);
        }
        if measurables
            .iter()
            .any(|measurable| measurable.parent_data().has_weight())
        {
            return None;
        }
        let (_, max_main, _, _) = self.get_axis_constraints(constraints);
        let spacing = self.get_spacing();
        let mut main = BoundRange::ANY;
        let mut cross = BoundRange::ANY;
        let mut fixed_space = 0.0_f32;
        let mut total_main = 0.0_f32;
        let mut cross_size = 0.0_f32;
        for measurable in measurables {
            let (child, hold) = measurable.measured_hold()?;
            let (main_hold, cross_hold) = match self.axis {
                Axis::Horizontal => (hold.width, hold.height),
                Axis::Vertical => (hold.height, hold.width),
            };
            let child_main = self.get_main_axis_size(child.width, child.height);
            main = main.intersect(child_main_range(
                main_hold.max,
                max_main,
                fixed_space,
                child_main,
                spacing,
            )?)?;
            cross = cross.intersect(cross_hold.max)?;
            fixed_space += child_main + spacing;
            total_main += child_main;
            cross_size = cross_size.max(self.get_cross_axis_size(child.width, child.height));
        }
        total_main += spacing * (measurables.len() - 1) as f32;
        let size_main = self.get_main_axis_size(size.width, size.height);
        let size_cross = self.get_cross_axis_size(size.width, size.height);
        // Bounds that cut or stretched the layout size it from themselves.
        if size_main != total_main || size_cross != cross_size {
            return None;
        }
        let main = AxisHold {
            min: BoundRange::up_to(size_main),
            max: main.intersect(BoundRange::from(size_main))?,
        };
        let cross = AxisHold {
            min: BoundRange::up_to(size_cross),
            max: cross.intersect(BoundRange::from(size_cross))?,
        };
        Some(match self.axis {
            Axis::Horizontal => ConstraintsHold {
                width: main,
                height: cross,
            },
            Axis::Vertical => ConstraintsHold {
                width: cross,
                height: main,
            },
        })
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        if measurables.is_empty() {
            let (width, height) = constraints.constrain(0.0, 0.0);
            return crate::modifier::Size { width, height }.into();
        }

        let (min_main, max_main, min_cross, max_cross) = self.get_axis_constraints(constraints);
        let spacing = self.get_spacing();

        // Filled in place: a collected list of eight inline children is a
        // 776-byte value moved out of `collect`.
        let mut children: SmallVec<[FlexChild; 8]> = SmallVec::new();
        children.extend(measurables.iter().map(|child| FlexChild {
            data: child.parent_data(),
            placeable: None,
        }));
        let (fixed_space, mut max_cross_size) =
            self.measure_fixed_children(measurables, &mut children, (max_main, max_cross), spacing);

        let num_children = measurables.len();
        let total_spacing = if num_children > 1 {
            spacing * (num_children - 1) as f32
        } else {
            0.0
        };

        max_cross_size = max_cross_size.max(self.measure_weighted_children(
            measurables,
            &mut children,
            (max_main, max_cross),
            (fixed_space, spacing),
        ));

        let (total_main, before_baseline, after_baseline) = self.measured_extents(&children);
        let total_main = total_main + total_spacing;
        max_cross_size = max_cross_size.max(before_baseline + after_baseline);

        let container_main = total_main.clamp(min_main, max_main);
        let container_cross = max_cross_size.clamp(min_cross, max_cross);

        let child_main_sizes: SmallVec<[f32; 8]> = measured(&children)
            .map(|(p, _)| self.get_main_axis_size(p.width(), p.height()))
            .collect();

        let mut main_positions: SmallVec<[f32; 8]> =
            SmallVec::with_capacity(child_main_sizes.len());
        main_positions.resize(child_main_sizes.len(), 0.0);

        let arrangement = if total_main > container_main && !self.main_axis_arrangement.is_spaced()
        {
            LinearArrangement::Start
        } else {
            self.main_axis_arrangement
        };
        arrangement.arrange(
            self.density,
            container_main,
            &child_main_sizes,
            &mut main_positions,
        );

        placements.reserve(children.len());
        for ((placeable, data), &main_pos) in measured(&children).zip(&main_positions) {
            let cross_pos =
                self.cross_axis_position(placeable, data, container_cross, before_baseline);

            let (x, y) = match self.axis {
                Axis::Horizontal => (main_pos, cross_pos),
                Axis::Vertical => (cross_pos, main_pos),
            };
            let width = match self.axis {
                Axis::Horizontal => container_main,
                Axis::Vertical => container_cross,
            };
            let x = self.layout_direction.place_x(x, width, placeable.width());

            placeable.place(x, y);
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
        }

        let (width, height) = match self.axis {
            Axis::Horizontal => (container_main, container_cross),
            Axis::Vertical => (container_cross, container_main),
        };

        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => {
                self.intrinsic_main_size(measurables, |m| m.min_intrinsic_width(height))
            }
            Axis::Vertical => self.intrinsic_cross_size(
                measurables,
                height,
                |m, width| m.max_intrinsic_height(width),
                |m, main| m.min_intrinsic_width(main),
            ),
        }
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => {
                self.intrinsic_main_size(measurables, |m| m.max_intrinsic_width(height))
            }
            Axis::Vertical => self.intrinsic_cross_size(
                measurables,
                height,
                |m, width| m.max_intrinsic_height(width),
                |m, main| m.max_intrinsic_width(main),
            ),
        }
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => self.intrinsic_cross_size(
                measurables,
                width,
                |m, height| m.max_intrinsic_width(height),
                |m, main| m.min_intrinsic_height(main),
            ),
            Axis::Vertical => {
                self.intrinsic_main_size(measurables, |m| m.min_intrinsic_height(width))
            }
        }
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => self.intrinsic_cross_size(
                measurables,
                width,
                |m, height| m.max_intrinsic_width(height),
                |m, main| m.max_intrinsic_height(main),
            ),
            Axis::Vertical => {
                self.intrinsic_main_size(measurables, |m| m.max_intrinsic_height(width))
            }
        }
    }
}

/// Which way a flow layout places its children before it wraps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowAxis {
    /// Left to right (right to left in a right-to-left layout), wrapping
    /// onto the next row: Compose's `FlowRow`.
    Horizontal,
    /// Top to bottom, wrapping onto the next column, which starts at the
    /// end side in a right-to-left layout: Compose's `FlowColumn`.
    Vertical,
}

/// MeasurePolicy for `FlowRow` and `FlowColumn`: children go along the main
/// axis and wrap onto the next line when the available length runs out
/// (Jetpack Compose `FlowRow`, `FlowColumn`).
///
/// - Children are measured with loose constraints (min = 0) capped at the
///   incoming max width and height, then packed along the main axis.
/// - A child that no longer fits on the current line starts a new line; a
///   child longer than the whole line gets a line of its own (and may
///   overflow, like Compose).
/// - `main_axis_spacing` separates children on the same line and
///   `cross_axis_spacing` separates lines; children align to the start of
///   their line.
/// - With an unbounded main axis everything stays on one line.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowMeasurePolicy {
    /// Which way children go before they wrap.
    pub axis: FlowAxis,
    /// Gap between adjacent children on the same line, in dp.
    pub main_axis_spacing: f32,
    /// Gap between consecutive lines, in dp.
    pub cross_axis_spacing: f32,
    /// Which side the horizontal axis starts at.
    pub layout_direction: crate::LayoutDirection,
}

impl FlowMeasurePolicy {
    /// Creates a policy along `axis` with the given gaps in logical pixels.
    /// Negative gaps count as zero. The layout direction is left to right.
    pub fn new(axis: FlowAxis, main_axis_spacing: f32, cross_axis_spacing: f32) -> Self {
        Self {
            axis,
            main_axis_spacing: main_axis_spacing.max(0.0),
            cross_axis_spacing: cross_axis_spacing.max(0.0),
            layout_direction: crate::LayoutDirection::Ltr,
        }
    }

    /// `(main, cross)` of a width and height, and back: the same swap.
    fn along(&self, width: f32, height: f32) -> (f32, f32) {
        match self.axis {
            FlowAxis::Horizontal => (width, height),
            FlowAxis::Vertical => (height, width),
        }
    }

    /// A child's intrinsic length along the main axis when the cross axis
    /// has `cross` room.
    fn main_intrinsic(&self, measurable: &dyn Measurable, cross: f32, min: bool) -> f32 {
        match (self.axis, min) {
            (FlowAxis::Horizontal, true) => measurable.min_intrinsic_width(cross),
            (FlowAxis::Horizontal, false) => measurable.max_intrinsic_width(cross),
            (FlowAxis::Vertical, true) => measurable.min_intrinsic_height(cross),
            (FlowAxis::Vertical, false) => measurable.max_intrinsic_height(cross),
        }
    }

    /// A child's intrinsic length across the main axis when it takes `main`.
    fn cross_intrinsic(&self, measurable: &dyn Measurable, main: f32, min: bool) -> f32 {
        match (self.axis, min) {
            (FlowAxis::Horizontal, true) => measurable.min_intrinsic_height(main),
            (FlowAxis::Horizontal, false) => measurable.max_intrinsic_height(main),
            (FlowAxis::Vertical, true) => measurable.min_intrinsic_width(main),
            (FlowAxis::Vertical, false) => measurable.max_intrinsic_width(main),
        }
    }

    /// The cross-axis length the children take when they wrap at
    /// `available_main`.
    fn wrapped_cross(
        &self,
        measurables: &[Box<dyn Measurable>],
        available_main: f32,
        min: bool,
    ) -> f32 {
        let mut cursor = 0.0_f32;
        let mut line_start = 0.0_f32;
        let mut line_cross = 0.0_f32;

        for measurable in measurables {
            let child_main = self.main_intrinsic(measurable.as_ref(), f32::INFINITY, false);
            let child_cross = self.cross_intrinsic(measurable.as_ref(), child_main, min);

            if cursor > 0.0 && cursor + self.main_axis_spacing + child_main > available_main {
                line_start += line_cross + self.cross_axis_spacing;
                cursor = 0.0;
                line_cross = 0.0;
            }
            cursor += if cursor > 0.0 {
                self.main_axis_spacing + child_main
            } else {
                child_main
            };
            line_cross = line_cross.max(child_cross);
        }

        line_start + line_cross
    }

    /// The main-axis length of every child on one line.
    fn unwrapped_main(&self, measurables: &[Box<dyn Measurable>], cross: f32) -> f32 {
        let total_spacing = self.main_axis_spacing * measurables.len().saturating_sub(1) as f32;
        measurables
            .iter()
            .map(|m| self.main_intrinsic(m.as_ref(), cross, false))
            .sum::<f32>()
            + total_spacing
    }

    /// The longest child's smallest main-axis length: a line each.
    fn narrowest_main(&self, measurables: &[Box<dyn Measurable>], cross: f32) -> f32 {
        measurables
            .iter()
            .map(|m| self.main_intrinsic(m.as_ref(), cross, true))
            .fold(0.0, f32::max)
    }
}

impl MeasurePolicy for FlowMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, measurables, constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        if measurables.is_empty() {
            let (width, height) = constraints.constrain(0.0, 0.0);
            return crate::modifier::Size { width, height }.into();
        }

        let child_constraints = Constraints {
            min_width: 0.0,
            max_width: constraints.max_width,
            min_height: 0.0,
            max_height: constraints.max_height,
        };

        let placeables: SmallVec<[cranpose_ui_layout::Placeable; 8]> = measurables
            .iter()
            .map(|measurable| measurable.measure(child_constraints))
            .collect();

        let (max_main, _) = self.along(constraints.max_width, constraints.max_height);
        let mut cursor = 0.0_f32;
        let mut line_start = 0.0_f32;
        let mut line_cross = 0.0_f32;
        let mut longest_line = 0.0_f32;

        placements.reserve(placeables.len());
        for placeable in &placeables {
            let (child_main, child_cross) = self.along(placeable.width(), placeable.height());

            if cursor > 0.0 && cursor + self.main_axis_spacing + child_main > max_main {
                longest_line = longest_line.max(cursor);
                line_start += line_cross + self.cross_axis_spacing;
                cursor = 0.0;
                line_cross = 0.0;
            }

            let main = if cursor > 0.0 {
                cursor + self.main_axis_spacing
            } else {
                0.0
            };
            let (x, y) = self.along(main, line_start);
            if !self.layout_direction.is_rtl() {
                placeable.place(x, y);
            }
            placements.push(Placement::new(placeable.node_id(), x, y, 0));

            cursor = main + child_main;
            line_cross = line_cross.max(child_cross);
        }
        longest_line = longest_line.max(cursor);

        let (width, height) = self.along(longest_line, line_start + line_cross);
        let width = width.clamp(constraints.min_width, constraints.max_width);
        let height = height.clamp(constraints.min_height, constraints.max_height);
        if self.layout_direction.is_rtl() {
            for (placement, placeable) in placements.iter_mut().zip(&placeables) {
                placement.x = self
                    .layout_direction
                    .place_x(placement.x, width, placeable.width());
                placeable.place(placement.x, placement.y);
            }
        }
        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        match self.axis {
            FlowAxis::Horizontal => self.narrowest_main(measurables, height),
            FlowAxis::Vertical => self.wrapped_cross(measurables, height, true),
        }
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        match self.axis {
            FlowAxis::Horizontal => self.unwrapped_main(measurables, height),
            FlowAxis::Vertical => self.wrapped_cross(measurables, height, false),
        }
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        match self.axis {
            FlowAxis::Horizontal => self.wrapped_cross(measurables, width, true),
            FlowAxis::Vertical => self.narrowest_main(measurables, width),
        }
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        match self.axis {
            FlowAxis::Horizontal => self.wrapped_cross(measurables, width, false),
            FlowAxis::Vertical => self.unwrapped_main(measurables, width),
        }
    }
}

/// MeasurePolicy for leaf nodes with fixed intrinsic size (like Spacer).
/// This policy respects the provided constraints but has a preferred intrinsic size.
#[derive(Clone, Debug, PartialEq)]
pub struct LeafMeasurePolicy {
    /// Preferred size in logical pixels. The measured size is this size limited by the incoming
    /// constraints.
    pub intrinsic_size: crate::modifier::Size,
}

impl LeafMeasurePolicy {
    /// Creates a policy whose preferred size is `intrinsic_size`.
    pub fn new(intrinsic_size: crate::modifier::Size) -> Self {
        Self { intrinsic_size }
    }
}

impl MeasurePolicy for LeafMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, &[], constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        let (width, height) =
            constraints.constrain(self.intrinsic_size.width, self.intrinsic_size.height);

        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.intrinsic_size.width
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.intrinsic_size.width
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.intrinsic_size.height
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.intrinsic_size.height
    }
}

/// EmptyMeasurePolicy that delegates all measurement to modifier nodes.
///
/// This is used when a Layout has no child layout logic - all measurement
/// is handled by modifier nodes (e.g., TextModifierNode for Text widgets).
/// Matches Jetpack Compose's EmptyMeasurePolicy pattern used in BasicText.
#[derive(Clone, Debug, PartialEq)]
pub struct EmptyMeasurePolicy;

impl EmptyMeasurePolicy {
    /// Creates the policy.
    pub fn new() -> Self {
        Self
    }
}

impl Default for EmptyMeasurePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl MeasurePolicy for EmptyMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, &[], constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        let (width, height) = constraints.constrain(0.0, 0.0);

        crate::modifier::Size { width, height }.into()
    }

    fn measure_hold(
        &self,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        size: crate::modifier::Size,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        empty_layout_hold(constraints, size)
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        0.0
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        0.0
    }
}

#[cfg(test)]
#[path = "tests/policies_tests.rs"]
mod tests;

/// What a layout with nothing in it holds for: it takes its min
/// constraints, so any constraints with the same mins and a max that allows
/// them size it the same.
fn empty_layout_hold(
    constraints: Constraints,
    size: crate::modifier::Size,
) -> Option<cranpose_ui_layout::ConstraintsHold> {
    (size.width == constraints.min_width && size.height == constraints.min_height)
        .then(|| cranpose_ui_layout::ConstraintsHold::at_min(size.width, size.height))
}
