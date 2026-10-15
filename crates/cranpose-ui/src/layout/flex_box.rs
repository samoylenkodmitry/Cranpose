//! The measure policy of `FlexBox`, after Compose's `FlexBoxMeasurePolicy`:
//! children take their base sizes, break into lines, grow or shrink on
//! each line, and align along and across it.

use cranpose_ui_layout::{
    Constraints, MeasurePolicy, MeasureResult, MeasureScope, Placeable, Placement, round_to_px,
};
use smallvec::SmallVec;

use crate::{
    layout::core::Measurable,
    widgets::flex_box::{
        FlexAlignContent, FlexAlignItems, FlexAlignSelf, FlexBasis, FlexBoxConfig, FlexConfig,
        FlexJustifyContent,
    },
};

/// Room a sum of sizes may pass a limit by and still fit: float sums of
/// whole pixels carry rounding error.
const FIT_EPSILON: f32 = 1e-3;

/// Lays out the children of a `FlexBox` by its config.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FlexBoxMeasurePolicy {
    pub(crate) config: FlexBoxConfig,
    pub(crate) layout_direction: crate::LayoutDirection,
    /// Device pixels per dp, which gaps and sizes round to.
    pub(crate) density: f32,
}

/// The incoming constraints along the main axis and across it.
#[derive(Clone, Copy)]
struct AxisConstraints {
    main_min: f32,
    main_max: f32,
    cross_min: f32,
    cross_max: f32,
}

impl AxisConstraints {
    fn of(constraints: Constraints, horizontal: bool) -> Self {
        if horizontal {
            Self {
                main_min: constraints.min_width,
                main_max: constraints.max_width,
                cross_min: constraints.min_height,
                cross_max: constraints.max_height,
            }
        } else {
            Self {
                main_min: constraints.min_height,
                main_max: constraints.max_height,
                cross_min: constraints.min_width,
                cross_max: constraints.max_width,
            }
        }
    }
}

/// One child and the state the flex algorithm keeps for it.
struct Item {
    child: usize,
    config: FlexConfig,
    grow: f32,
    min_main: f32,
    flex_base: f32,
    hypothetical: f32,
    target: f32,
    frozen: bool,
    placeable: Option<Placeable>,
    baseline: Option<f32>,
    cross_size: f32,
    main_position: f32,
    cross_position: f32,
}

/// The children `start..end` of the sorted items, on one line.
struct Line {
    start: usize,
    end: usize,
    main_size: f32,
    cross_size: f32,
    cross_start: f32,
    max_above_baseline: f32,
}

impl Line {
    fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            main_size: 0.0,
            cross_size: 0.0,
            cross_start: 0.0,
            max_above_baseline: 0.0,
        }
    }
}

type Items = SmallVec<[Item; 8]>;
type Lines = SmallVec<[Line; 4]>;

/// How lines break: the box's main room and the gap along it.
struct Breaking {
    available: f32,
    gap: f32,
}

impl FlexBoxMeasurePolicy {
    fn horizontal(&self) -> bool {
        self.config.direction.is_horizontal()
    }

    fn px(&self, value: f32) -> f32 {
        round_to_px(value, self.density)
    }

    /// `value` cut to whole device pixels toward zero, as Compose's integer
    /// division cuts it.
    fn truncate_px(&self, value: f32) -> f32 {
        (value * self.density).trunc() / self.density
    }

    /// The gaps along the main axis and across it.
    fn gaps(&self) -> (f32, f32) {
        let (row, column) = (
            self.px(self.config.row_gap),
            self.px(self.config.column_gap),
        );
        if self.horizontal() {
            (column, row)
        } else {
            (row, column)
        }
    }

    fn main_intrinsic(&self, measurable: &dyn Measurable, cross: f32, min: bool) -> f32 {
        match (self.horizontal(), min) {
            (true, true) => measurable.min_intrinsic_width(cross),
            (true, false) => measurable.max_intrinsic_width(cross),
            (false, true) => measurable.min_intrinsic_height(cross),
            (false, false) => measurable.max_intrinsic_height(cross),
        }
    }

