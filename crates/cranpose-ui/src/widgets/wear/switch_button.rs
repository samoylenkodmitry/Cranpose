//! A Wear switch row: a 52dp capsule with a label and a 32x22dp switch.
//!
//! Three details here are measured, not guessed, and each is a place a
//! reasonable implementation goes wrong:
//!
//! - **The switch graphic sits 2dp above the row's centre.** Its slot is
//!   32x24dp but the switch draws at 32x22dp, and the slot aligns width only —
//!   vertical placement falls back to the top. Centring the graphic in the row,
//!   which is the obvious thing to do, is wrong by 2 pixels at density 2.
//! - **A checked switch has no border.** The track and the border both resolve
//!   to `primary`, and the drawing suppresses a border equal to its track. Only
//!   the unchecked state shows the outline ring.
//! - **A checked thumb is often invisible, and still matters.** In a scheme
//!   where the thumb and the container are the same colour it shows only where
//!   it overlaps the light track — but it still occludes the track, so it
//!   cannot be optimised away.

#![allow(non_snake_case)]

use cranpose_core::NodeId;
use cranpose_foundation::SemanticsWidgetRole;
use cranpose_ui_graphics::{DrawScope, Size, VectorPath};
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

use crate::{
    composable,
    density::Density,
    modifier::{Brush, Color, CornerRadii, Modifier, Point, Rect},
    widgets::{
        Layout, Text,
        wear::theme::{WearColors, WearTextStyle},
    },
};

/// `SwitchButtonDefaults` plus `ToggleButton`'s layout constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchButtonSpec {
    pub colors: WearColors,
    pub label_style: WearTextStyle,
    pub secondary_style: WearTextStyle,
    pub min_height: f32,
    pub corner_radius: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    /// `SWITCH_WIDTH`.
    pub switch_width: f32,
    /// `SWITCH_OUTER_HEIGHT` — the slot the graphic is placed in.
    pub switch_slot_height: f32,
    /// `SWITCH_INNER_HEIGHT` — what the graphic actually draws.
    pub switch_height: f32,
    /// `SWITCH_TRACK_WIDTH` — the unchecked state's border stroke.
    pub track_border_width: f32,
    pub thumb_radius_unchecked: f32,
    pub thumb_radius_checked: f32,
    /// `TOGGLE_CONTROL_SPACING`.
    pub control_spacing: f32,
    /// `LabelSpacerSize`.
    pub label_spacing: f32,
    /// How far the thumb has travelled, `0.0` unchecked and `1.0` checked.
    ///
    /// A caller animating the transition drives this; a caller that does not
    /// passes the checked state itself. Wear animates it with
    /// `motionScheme.fastEffectsSpec()`, a critically damped spring of
    /// stiffness 1400.
    pub progress: f32,
}

impl Default for SwitchButtonSpec {
    fn default() -> Self {
        Self {
            colors: WearColors::default(),
            label_style: WearTextStyle::LABEL_MEDIUM,
            secondary_style: WearTextStyle::LABEL_SMALL,
            min_height: 52.0,
            corner_radius: 26.0,
            padding_horizontal: 14.0,
            padding_vertical: 8.0,
            switch_width: 32.0,
            switch_slot_height: 24.0,
            switch_height: 22.0,
            track_border_width: 2.0,
            thumb_radius_unchecked: 6.0,
            thumb_radius_checked: 9.0,
            control_spacing: 6.0,
            label_spacing: 1.0,
            progress: 0.0,
        }
    }
}

impl SwitchButtonSpec {
    pub fn colors(mut self, colors: WearColors) -> Self {
        self.colors = colors;
        self
    }

    /// The thumb travel, which a caller animates or snaps.
    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = progress.clamp(0.0, 1.0);
        self
    }
}

/// The stiffness and damping of the two springs a Wear switch animates on.
///
/// `MotionScheme.standard()`: `fastEffectsSpec` moves the thumb, its radius and
/// the tick's scale; `slowEffectsSpec` moves every colour. Both are critically
/// damped — a switch does not bounce.
pub const SWITCH_THUMB_STIFFNESS: f32 = 1400.0;
pub const SWITCH_COLOR_STIFFNESS: f32 = 260.0;
pub const SWITCH_DAMPING_RATIO: f32 = 1.0;

