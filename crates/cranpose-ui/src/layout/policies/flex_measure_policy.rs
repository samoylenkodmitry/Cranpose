use std::cell::RefCell;

use cranpose_ui_graphics::whole;
use cranpose_ui_layout::{
    Axis, Constraints, MeasurePolicy, MeasureResult, MeasureScope, ParentData, Placement,
    bias_offset, round_to_px,
};
use smallvec::SmallVec;

use super::empty_measure_policy::empty_layout_hold;
use crate::{
    layout::core::{
        Arrangement, HorizontalAlignment, LinearArrangement, Measurable, VerticalAlignment,
    },
    thread_state,
};

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

// The child lists of a thread's rows and columns, kept between measures. A
// list on the stack took 548 bytes on the ESP32 at every nesting level a
// measure passes through, eight children inline.
pub(crate) struct SpareFlexChildren(RefCell<Vec<Vec<FlexChild>>>);

// Nested rows and columns each hold one list while they measure; a list over
// this many spares goes back to the heap.
const SPARE_LISTS: usize = 32;

impl SpareFlexChildren {
    pub(crate) const fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }

    fn take() -> Vec<FlexChild> {
        thread_state::with(|state| state.spare_flex_children.0.borrow_mut().pop())
            .unwrap_or_default()
    }

    fn give(mut children: Vec<FlexChild>) {
        children.clear();
        thread_state::with(|state| {
            let mut spares = state.spare_flex_children.0.borrow_mut();
            if spares.len() < SPARE_LISTS {
                spares.push(children);
            }
        });
    }
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

        let mut children = SpareFlexChildren::take();
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
        SpareFlexChildren::give(children);

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