    fn cross_intrinsic(&self, measurable: &dyn Measurable, main: f32, min: bool) -> f32 {
        match (self.horizontal(), min) {
            (true, true) => measurable.min_intrinsic_height(main),
            (true, false) => measurable.max_intrinsic_height(main),
            (false, true) => measurable.min_intrinsic_width(main),
            (false, false) => measurable.max_intrinsic_width(main),
        }
    }

    /// Whether a child aligns by its baseline.
    fn aligns_by_baseline(&self, config: &FlexConfig) -> bool {
        config.align_self == FlexAlignSelf::Baseline
            || (config.align_self == FlexAlignSelf::Auto
                && self.config.align_items == FlexAlignItems::Baseline)
    }

    /// Whether a child takes its line's cross size.
    fn stretches(&self, config: &FlexConfig) -> bool {
        config.align_self == FlexAlignSelf::Stretch
            || (config.align_self == FlexAlignSelf::Auto
                && self.config.align_items == FlexAlignItems::Stretch)
    }

    /// A child's config, grow and base sizes. Compose's `createFlexItem`.
    fn item(&self, child: usize, measurable: &dyn Measurable, axes: AxisConstraints) -> Item {
        let data = measurable.parent_data();
        let config = data
            .layout_data::<FlexConfig>()
            .copied()
            .unwrap_or_default();
        let fill = if self.horizontal() {
            data.fill_width_fraction
        } else {
            data.fill_height_fraction
        };
        let grow = config.grow.unwrap_or_else(|| fill.max(0.0));
        let min_main = self.main_intrinsic(measurable, axes.cross_max, true);
        let flex_base = match config.basis {
            FlexBasis::Dp(size) => self.px(size.max(0.0)),
            FlexBasis::Percent(fraction) if axes.main_max.is_finite() && !fraction.is_nan() => {
                self.truncate_px(axes.main_max * fraction)
            }
            FlexBasis::Percent(_) | FlexBasis::Auto => {
                self.main_intrinsic(measurable, axes.cross_max, false)
            }
        };
        let hypothetical = flex_base.max(min_main);
        Item {
            child,
            config,
            grow,
            min_main,
            flex_base,
            hypothetical,
            target: hypothetical,
            frozen: false,
            placeable: None,
            baseline: None,
            cross_size: 0.0,
            main_position: 0.0,
            cross_position: 0.0,
        }
    }

    /// Breaks the items into lines and resolves each line's main sizes, and
    /// its cross size when `upfront` asks for it before the items measure.
    /// Returns the lines and the sum of their cross sizes.
    fn build_lines(
        &self,
        items: &mut [Item],
        measurables: &[Box<dyn Measurable>],
        axes: AxisConstraints,
        (main_gap, cross_gap): (f32, f32),
        upfront: bool,
    ) -> (Lines, f32) {
        let wraps = self.config.wrap.wraps();
        let mut lines = Lines::new();
        let mut line_main = 0.0_f32;
        let mut cross_position = 0.0_f32;
        let mut total_cross = 0.0_f32;
        let mut line_start = 0;
        let mut remaining_cross = axes.cross_max;
        let breaking = Breaking {
            available: axes.main_max,
            gap: main_gap,
        };
        for index in 0..=items.len() {
            let breaks_here = index == items.len()
                || (wraps
                    && index > line_start
                    && (index - line_start >= self.config.max_items_in_each_line
                        || line_main + items[index].hypothetical > axes.main_max + FIT_EPSILON));
            if !breaks_here {
                line_main += items[index].hypothetical + main_gap;
                continue;
            }
            if line_start == index {
                break;
            }
            let mut line = Line::new(line_start, index);
            line.main_size = self.resolve_flexible_lengths(
                &mut items[line_start..index],
                line_main - main_gap,
                &breaking,
            );
            if upfront {
                self.line_cross_size(&mut line, items, measurables, remaining_cross);
            }
            total_cross += line.cross_size;
            line.cross_start = cross_position;
            cross_position += line.cross_size + cross_gap;
            remaining_cross = (remaining_cross - (line.cross_size + cross_gap)).max(0.0);
            lines.push(line);
            line_start = index;
            if let Some(item) = items.get(index) {
                line_main = item.hypothetical + main_gap;
            }
        }
        (lines, total_cross)
    }