/// The colours one state of the switch draws with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchColors {
    pub container: Color,
    pub label: Color,
    pub secondary_label: Color,
    pub track: Color,
    /// `Color::rgba(_, _, _, 0.0)` when the border is suppressed.
    pub track_border: Color,
    pub thumb: Color,
    pub tick: Color,
}

impl SwitchColors {
    /// The slots a switch resolves to at a given checked state.
    ///
    /// The border suppression lives here: `if currentTrackColor ==
    /// currentTrackBorderColor { borderColor = Color.Transparent }`, which in
    /// the checked state is always true because both are `primary`.
    pub fn of(colors: WearColors, checked: bool) -> Self {
        let (container, label, secondary_label, track, border, thumb, tick) = if checked {
            (
                colors.primary_container,
                colors.on_primary_container,
                fade(colors.on_primary_container, 0.9),
                colors.primary,
                colors.primary,
                colors.primary_container,
                colors.primary,
            )
        } else {
            (
                colors.surface_container,
                colors.on_surface,
                colors.on_surface_variant,
                colors.surface_container,
                colors.outline,
                colors.outline,
                colors.primary,
            )
        };
        let track_border = if border == track {
            Color::rgba(border.0, border.1, border.2, 0.0)
        } else {
            border
        };
        Self {
            container,
            label,
            secondary_label,
            track,
            track_border,
            thumb,
            tick,
        }
    }
}

fn fade(color: Color, alpha: f32) -> Color {
    Color::rgba(color.0, color.1, color.2, color.3 * alpha)
}

/// Where the thumb is and how big, at a given progress.
///
/// `thumbPadding` is `height/2 - radius` on each side, so the thumb grows into
/// its own margin as it travels: 5dp of clearance unchecked and 2dp checked.
pub fn switch_thumb(spec: SwitchButtonSpec, progress: f32) -> (f32, f32) {
    let progress = progress.clamp(0.0, 1.0);
    let radius = spec.thumb_radius_unchecked
        + (spec.thumb_radius_checked - spec.thumb_radius_unchecked) * progress;
    let half = spec.switch_height * 0.5;
    let start = radius + (half - spec.thumb_radius_unchecked);
    let end = spec.switch_width - radius - (half - spec.thumb_radius_checked);
    (start + (end - start) * progress, radius)
}

/// The tick's scale factor: a cubic ease-out on the thumb progress.
///
/// The stroke scales with the canvas, so the tick's own line thins as it grows
/// in rather than being drawn thin and widening.
pub fn switch_tick_scale(progress: f32) -> f32 {
    let remaining = 1.0 - progress.clamp(0.0, 1.0);
    1.0 - remaining * remaining * remaining
}

/// Draws the 32x22dp switch graphic into a scope of exactly that size.
pub fn draw_switch(scope: &mut dyn DrawScope, spec: SwitchButtonSpec, colors: SwitchColors) {
    let size = scope.size();
    if size.width <= 0.0 || size.height <= 0.0 {
        return;
    }
    let radius = size.height * 0.5;
    scope.draw_round_rect(Brush::Solid(colors.track), CornerRadii::uniform(radius));

    if colors.track_border.3 > 0.0 {
        let inset = spec.track_border_width * 0.5;
        scope.draw_round_rect_at_stroked(
            Rect {
                x: inset,
                y: inset,
                width: size.width - spec.track_border_width,
                height: size.height - spec.track_border_width,
            },
            Brush::Solid(colors.track_border),
            CornerRadii::uniform(radius - inset),
            cranpose_ui_graphics::Stroke::new(spec.track_border_width),
        );
    }

    let (centre_x, thumb_radius) = switch_thumb(spec, spec.progress);
    scope.draw_circle(
        Brush::Solid(colors.thumb),
        Point {
            x: centre_x,
            y: radius,
        },
        thumb_radius,
    );

    draw_tick(
        scope,
        Point {
            x: centre_x,
            y: radius,
        },
        switch_tick_scale(spec.progress),
        colors.tick,
    );
}