    /// Grows or shrinks one line's items into its room and returns the
    /// line's main size. Compose's `resolveFlexibleLengths`.
    fn resolve_flexible_lengths(
        &self,
        items: &mut [Item],
        hypothetical_line: f32,
        breaking: &Breaking,
    ) -> f32 {
        let total_gap = breaking.gap * items.len().saturating_sub(1) as f32;
        let container = breaking.available;
        if !container.is_finite() {
            for item in items.iter_mut() {
                item.target = item.hypothetical;
            }
            return total_gap + items.iter().map(|item| item.target).sum::<f32>();
        }
        let growing = hypothetical_line < container;
        let mut unfrozen = 0_usize;
        let (mut sum_grow, mut sum_scaled_shrink, mut sum_factors) = (0.0_f32, 0.0_f32, 0.0_f32);
        let (mut sum_base, mut sum_frozen) = (0.0_f32, 0.0_f32);
        for item in items.iter_mut() {
            let factor = if growing {
                item.grow
            } else {
                item.config.shrink
            };
            if factor == 0.0 || (!growing && item.flex_base < item.hypothetical) {
                item.target = item.hypothetical;
                item.frozen = true;
                sum_frozen += item.target;
            } else {
                item.frozen = false;
                unfrozen += 1;
                sum_base += item.flex_base;
                sum_factors += factor;
                if growing {
                    sum_grow += item.grow;
                } else {
                    sum_scaled_shrink += item.config.shrink * item.flex_base;
                }
            }
        }
        let initial_free = container - total_gap - sum_frozen - sum_base;
        // With factors that add up to less than 1, a line hands out only
        // that fraction of its free room.
        let free_room = |sum_frozen: f32, sum_base: f32, sum_factors: f32| {
            let free = container - (total_gap + sum_frozen + sum_base);
            let fractional = initial_free * sum_factors;
            if sum_factors < 1.0 && fractional.abs() < free.abs() {
                fractional
            } else {
                free
            }
        };
        let mut allocated = 0.0_f32;
        if growing {
            let mut free = free_room(sum_frozen, sum_base, sum_factors);
            for item in items.iter_mut().filter(|item| !item.frozen) {
                let share = if sum_grow > 0.0 {
                    item.grow / sum_grow
                } else {
                    0.0
                };
                let space = self.px(free * share);
                free -= space;
                sum_grow -= item.grow;
                item.target = item.flex_base + space;
                allocated += item.target;
            }
        } else {
            let mut redistribute = true;
            let mut passes = 0;
            while redistribute && passes < items.len() && unfrozen > 0 {
                redistribute = false;
                passes += 1;
                allocated = 0.0;
                let mut free = free_room(sum_frozen, sum_base, sum_factors).abs();
                let mut scaled_left = sum_scaled_shrink;
                for item in items.iter_mut().filter(|item| !item.frozen) {
                    let scaled = item.config.shrink * item.flex_base;
                    let share = if scaled_left > 0.0 {
                        scaled / scaled_left
                    } else {
                        0.0
                    };
                    let take = self.px(free * share);
                    let target = item.flex_base - take;
                    scaled_left -= scaled;
                    if target < item.min_main {
                        // The item stops at its smallest size; the others
                        // share what it could not give up on the next pass.
                        item.frozen = true;
                        redistribute = true;
                        free -= item.flex_base - item.min_main;
                        item.target = item.min_main;
                        unfrozen -= 1;
                        sum_frozen += item.min_main;
                        sum_base -= item.flex_base;
                        sum_scaled_shrink -= scaled;
                        sum_factors -= item.config.shrink;
                    } else {
                        free -= take;
                        item.target = target;
                        allocated += target;
                    }
                }
            }
        }
        total_gap + sum_frozen + allocated
    }

    /// Measures the line's baseline children and asks the others for their
    /// cross size at their main size, for a line sized before its children
    /// measure. Compose's `calculateLineCrossAxisSize`.
    fn line_cross_size(
        &self,
        line: &mut Line,
        items: &mut [Item],
        measurables: &[Box<dyn Measurable>],
        remaining_cross: f32,
    ) {
        let (mut size, mut above, mut below) = (0.0_f32, 0.0_f32, 0.0_f32);
        for item in &mut items[line.start..line.end] {
            let measurable = measurables[item.child].as_ref();
            let contribution = if self.aligns_by_baseline(&item.config) {
                self.measure_item(item, measurable, None, remaining_cross);
                let baseline = item.placeable.as_ref().map_or(0.0, |placeable| {
                    placeable
                        .alignment_lines()
                        .first_baseline()
                        .unwrap_or_else(|| placeable.height())
                });
                item.baseline = Some(baseline);
                above = above.max(baseline);
                below = below.max(item.cross_size - baseline);
                above + below
            } else {
                item.cross_size = self.cross_intrinsic(measurable, item.target, false);
                item.cross_size
            };
            size = size.max(contribution);
        }
        line.max_above_baseline = above;
        line.cross_size = size;
    }

    /// Measures a child at its main size, filling `stretch_cross` across
    /// when given. Returns its cross size.
    fn measure_item(
        &self,
        item: &mut Item,
        measurable: &dyn Measurable,
        stretch_cross: Option<f32>,
        remaining_cross: f32,
    ) -> f32 {
        let (cross_min, cross_max) = match stretch_cross {
            Some(cross) => {
                let cross = cross.min(remaining_cross);
                (cross, cross)
            }
            None => (0.0, remaining_cross),
        };
        let constraints = if self.horizontal() {
            Constraints {
                min_width: item.target,
                max_width: item.target,
                min_height: cross_min,
                max_height: cross_max,
            }
        } else {
            Constraints {
                min_width: cross_min,
                max_width: cross_max,
                min_height: item.target,
                max_height: item.target,
            }
        };
        let placeable = measurable.measure(constraints);
        item.cross_size = if self.horizontal() {
            placeable.height()
        } else {
            placeable.width()
        };
        item.placeable = Some(placeable);
        item.cross_size
    }

    /// Measures the items the line sizing left, and sizes the lines that it
    /// did not. Returns the lines' total cross size. Compose's
    /// `measureFlexItems`.
    fn measure_items(
        &self,
        lines: &mut [Line],
        items: &mut [Item],
        measurables: &[Box<dyn Measurable>],
        (axes, cross_gap): (AxisConstraints, f32),
        (upfront, total_cross): (bool, f32),
    ) -> f32 {
        let wraps = self.config.wrap.wraps();
        let mut measured_total = total_cross;
        let mut remaining_cross = axes.cross_max;
        for line in lines.iter_mut() {
            let mut line_cross = if upfront { line.cross_size } else { 0.0 };
            for item in &mut items[line.start..line.end] {
                if item.placeable.is_some() {
                    if !upfront {
                        line_cross = line_cross.max(item.cross_size);
                    }
                    continue;
                }
                let stretch = (self.stretches(&item.config) && line.cross_size > 0.0)
                    .then_some(line.cross_size);
                let cross = self.measure_item(
                    item,
                    measurables[item.child].as_ref(),
                    stretch,
                    remaining_cross,
                );
                if !upfront {
                    line_cross = line_cross.max(cross);
                }
                if !wraps {
                    line_cross = line_cross.max(axes.cross_min);
                }
            }
            if !upfront {
                line.cross_size = line_cross;
                measured_total += line_cross;
            }
            remaining_cross = (remaining_cross - line.cross_size - cross_gap).max(0.0);
        }
        if upfront { total_cross } else { measured_total }
    }