/// Wear's tick, centred on a point.
///
/// The path lives in a 24x24dp design box and is **two disjoint subpaths**, not
/// one polyline: the joint is two overlapping round caps rather than a stroke
/// join, which is a visibly different corner.
fn draw_tick(scope: &mut dyn DrawScope, centre: Point, scale: f32, color: Color) {
    if scale <= 0.0 || color.3 <= 0.0 {
        return;
    }
    const SEGMENTS: [((f32, f32), (f32, f32)); 2] = [
        ((7.4 - 12.0, 13.0 - 12.0), (9.9 - 12.0, 15.5 - 12.0)),
        ((10.5 - 12.0, 15.1 - 12.0), (16.5 - 12.0, 9.1 - 12.0)),
    ];
    const STROKE: f32 = 2.0;
    let half = STROKE * scale * 0.5;
    for (start, end) in SEGMENTS {
        let a = Point {
            x: centre.x + start.0 * scale,
            y: centre.y + start.1 * scale,
        };
        let b = Point {
            x: centre.x + end.0 * scale,
            y: centre.y + end.1 * scale,
        };
        stroke_round_capped(scope, a, b, half, color);
    }
}

/// A round-capped straight stroke, as its exact constituent shapes.
///
/// A round-capped stroke is the sum of the segment and a disc, so it is a
/// parallelogram body between two circles. `DrawScope` has no line primitive
/// and `draw_vector_path` fills rather than strokes, so this composes the three
/// shapes it is made of rather than approximating it.
fn stroke_round_capped(scope: &mut dyn DrawScope, a: Point, b: Point, half: f32, color: Color) {
    if half <= 0.0 {
        return;
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = (dx * dx + dy * dy).sqrt();
    if length > f32::EPSILON {
        let (nx, ny) = (-dy / length * half, dx / length * half);
        let body = format!(
            "M {} {} L {} {} L {} {} L {} {} Z",
            a.x + nx,
            a.y + ny,
            b.x + nx,
            b.y + ny,
            b.x - nx,
            b.y - ny,
            a.x - nx,
            a.y - ny
        );
        if let Ok(path) = VectorPath::parse(&body) {
            scope.draw_vector_path(&path, Brush::Solid(color));
        }
    }
    scope.draw_circle(Brush::Solid(color), a, half);
    scope.draw_circle(Brush::Solid(color), b, half);
}

/// A switch row: a label, an optional secondary label, and a switch.
///
/// `on_checked_change` is handed the **new** value, matching
/// `Modifier.toggleable`. It is unwrapped into a zero-argument callback for the
/// node below because a composable's callback parameters take no arguments —
/// the macro re-installs them through an `Fn()`.
pub fn SwitchButton<F>(
    modifier: Modifier,
    spec: SwitchButtonSpec,
    checked: bool,
    label: String,
    secondary_label: Option<String>,
    on_checked_change: F,
) -> NodeId
where
    F: Fn(bool) + 'static,
{
    SwitchButtonNode(modifier, spec, checked, label, secondary_label, move || {
        on_checked_change(!checked)
    })
}

/// The node half of [`SwitchButton`].
#[composable]
pub fn SwitchButtonNode<F>(
    modifier: Modifier,
    spec: SwitchButtonSpec,
    checked: bool,
    label: String,
    secondary_label: Option<String>,
    on_toggle: F,
) -> NodeId
where
    F: FnMut() + 'static,
{
    let density = crate::density::density();
    let colors = SwitchColors::of(spec.colors, checked);
    let radius = density.dp(spec.corner_radius);
    let container = colors.container;
    let chrome = modifier
        .draw_behind(move |scope: &mut dyn DrawScope| {
            scope.draw_round_rect(Brush::Solid(container), CornerRadii::uniform(radius));
        })
        .toggleable(
            checked,
            Some(format!("{label}, {}", if checked { "on" } else { "off" })),
            Some(SemanticsWidgetRole::Switch),
            move |_next| on_toggle(),
        );

    let label_style = spec.label_style.resolve(colors.label);
    let secondary_style = spec.secondary_style.resolve(colors.secondary_label);
    let label_text = label;
    Layout(
        chrome,
        SwitchButtonMeasurePolicy {
            spec,
            density: density.density(),
            has_secondary: secondary_label.is_some(),
        },
        move || {
            Text(label_text.clone(), Modifier::empty(), label_style.clone());
            if let Some(secondary) = secondary_label.clone() {
                Text(secondary, Modifier::empty(), secondary_style.clone());
            }
            SwitchGraphic(Modifier::empty(), spec, colors);
        },
    )
}

/// The switch itself, as its own node so it has its own hit rect and its own
/// place in the tree.
#[composable]
pub fn SwitchGraphic(modifier: Modifier, spec: SwitchButtonSpec, colors: SwitchColors) -> NodeId {
    crate::widgets::Canvas(
        modifier.size_points(spec.switch_width, spec.switch_height),
        move |scope: &mut dyn DrawScope| draw_switch(scope, spec, colors),
    )
}

#[derive(Clone, Debug, PartialEq)]
struct SwitchButtonMeasurePolicy {
    spec: SwitchButtonSpec,
    density: f32,
    has_secondary: bool,
}

impl MeasurePolicy for SwitchButtonMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let size = self.measure_into(scope, measurables, constraints, &mut placements);
        MeasureResult::new(size, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> Size {
        placements.clear();
        let density = Density::new(self.density, 1.0);
        let horizontal = density.dp(self.spec.padding_horizontal) * 2.0;
        let vertical = density.dp(self.spec.padding_vertical) * 2.0;
        let width = if constraints.max_width.is_finite() {
            constraints.max_width
        } else {
            constraints.min_width
        };

        let switch_width = density.dp(self.spec.switch_width);
        let spacing = density.dp(self.spec.control_spacing);
        let label_width = (width - horizontal - switch_width - spacing).max(0.0);
        let label_constraints = Constraints {
            min_width: 0.0,
            max_width: label_width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };

        let label_count = if self.has_secondary { 2 } else { 1 };
        let mut labels = Vec::with_capacity(label_count);
        for measurable in measurables.iter().take(label_count) {
            labels.push(measurable.measure(label_constraints));
        }
        let switch = measurables.get(label_count).map(|measurable| {
            measurable.measure(Constraints {
                min_width: 0.0,
                max_width: switch_width,
                min_height: 0.0,
                max_height: f32::INFINITY,
            })
        });

        let label_spacing = density.dp(self.spec.label_spacing);
        let column: f32 = labels
            .iter()
            .map(|placeable| density.ceil(placeable.height()))
            .sum::<f32>()
            + label_spacing * labels.len().saturating_sub(1) as f32;
        let slot = density.dp(self.spec.switch_slot_height);
        let content_demand = column.max(slot);
        let height = density
            .dp(self.spec.min_height)
            .max(density.ceil(content_demand) + vertical)
            .clamp(constraints.min_height, constraints.max_height);

        let content = height - vertical;
        let padding_top = density.dp(self.spec.padding_vertical);
        let mut y = padding_top + density.centre(content, column);
        let x = density.dp(self.spec.padding_horizontal);
        for placeable in &labels {
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
            y += density.ceil(placeable.height()) + label_spacing;
        }

        if let Some(switch) = switch {
            let slot_top = padding_top + density.centre(content, slot);
            let switch_x = width - density.dp(self.spec.padding_horizontal) - switch.width();
            placements.push(Placement::new(switch.node_id(), switch_x, slot_top, 0));
        }

        Size::new(width, height)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        let density = Density::new(self.density, 1.0);
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .fold(0.0, f32::max)
            + density.dp(self.spec.padding_horizontal) * 2.0
            + density.dp(self.spec.switch_width)
            + density.dp(self.spec.control_spacing)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        self.min_intrinsic_width(measurables, height)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        let density = Density::new(self.density, 1.0);
        let label_count = if self.has_secondary { 2 } else { 1 };
        let column: f32 = measurables
            .iter()
            .take(label_count)
            .map(|m| m.min_intrinsic_height(width))
            .sum::<f32>();
        density.dp(self.spec.min_height).max(
            density.ceil(column.max(density.dp(self.spec.switch_slot_height)))
                + density.dp(self.spec.padding_vertical) * 2.0,
        )
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.min_intrinsic_height(measurables, width)
    }
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::{DrawScopeDefault, Size as GraphicsSize};

    use super::*;

    fn colors() -> WearColors {
        WearColors {
            primary: Color::from_rgb_u8(0xB9, 0xF2, 0xFF),
            primary_container: Color::from_rgb_u8(0x0F, 0x36, 0x4E),
            surface_container: Color::from_rgb_u8(0x0A, 0x16, 0x22),
            outline: Color::from_rgb_u8(0x1D, 0x4D, 0x69),
            ..WearColors::default()
        }
    }

    #[test]
    fn the_defaults_are_the_ones_the_tokens_declare() {
        let spec = SwitchButtonSpec::default();
        assert_eq!(spec.min_height, 52.0);
        assert_eq!(spec.corner_radius, 26.0);
        assert_eq!(spec.padding_horizontal, 14.0);
        assert_eq!(spec.padding_vertical, 8.0);
        assert_eq!(spec.switch_width, 32.0);
        assert_eq!(spec.switch_slot_height, 24.0);
        assert_eq!(spec.switch_height, 22.0);
        assert_eq!(spec.track_border_width, 2.0);
        assert_eq!(spec.thumb_radius_unchecked, 6.0);
        assert_eq!(spec.thumb_radius_checked, 9.0);
        assert_eq!(spec.control_spacing, 6.0);
    }

    #[test]
    fn the_thumb_travels_between_the_two_measured_positions() {
        let spec = SwitchButtonSpec::default();
        let (unchecked, radius_off) = switch_thumb(spec, 0.0);
        assert_eq!(unchecked, 11.0, "22px at density 2");
        assert_eq!(radius_off, 6.0);
        let (checked, radius_on) = switch_thumb(spec, 1.0);
        assert_eq!(checked, 21.0, "42px at density 2");
        assert_eq!(radius_on, 9.0);
    }

    #[test]
    fn a_checked_switch_draws_no_border_and_an_unchecked_one_does() {
        let checked = SwitchColors::of(colors(), true);
        assert_eq!(
            checked.track_border.3, 0.0,
            "track and border are both primary, so the border is suppressed"
        );
        assert_eq!(checked.track, colors().primary);
        assert_eq!(checked.thumb, colors().primary_container);

        let unchecked = SwitchColors::of(colors(), false);
        assert!(unchecked.track_border.3 > 0.0);
        assert_eq!(unchecked.track_border, colors().outline);
    }

    #[test]
    fn a_checked_thumb_is_the_container_colour_and_still_gets_drawn() {
        let scheme = colors();
        let checked = SwitchColors::of(scheme, true);
        assert_eq!(
            checked.thumb, checked.container,
            "invisible against the row, and still occluding the track"
        );
        let mut scope = DrawScopeDefault::new(GraphicsSize::new(32.0, 22.0));
        draw_switch(
            &mut scope,
            SwitchButtonSpec::default().colors(scheme).progress(1.0),
            checked,
        );
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 8);
    }

    #[test]
    fn an_unchecked_switch_draws_its_ring_and_no_tick() {
        let mut scope = DrawScopeDefault::new(GraphicsSize::new(32.0, 22.0));
        draw_switch(
            &mut scope,
            SwitchButtonSpec::default().colors(colors()).progress(0.0),
            SwitchColors::of(colors(), false),
        );
        assert_eq!(scope.into_primitives().len(), 3);
    }

    #[test]
    fn the_tick_eases_out_rather_than_growing_linearly() {
        assert_eq!(switch_tick_scale(0.0), 0.0);
        assert_eq!(switch_tick_scale(1.0), 1.0);
        assert!(switch_tick_scale(0.5) > 0.5);
        assert!((switch_tick_scale(0.5) - 0.875).abs() < 1e-6);
    }

    #[test]
    fn the_springs_are_the_ones_the_standard_motion_scheme_declares() {
        assert_eq!(SWITCH_THUMB_STIFFNESS, 1400.0);
        assert_eq!(SWITCH_COLOR_STIFFNESS, 260.0);
        assert_eq!(SWITCH_DAMPING_RATIO, 1.0);
    }
}