    /// Gives each line of a wrapping box an equal share of the room up to
    /// the box's smallest cross size. Compose's `applyAlignContentStretch`.
    fn stretch_lines(
        &self,
        lines: &mut [Line],
        axes: AxisConstraints,
        total_cross: f32,
        cross_gap: f32,
    ) -> f32 {
        if !self.config.wrap.wraps()
            || self.config.align_content != FlexAlignContent::Stretch
            || !axes.cross_min.is_finite()
            || lines.is_empty()
        {
            return total_cross;
        }
        let spacing = cross_gap * (lines.len() - 1) as f32;
        if total_cross + spacing >= axes.cross_min {
            return total_cross;
        }
        let extra = (axes.cross_min - total_cross - spacing).max(0.0);
        let per_line = self.truncate_px(extra / lines.len() as f32);
        let mut position = 0.0;
        for line in lines.iter_mut() {
            line.cross_start = position;
            line.cross_size += per_line;
            position += line.cross_size + cross_gap;
        }
        total_cross + per_line * lines.len() as f32
    }

    /// Places the lines of a wrapping box across it. Compose's
    /// `calculateLineCrossPositions`.
    fn position_lines(&self, lines: &mut [Line], space: f32, total_cross: f32, cross_gap: f32) {
        if !self.config.wrap.wraps() || lines.is_empty() {
            return;
        }
        let reverse = self.config.wrap == crate::widgets::flex_box::FlexWrap::WrapReverse;
        let count = lines.len() as f32;
        let free = space - total_cross - cross_gap * (count - 1.0);
        let between = match self.config.align_content {
            FlexAlignContent::SpaceAround => self.truncate_px(free / count),
            FlexAlignContent::SpaceBetween if lines.len() > 1 => {
                self.truncate_px(free / (count - 1.0))
            }
            _ => 0.0,
        };
        let mut position = match self.config.align_content {
            FlexAlignContent::End if !reverse => free,
            FlexAlignContent::End => 0.0,
            FlexAlignContent::Center => self.truncate_px(free / 2.0),
            FlexAlignContent::SpaceAround => self.truncate_px(between / 2.0),
            _ if reverse => free,
            _ => 0.0,
        };
        let mut place = |line: &mut Line| {
            line.cross_start = position;
            position += line.cross_size + between + cross_gap;
        };
        if reverse {
            lines.iter_mut().rev().for_each(&mut place);
        } else {
            lines.iter_mut().for_each(&mut place);
        }
    }

    /// Places one line's items along the main axis by the justification and
    /// across it by their alignment. Compose's `positionItemsOnMainAxis`.
    fn position_line(&self, line: &Line, items: &mut [Item], container_main: f32, main_gap: f32) {
        let count = line.end - line.start;
        if count == 0 {
            return;
        }
        let reverse = self.config.direction.is_reverse();
        let remaining = container_main - line.main_size;
        let n = count as f32;
        let between = match self.config.justify_content {
            FlexJustifyContent::SpaceAround => self.truncate_px(remaining / n),
            FlexJustifyContent::SpaceBetween if count > 1 => {
                self.truncate_px(remaining / (n - 1.0))
            }
            FlexJustifyContent::SpaceEvenly => self.truncate_px(remaining / (n + 1.0)),
            _ => 0.0,
        };
        let mut position = match self.config.justify_content {
            FlexJustifyContent::End if reverse => 0.0,
            FlexJustifyContent::End => remaining,
            FlexJustifyContent::Center => self.truncate_px(remaining / 2.0),
            FlexJustifyContent::SpaceAround => self.truncate_px(between / 2.0),
            FlexJustifyContent::SpaceEvenly => between,
            FlexJustifyContent::SpaceBetween if count == 1 && reverse => remaining,
            FlexJustifyContent::SpaceBetween => 0.0,
            FlexJustifyContent::Start if reverse => remaining,
            FlexJustifyContent::Start => 0.0,
        };
        let mut place = |item: &mut Item| {
            item.main_position = position;
            position += item.target + between + main_gap;
            item.cross_position = line.cross_start + self.cross_offset(item, line);
        };
        let line_items = &mut items[line.start..line.end];
        if reverse {
            line_items.iter_mut().rev().for_each(&mut place);
        } else {
            line_items.iter_mut().for_each(&mut place);
        }
    }

    /// Where an item sits across its line. Compose's
    /// `calculateItemCrossPosition`.
    fn cross_offset(&self, item: &Item, line: &Line) -> f32 {
        let alignment = match item.config.align_self {
            FlexAlignSelf::Auto => match self.config.align_items {
                FlexAlignItems::Start => FlexAlignSelf::Start,
                FlexAlignItems::End => FlexAlignSelf::End,
                FlexAlignItems::Center => FlexAlignSelf::Center,
                FlexAlignItems::Stretch => FlexAlignSelf::Stretch,
                FlexAlignItems::Baseline => FlexAlignSelf::Baseline,
            },
            alignment => alignment,
        };
        match alignment {
            FlexAlignSelf::End => line.cross_size - item.cross_size,
            FlexAlignSelf::Center => self.truncate_px((line.cross_size - item.cross_size) / 2.0),
            FlexAlignSelf::Baseline => item
                .baseline
                .map_or(0.0, |baseline| line.max_above_baseline - baseline),
            FlexAlignSelf::Auto | FlexAlignSelf::Start | FlexAlignSelf::Stretch => 0.0,
        }
    }

    /// The lines' cross size of all children when each sizes along the main
    /// axis by `main`, breaking lines in `available`. Compose's
    /// `intrinsicCrossAxisSize`: an approximation, as children do not grow
    /// or shrink here.
    fn intrinsic_cross(
        &self,
        measurables: &[Box<dyn Measurable>],
        available: f32,
        min: bool,
    ) -> f32 {
        let (main_gap, cross_gap) = self.gaps();
        let wraps = self.config.wrap.wraps();
        let (mut line_main, mut line_cross, mut total, mut in_line) = (0.0_f32, 0.0_f32, 0.0, 0);
        for measurable in measurables {
            let main = self.main_intrinsic(measurable.as_ref(), f32::INFINITY, min);
            let cross = self.cross_intrinsic(measurable.as_ref(), main, min);
            let projected = if in_line == 0 {
                main
            } else {
                line_main + main_gap + main
            };
            if wraps
                && in_line > 0
                && (in_line >= self.config.max_items_in_each_line
                    || projected > available + FIT_EPSILON)
            {
                total += line_cross + cross_gap;
                (line_main, line_cross, in_line) = (main, cross, 1);
            } else {
                line_main = projected;
                line_cross = line_cross.max(cross);
                in_line += 1;
            }
        }
        total + line_cross
    }

    /// The smallest main size: the widest child when the box wraps, all of
    /// them with their gaps when it does not.
    fn min_intrinsic_main(&self, measurables: &[Box<dyn Measurable>], cross: f32) -> f32 {
        let sizes = measurables
            .iter()
            .map(|measurable| self.main_intrinsic(measurable.as_ref(), cross, true));
        if self.config.wrap.wraps() {
            sizes.fold(0.0, f32::max)
        } else {
            sizes.sum::<f32>() + self.gaps().0 * measurables.len().saturating_sub(1) as f32
        }
    }

    /// The preferred main size: one line, or the longest line when lines
    /// break after `max_items_in_each_line` children. Compose's
    /// `maxIntrinsicMainAxisSize`.
    fn max_intrinsic_main(&self, measurables: &[Box<dyn Measurable>], cross: f32) -> f32 {
        let gap = self.gaps().0;
        let per_line = if self.config.wrap.wraps() {
            self.config.max_items_in_each_line
        } else {
            usize::MAX
        };
        let (mut longest, mut line, mut in_line) = (0.0_f32, 0.0_f32, 0);
        for measurable in measurables {
            let size = self.main_intrinsic(measurable.as_ref(), cross, false);
            line += if in_line == 0 { size } else { gap + size };
            in_line += 1;
            if in_line == per_line {
                longest = longest.max(line);
                (line, in_line) = (0.0, 0);
            }
        }
        longest.max(line)
    }

    /// An intrinsic size along `width` (true) or height; `other` is the
    /// size given on the other axis.
    fn intrinsic(
        &self,
        measurables: &[Box<dyn Measurable>],
        width: bool,
        other: f32,
        min: bool,
    ) -> f32 {
        if measurables.is_empty() {
            return 0.0;
        }
        match (width == self.horizontal(), min) {
            (true, true) => self.min_intrinsic_main(measurables, other),
            (true, false) => self.max_intrinsic_main(measurables, other),
            (false, _) => self.intrinsic_cross(measurables, other, min),
        }
    }
}

impl MeasurePolicy for FlexBoxMeasurePolicy {
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
            return crate::modifier::Size {
                width: constraints.min_width,
                height: constraints.min_height,
            }
            .into();
        }
        let horizontal = self.horizontal();
        let axes = AxisConstraints::of(constraints, horizontal);
        let mut upfront = matches!(
            self.config.align_items,
            FlexAlignItems::Stretch | FlexAlignItems::Baseline
        ) || (self.config.wrap.wraps()
            && self.config.align_content == FlexAlignContent::Stretch
            && axes.cross_max.is_finite());
        let mut items: Items = measurables
            .iter()
            .enumerate()
            .map(|(child, measurable)| self.item(child, measurable.as_ref(), axes))
            .collect();
        upfront |= items.iter().any(|item| {
            matches!(
                item.config.align_self,
                FlexAlignSelf::Baseline | FlexAlignSelf::Stretch
            )
        });
        if items.iter().any(|item| item.config.order != 0) {
            items.sort_by_key(|item| item.config.order);
        }
        let (main_gap, cross_gap) = self.gaps();
        let (mut lines, total_cross) = self.build_lines(
            &mut items,
            measurables,
            axes,
            (main_gap, cross_gap),
            upfront,
        );
        if !self.config.wrap.wraps()
            && let Some(line) = lines.first_mut()
        {
            line.cross_size = line.cross_size.max(axes.cross_min);
        }
        let total_cross = self.stretch_lines(&mut lines, axes, total_cross, cross_gap);
        let total_cross = self.measure_items(
            &mut lines,
            &mut items,
            measurables,
            (axes, cross_gap),
            (upfront, total_cross),
        );
        self.position_lines(
            &mut lines,
            total_cross.max(axes.cross_min),
            total_cross,
            cross_gap,
        );
        let reverse_lines = self.config.wrap == crate::widgets::flex_box::FlexWrap::WrapReverse;
        let edge_line = if reverse_lines {
            lines.first()
        } else {
            lines.last()
        };
        let lines_cross = edge_line
            .map_or(0.0, |line| line.cross_start + line.cross_size)
            .max(axes.cross_min);
        let longest_line = lines
            .iter()
            .map(|line| line.main_size)
            .reduce(f32::max)
            .unwrap_or(axes.main_min);
        let main = longest_line
            .max(axes.main_min)
            .clamp(axes.main_min, axes.main_max);
        let cross = lines_cross.clamp(axes.cross_min, axes.cross_max);
        let (width, height) = if horizontal {
            (main, cross)
        } else {
            (cross, main)
        };

        let reverse_items = self.config.direction.is_reverse();
        placements.reserve(items.len());
        let mut place_line = |line: &Line| {
            self.position_line(line, &mut items, main, main_gap);
            let mut place = |item: &mut Item| {
                let Some(placeable) = item.placeable.take() else {
                    return;
                };
                let (x, y) = if horizontal {
                    (item.main_position, item.cross_position)
                } else {
                    (item.cross_position, item.main_position)
                };
                let x = self.layout_direction.place_x(x, width, placeable.width());
                placeable.place(x, y);
                placements.push(Placement::new(placeable.node_id(), x, y, 0));
            };
            let line_items = &mut items[line.start..line.end];
            if reverse_items {
                line_items.iter_mut().rev().for_each(&mut place);
            } else {
                line_items.iter_mut().for_each(&mut place);
            }
        };
        if reverse_lines {
            lines.iter().rev().for_each(&mut place_line);
        } else {
            lines.iter().for_each(&mut place_line);
        }
        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        self.intrinsic(measurables, true, height, true)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        self.intrinsic(measurables, true, height, false)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.intrinsic(measurables, false, width, true)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.intrinsic(measurables, false, width, false)
    }
}
