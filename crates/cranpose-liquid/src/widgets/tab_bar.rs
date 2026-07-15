//! The floating glass tab bar: a capsule of tabs over live content with a
//! liquid selection blob (dual-spring stretch), plus an optional detached
//! circular accessory (the iOS 26 search button).

use crate::material::{
    liquid_content_mask_with, neutral_surface_lift, neutral_surface_tint, Glass, GlassDynamics,
    GlassMorph, LiquidModifierExt,
};
use crate::motion::{liquid_axis_owns_visual_selection, liquid_visual_index, LiquidMotion};
use crate::theme::{liquid_colors, liquid_typography, LiquidTypography};
use cranpose_foundation::PointerId;
use cranpose_macros::composable;
use cranpose_services::{default_haptics, HapticFeedback};
use cranpose_ui::text::{FontWeight, SpanStyle, TextStyle};
use cranpose_ui::widgets::{
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Column, ColumnSpec, Row, RowSpec,
    Text,
};
use cranpose_ui::{
    Brush, Color, CornerRadii, Modifier, PointerEventKind, PointerInputScope, Rect, Size,
};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, VerticalAlignment};
use std::cell::RefCell;
use std::rc::Rc;

/// Visual treatment for a tab icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiquidTabIconStyle {
    #[default]
    Plain,
    AppBadge,
}

/// One tab: icon path data (24×24 viewBox), label, and icon treatment.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidTab {
    pub icon: &'static str,
    pub label: &'static str,
    pub icon_style: LiquidTabIconStyle,
    /// Optical correction for symbols whose path bounds do not fill the
    /// shared icon frame uniformly.
    pub icon_scale: f32,
}

impl LiquidTab {
    pub fn new(icon: &'static str, label: &'static str) -> Self {
        Self {
            icon,
            label,
            icon_style: LiquidTabIconStyle::Plain,
            icon_scale: 1.0,
        }
    }

    pub fn app_badge(icon: &'static str, label: &'static str) -> Self {
        Self {
            icon,
            label,
            icon_style: LiquidTabIconStyle::AppBadge,
            icon_scale: 1.0,
        }
    }

    pub fn with_icon_scale(mut self, scale: f32) -> Self {
        self.icon_scale = normalize_icon_scale(scale);
        self
    }
}

fn normalize_icon_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(0.5, 1.5)
    } else {
        1.0
    }
}

fn tab_base_content_color(colors: crate::theme::LiquidColors) -> Color {
    colors.label
}

fn tab_selection_content_color(colors: crate::theme::LiquidColors) -> Color {
    colors.accent
}

fn mix_color(from: Color, to: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return from;
    }
    if amount >= 1.0 {
        return to;
    }
    Color::rgba(
        from.r() + (to.r() - from.r()) * amount,
        from.g() + (to.g() - from.g()) * amount,
        from.b() + (to.b() - from.b()) * amount,
        from.a() + (to.a() - from.a()) * amount,
    )
}

fn tab_base_selected_color(colors: crate::theme::LiquidColors, activity: f32) -> Color {
    mix_color(colors.accent, colors.label, activity)
}

const BAR_HEIGHT: f32 = 64.0;
const BLOB_HEIGHT: f32 = 52.0;
const BLOB_MARGIN: f32 = 8.0;
const FLIGHT_LENS_WIDTH_FACTOR: f32 = 1.25;
const FLIGHT_LENS_HEIGHT_FACTOR: f32 = 1.20;
/// Width allotted to each tab inside the pill.
const TAB_WIDTH: f32 = 78.0;
/// Plain icon frame size (its path occupies about 25dp over 11dp labels).
const TAB_ICON_SIZE: f32 = 32.0;
const TAB_LABEL_SIZE: f32 = 11.0;
/// The drag lens overflows the pill vertically like the reference bubble.
const TAP_SLOP: f32 = 6.0;
const ACCESSORY_GAP: f32 = 10.0;

/// Layout parameters for a [`LiquidTabBar`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidTabBarSpec {
    max_tab_width: f32,
}

impl LiquidTabBarSpec {
    pub fn new(max_tab_width: f32) -> Self {
        Self {
            max_tab_width: if max_tab_width.is_finite() {
                max_tab_width.max(1.0)
            } else {
                TAB_WIDTH
            },
        }
    }
}

impl Default for LiquidTabBarSpec {
    fn default() -> Self {
        Self::new(TAB_WIDTH)
    }
}

fn tab_flight_lens_material(foreground: cranpose_ui_graphics::Color) -> Glass {
    Glass::lens()
        .no_clip()
        .tint(neutral_surface_tint(foreground, 0.13, 0.10))
        .blur_radius(0.0)
        .refraction_depth(0.26)
        .refraction_curve(0.82)
        .dispersion(0.16)
        .lift(0.0)
        .highlight(0.24)
}

fn tab_bar_surface_material(foreground: cranpose_ui_graphics::Color) -> Glass {
    Glass::regular()
        .tint(neutral_surface_tint(foreground, 0.0, 0.04))
        .blur_radius(4.0)
        .saturation(0.95)
        .lift(neutral_surface_lift(foreground, 0.48, -0.24))
        .highlight(0.20)
        .adaptive_frost(foreground, 0.42)
}

fn tab_flight_tint_multiplier(activity: f32) -> f32 {
    1.0 - 0.25 * activity.clamp(0.0, 1.0)
}

fn tab_lens_activity_motion() -> cranpose_animation::AnimationType {
    cranpose_animation::spring(1.0, 900.0)
}

fn tab_lens_left(pointer_x: f32, tab_width: f32, count: usize, has_accessory: bool) -> f32 {
    let last_tab = tab_width * count.saturating_sub(1) as f32;
    let min = if has_accessory { -tab_width * 0.2 } else { 0.0 };
    let max = if has_accessory {
        tab_width * (count as f32 - 0.45)
    } else {
        last_tab
    };
    (pointer_x - tab_width * 0.5).clamp(min, max)
}

fn tab_visual_selection(
    selected: usize,
    lens_position: f32,
    tab_width: f32,
    count: usize,
    direct: bool,
) -> usize {
    let selected = selected.min(count.saturating_sub(1));
    let state_position = tab_width * selected as f32;
    let lens_owns_selection =
        liquid_axis_owns_visual_selection(direct, lens_position, state_position, tab_width);
    liquid_visual_index(
        selected,
        lens_position,
        tab_width,
        count,
        lens_owns_selection,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct AppBadgeGeometry {
    size: Size,
    corner_radius: f32,
    stripe: Rect,
    glyph: Rect,
}

fn app_badge_geometry(optical_scale: f32) -> AppBadgeGeometry {
    let scale = normalize_icon_scale(optical_scale);
    AppBadgeGeometry {
        size: Size::new(20.0 * scale, 32.0 * scale),
        corner_radius: 5.0 * scale,
        stripe: Rect {
            x: 7.0 * scale,
            y: 4.5 * scale,
            width: 6.0 * scale,
            height: 1.5 * scale,
        },
        glyph: Rect {
            x: 3.0 * scale,
            y: 10.0 * scale,
            width: 14.0 * scale,
            height: 14.0 * scale,
        },
    }
}

#[composable]
#[allow(non_snake_case)]
fn TabIcon(icon: &'static str, style: LiquidTabIconStyle, color: Color, optical_scale: f32) {
    const FRAME_HEIGHT: f32 = 32.0;
    Box(
        Modifier::empty().size(Size::new(TAB_ICON_SIZE, FRAME_HEIGHT)),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || match style {
            LiquidTabIconStyle::Plain => {
                crate::icons::Icon(icon, TAB_ICON_SIZE * optical_scale, color)
            }
            LiquidTabIconStyle::AppBadge => {
                let geometry = app_badge_geometry(optical_scale);
                Box(
                    Modifier::empty()
                        .size(geometry.size)
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(color),
                                CornerRadii::uniform(geometry.corner_radius),
                            );
                            scope.draw_rect_at(geometry.stripe, Brush::solid(Color::WHITE));
                        }),
                    BoxSpec::default(),
                    move || {
                        Box(
                            Modifier::empty()
                                .offset(geometry.glyph.x, geometry.glyph.y)
                                .size(Size::new(geometry.glyph.width, geometry.glyph.height)),
                            BoxSpec::default(),
                            move || crate::icons::Icon(icon, geometry.glyph.width, Color::WHITE),
                        );
                    },
                );
            }
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct TabCellsSpec {
    base_color: Color,
    selected: Option<usize>,
    selected_color: Color,
    interactive: bool,
    selection_only: bool,
}

fn tab_cell_is_visible(index: usize, selected: Option<usize>, selection_only: bool) -> bool {
    !selection_only || selected == Some(index)
}

#[composable]
#[allow(non_snake_case)]
fn TabCells(
    modifier: Modifier,
    tabs: Rc<Vec<LiquidTab>>,
    typography: LiquidTypography,
    tab_width: f32,
    spec: TabCellsSpec,
) {
    Row(modifier, RowSpec::default(), move || {
        for (index, tab) in tabs.iter().enumerate() {
            let visible = tab_cell_is_visible(index, spec.selected, spec.selection_only);
            let color = if spec.selected == Some(index) {
                spec.selected_color
            } else {
                spec.base_color
            };
            let label_for_semantics = tab.label;
            let mut cell = Modifier::empty().size(Size::new(tab_width, BLOB_HEIGHT));
            if spec.interactive {
                cell = cell.semantics(move |config| {
                    config.is_button = true;
                    config.is_clickable = true;
                    config.content_description = Some(label_for_semantics.to_string());
                });
            }
            let icon = tab.icon;
            let icon_style = tab.icon_style;
            let icon_scale = tab.icon_scale;
            let label = tab.label;
            let label_style = TextStyle {
                span_style: SpanStyle {
                    color: Some(color),
                    font_size: cranpose_ui::text::TextUnit::Sp(TAB_LABEL_SIZE),
                    font_weight: Some(FontWeight::MEDIUM),
                    ..typography.caption1.span_style.clone()
                },
                ..typography.caption1.clone()
            };
            Box(
                cell,
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    if !visible {
                        return;
                    }
                    let label_style = label_style.clone();
                    Column(
                        Modifier::empty(),
                        ColumnSpec::default()
                            .horizontal_alignment(HorizontalAlignment::CenterHorizontally),
                        move || {
                            TabIcon(icon, icon_style, color, icon_scale);
                            Text(label, Modifier::empty(), label_style.clone());
                        },
                    );
                },
            );
        }
    });
}

fn tab_lens_node_top(node_height: f32) -> f32 {
    (BAR_HEIGHT - node_height) * 0.5
}

fn tab_bar_accessory_gap(has_accessory: bool) -> f32 {
    if has_accessory {
        ACCESSORY_GAP
    } else {
        0.0
    }
}

fn accessory_surfaces_touch(edge_gap: f32) -> bool {
    edge_gap <= 0.0
}

fn tab_lens_base_size(tab_width: f32, activity: f32) -> (f32, f32) {
    let activity = activity.clamp(0.0, 1.0);
    let ease = activity * activity * (3.0 - 2.0 * activity);
    let rest_width = tab_width * 1.08;
    let active_width = tab_width * FLIGHT_LENS_WIDTH_FACTOR;
    (
        rest_width + (active_width - rest_width) * ease,
        BLOB_HEIGHT + (BAR_HEIGHT * FLIGHT_LENS_HEIGHT_FACTOR - BLOB_HEIGHT) * ease,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabFlightGeometry {
    center: (f32, f32),
    base_size: Size,
    pose: crate::dynamics::LiquidPose,
    lens_position: f32,
    lens_activity: f32,
    resting_tint: Color,
    accessory_center: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabFlightNode {
    origin: (f32, f32),
    size: Size,
}

fn tab_flight_dynamics(geometry: TabFlightGeometry, node: TabFlightNode) -> GlassDynamics {
    let activity = geometry.lens_activity.clamp(0.0, 1.0);
    let energy = geometry.pose.energy() * activity;
    let radius = geometry.base_size.height * (0.48 + 0.02 * energy);
    let glue = 20.0;
    let shapes = geometry
        .accessory_center
        .filter(|(x, _)| {
            let edge_gap = (*x - geometry.center.0).abs()
                - geometry.base_size.width * geometry.pose.stretch.max(geometry.pose.ortho) * 0.5
                - BAR_HEIGHT * 0.5;
            accessory_surfaces_touch(edge_gap)
        })
        .map(|(x, y)| {
            vec![(
                x - node.origin.0,
                y - node.origin.1,
                BAR_HEIGHT,
                BAR_HEIGHT,
                -1.0,
            )]
        })
        .unwrap_or_default();
    GlassDynamics {
        morph: Some(GlassMorph {
            node_size: (node.size.width, node.size.height),
            primary: (
                geometry.center.0 - node.origin.0,
                geometry.center.1 - node.origin.1,
                geometry.base_size.width,
                geometry.base_size.height,
                radius,
            ),
            shapes,
            glue,
            wobble_amplitude: 1.1 * energy,
            wobble_phase: geometry.lens_position * 0.045,
            bulge_amplitude: geometry.pose.bulge_amplitude.min(8.0) * activity,
            bulge_direction: geometry.pose.bulge_direction,
            ellipse_blend: 0.0,
            deformation: Some(crate::material::GlassDeformation::incompressible(
                geometry.pose.axis,
                1.0 + (geometry.pose.stretch - 1.0) * activity,
            )),
        }),
        activity: Some(activity),
        resting_tint: Some(geometry.resting_tint),
        tint_alpha_multiplier: Some(tab_flight_tint_multiplier(geometry.lens_activity)),
        ..Default::default()
    }
}

/// A unified floating glass tab bar with every destination inside one pill.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBar(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    tabs: Vec<LiquidTab>,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
) {
    LiquidTabBarLayout(modifier, spec, tabs, selected, on_select, false, || {});
}

/// A floating glass tab bar with a detached accessory to its right.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBarWithAccessory(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    tabs: Vec<LiquidTab>,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    accessory: impl FnMut() + 'static,
) {
    LiquidTabBarLayout(modifier, spec, tabs, selected, on_select, true, accessory);
}

#[composable]
#[allow(non_snake_case)]
fn LiquidTabBarLayout(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    tabs: Vec<LiquidTab>,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    has_accessory: bool,
    accessory: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let count = tabs.len().max(1);
    let selected = selected.min(count - 1);
    let on_select: Rc<dyn Fn(usize)> = Rc::new(on_select);
    let tabs = Rc::new(tabs);
    let accessory = Rc::new(RefCell::new(accessory));

    Row(
        modifier,
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let tabs = Rc::clone(&tabs);
            let typography = typography.clone();
            let on_select = Rc::clone(&on_select);
            let accessory = Rc::clone(&accessory);

            // The main pill (wrapped in a stack so the drag lens can float
            // ABOVE the finished bar and magnify icons + glass together).
            let lens_x_outer = cranpose_core::remember(|| {
                cranpose_core::mutableStateOf((
                    0.0f32,
                    0.0f32,
                    0.0f32,
                    crate::dynamics::LiquidPose::default(),
                    0usize,
                ))
            })
            .with(|state| *state);
            // The stack is pinned to the pill height so the mounting lens
            // (taller than the bar) can never inflate it — an unpinned stack
            // grew on press and the centering Row shifted the whole bar down.
            Box(
                Modifier::empty().height(BAR_HEIGHT),
                BoxSpec::default(),
                move || {
                    let tabs = Rc::clone(&tabs);
                    let typography = typography.clone();
                    let on_select = Rc::clone(&on_select);
                    let selection_tabs = Rc::clone(&tabs);
                    let selection_typography = typography.clone();
                    // The bar's edge is defined by shadow and contrast, not a
                    // bright rim stroke.
                    let pill = Modifier::empty()
                        .glass_effect(
                            // Dark labels must never sink into dark content
                            // scrolling beneath: the glass lifts adaptively over
                            // dark backdrops (inert over the light ones the
                            // pinned captures use).
                            tab_bar_surface_material(colors.label),
                        )
                        .height(BAR_HEIGHT);
                    Box(pill, BoxSpec::default(), move || {
                        let tabs = Rc::clone(&tabs);
                        let typography = typography.clone();
                        let on_select = Rc::clone(&on_select);
                        BoxWithConstraints(Modifier::empty().padding(BLOB_MARGIN), move |scope| {
                            let tabs = Rc::clone(&tabs);
                            let typography = typography.clone();
                            let on_select = Rc::clone(&on_select);
                            let constrained = scope.constraints().max_width;
                            let tab_width = if constrained.is_finite() && constrained > 1.0 {
                                (constrained / count as f32).min(spec.max_tab_width)
                            } else {
                                spec.max_tab_width
                            };

                            // One optical body owns selection at rest, under direct
                            // manipulation, and throughout release settle.
                            let lens_pressed =
                                cranpose_core::remember(|| cranpose_core::mutableStateOf(false))
                                    .with(|state| *state);
                            let resting_lens_x = tab_width * selected as f32;
                            let lens_axis =
                                crate::motion::remember_liquid_drag_axis(resting_lens_x);
                            lens_axis.settle_to(resting_lens_x, LiquidMotion::glide());
                            let lens_x = lens_axis.value();
                            let lens_pose = lens_axis.liquid_pose();
                            let lens_settling = !lens_axis.is_dragging()
                                && (lens_x - resting_lens_x).abs() > tab_width * 0.75;
                            let lens_activity_target = if lens_pressed.get() || lens_settling {
                                1.0
                            } else {
                                0.0
                            };
                            let lens_activity_anim = cranpose_animation::animateFloatAsState(
                                lens_activity_target,
                                tab_lens_activity_motion(),
                                "tabbar-lens-activity",
                            );
                            let lens_activity = lens_activity_anim.get();
                            let visual_selection = tab_visual_selection(
                                selected,
                                lens_x,
                                tab_width,
                                count,
                                lens_axis.is_dragging(),
                            );
                            TabCells(
                                Modifier::empty(),
                                Rc::clone(&tabs),
                                typography.clone(),
                                tab_width,
                                TabCellsSpec {
                                    base_color: tab_base_content_color(colors),
                                    selected: Some(selected),
                                    selected_color: tab_base_selected_color(colors, lens_activity),
                                    interactive: true,
                                    selection_only: false,
                                },
                            );

                            // Swipe/tap surface across the whole pill interior.
                            let row_width = tab_width * count as f32;
                            let gesture = Modifier::empty()
                                .size(Size::new(row_width, BLOB_HEIGHT))
                                .pointer_input(selected, {
                                    let on_select = Rc::clone(&on_select);
                                    let lens_axis = Rc::clone(&lens_axis);
                                    move |scope: PointerInputScope| {
                                        let on_select = Rc::clone(&on_select);
                                        let lens_axis = Rc::clone(&lens_axis);
                                        async move {
                                            scope
                                                .await_pointer_event_scope(
                                                    |await_scope| async move {
                                                        let mut down_x = 0.0f32;
                                                        let mut active_pointer =
                                                            Option::<PointerId>::None;
                                                        let mut moved = false;
                                                        loop {
                                                            let event = await_scope
                                                                .await_pointer_event()
                                                                .await;
                                                            match event.kind {
                                                                PointerEventKind::Down
                                                                    if active_pointer.is_none() =>
                                                                {
                                                                    active_pointer = Some(event.id);
                                                                    moved = false;
                                                                    down_x = event.position.x;
                                                                    lens_axis.begin(
                                                                        tab_lens_left(
                                                                            event.position.x,
                                                                            tab_width,
                                                                            count,
                                                                            has_accessory,
                                                                        ),
                                                                        event.time_ms,
                                                                    );
                                                                    lens_pressed.set(true);
                                                                    default_haptics().perform(
                                                                        HapticFeedback::Selection,
                                                                    );
                                                                    event.consume();
                                                                }
                                                                PointerEventKind::Move
                                                                    if active_pointer
                                                                        == Some(event.id) =>
                                                                {
                                                                    moved |= (event.position.x
                                                                        - down_x)
                                                                        .abs()
                                                                        > TAP_SLOP;
                                                                    lens_axis.move_to(
                                                                        tab_lens_left(
                                                                            event.position.x,
                                                                            tab_width,
                                                                            count,
                                                                            has_accessory,
                                                                        ),
                                                                        event.time_ms,
                                                                    );
                                                                    event.consume();
                                                                }
                                                                PointerEventKind::Up
                                                                    if active_pointer
                                                                        == Some(event.id) =>
                                                                {
                                                                    active_pointer = None;
                                                                    lens_pressed.set(false);
                                                                    let commit_x = if moved {
                                                                        event.position.x
                                                                    } else {
                                                                        down_x
                                                                    };
                                                                    let index = ((commit_x
                                                                        / tab_width)
                                                                        .floor()
                                                                        as isize)
                                                                        .clamp(
                                                                            0,
                                                                            count as isize - 1,
                                                                        )
                                                                        as usize;
                                                                    lens_axis.release_to(
                                                                        tab_width * index as f32,
                                                                        event.time_ms,
                                                                        LiquidMotion::glide(),
                                                                    );
                                                                    default_haptics().perform(
                                                                        HapticFeedback::ImpactLight,
                                                                    );
                                                                    on_select(index);
                                                                    event.consume();
                                                                }
                                                                PointerEventKind::Cancel
                                                                    if active_pointer
                                                                        == Some(event.id) =>
                                                                {
                                                                    active_pointer = None;
                                                                    lens_pressed.set(false);
                                                                    lens_axis.release_to(
                                                                        resting_lens_x,
                                                                        event.time_ms,
                                                                        LiquidMotion::glide(),
                                                                    );
                                                                    event.consume();
                                                                }
                                                                _ => {}
                                                            }
                                                        }
                                                    },
                                                )
                                                .await;
                                        }
                                    }
                                });
                            Box(gesture, BoxSpec::default(), || {});

                            // Publish the lens springs for the overlay rendered
                            // ABOVE the finished bar (outside this glass layer, so
                            // the lens magnifies icons and glass together).
                            let published = (
                                lens_x,
                                lens_activity,
                                tab_width,
                                lens_pose,
                                visual_selection,
                            );
                            if lens_x_outer.get() != published {
                                lens_x_outer.set(published);
                            }
                        });
                    });

                    // The lens bubble, floating above the whole pill. Its shape
                    // follows the droplet law (crate::dynamics): cruising speed
                    // stretches it along the travel axis, launch compresses it,
                    // braking swells the leading edge — orthogonal axis inverse,
                    // area conserved — and it magnifies harder in motion. The
                    // search accessory's circle joins its liquid field: drag the
                    // lens to the bar's end and the two glue through a
                    // smooth-union neck.
                    let (lens_px, lens_activity, lens_tab_w, pose, visual_selection) =
                        lens_x_outer.get();
                    let lens_w = lens_tab_w * FLIGHT_LENS_WIDTH_FACTOR;
                    let lens_h = BAR_HEIGHT * FLIGHT_LENS_HEIGHT_FACTOR;
                    // Node headroom for the deformation extremes (max axis
                    // stretch + leading bulge, max ortho swell) and rim glow.
                    let deformation_headroom =
                        crate::dynamics::STRETCH_MAX.max(1.0 / crate::dynamics::STRETCH_MIN);
                    let node_w = lens_w * deformation_headroom + crate::dynamics::BULGE_MAX + 20.0;
                    let node_h = lens_h * deformation_headroom + crate::dynamics::BULGE_MAX + 16.0;
                    let lens_center_x = BLOB_MARGIN + lens_px + lens_tab_w * 0.5;
                    let node_x = lens_center_x - node_w * 0.5;
                    let node_top = tab_lens_node_top(node_h);
                    let pill_w = lens_tab_w * count as f32 + 2.0 * BLOB_MARGIN;
                    let (base_w, base_h) = tab_lens_base_size(lens_tab_w, lens_activity);
                    let geometry = TabFlightGeometry {
                        center: (lens_center_x, BAR_HEIGHT * 0.5),
                        base_size: Size::new(base_w, base_h),
                        pose,
                        lens_position: lens_px,
                        lens_activity,
                        resting_tint: colors.fill,
                        accessory_center: has_accessory.then_some((
                            pill_w + tab_bar_accessory_gap(true) + BAR_HEIGHT * 0.5,
                            BAR_HEIGHT * 0.5,
                        )),
                    };
                    let mask_node = TabFlightNode {
                        origin: (0.0, 0.0),
                        size: Size::new(pill_w, BAR_HEIGHT),
                    };
                    let lens_node = TabFlightNode {
                        origin: (node_x, node_top),
                        size: Size::new(node_w, node_h),
                    };

                    let selection_geometry = geometry;
                    let selection_mask = liquid_content_mask_with(
                        Modifier::empty().required_size(mask_node.size),
                        tab_flight_lens_material(colors.label),
                        move || tab_flight_dynamics(selection_geometry, mask_node),
                    );
                    Box(selection_mask, BoxSpec::default(), move || {
                        TabCells(
                            Modifier::empty().offset(BLOB_MARGIN, BLOB_MARGIN),
                            Rc::clone(&selection_tabs),
                            selection_typography.clone(),
                            lens_tab_w,
                            TabCellsSpec {
                                base_color: tab_selection_content_color(colors),
                                selected: Some(visual_selection),
                                selected_color: tab_selection_content_color(colors),
                                interactive: false,
                                selection_only: true,
                            },
                        );
                    });

                    let lens_geometry = geometry;
                    let lens = Modifier::empty()
                        // required_size: the stack is pinned to BAR_HEIGHT so
                        // the taller lens can never inflate the bar; the node
                        // still measures (and draws) at its full size and the
                        // offset centers it on the pill.
                        .required_size(lens_node.size)
                        .offset(node_x, node_top)
                        .glass_effect_with(tab_flight_lens_material(colors.label), move || {
                            tab_flight_dynamics(lens_geometry, lens_node)
                        });
                    Box(lens, BoxSpec::default(), || {});
                },
            );

            if has_accessory {
                Box(
                    Modifier::empty().width(tab_bar_accessory_gap(true)),
                    BoxSpec::default(),
                    || {},
                );
                (accessory.borrow_mut())();
            }
        },
    );
}

/// The standard detached accessory: a circular glass search button.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBarSearchAccessory(on_click: impl Fn() + 'static) {
    // The reference search circle is nearly flush with the bar height.
    crate::widgets::GlassIconButton(
        Modifier::empty(),
        crate::widgets::GlassButtonSpec::glass(),
        BAR_HEIGHT * 0.94,
        on_click,
        crate::icons::SEARCH,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_bar_spec_normalizes_the_maximum_cell_width() {
        assert_eq!(LiquidTabBarSpec::default().max_tab_width, TAB_WIDTH);
        assert_eq!(LiquidTabBarSpec::new(85.0).max_tab_width, 85.0);
        assert_eq!(LiquidTabBarSpec::new(0.0).max_tab_width, 1.0);
        assert_eq!(LiquidTabBarSpec::new(f32::NAN).max_tab_width, TAB_WIDTH);
    }

    #[test]
    fn drag_pointer_centers_the_lens_and_preserves_end_overdrag() {
        let width = 100.0;
        assert_eq!(tab_lens_left(50.0, width, 4, true), 0.0);
        assert_eq!(tab_lens_left(250.0, width, 4, true), 200.0);
        assert_eq!(tab_lens_left(-100.0, width, 4, true), -20.0);
        assert_eq!(tab_lens_left(500.0, width, 4, true), 355.0);

        assert_eq!(tab_lens_left(-100.0, width, 4, false), 0.0);
        assert_eq!(tab_lens_left(500.0, width, 4, false), 300.0);
    }

    #[test]
    fn flight_lens_node_is_centered_on_the_bar_axis() {
        for node_height in [48.0, 64.0, 96.0, 128.0] {
            let center = tab_lens_node_top(node_height) + node_height * 0.5;
            assert!((center - BAR_HEIGHT * 0.5).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn liquid_tab_builds_reference_content() {
        assert_eq!(TAB_ICON_SIZE, 32.0);
        let tab = LiquidTab::new(crate::icons::STAR, "Discover");
        assert_eq!(tab.icon, crate::icons::STAR);
        assert_eq!(tab.label, "Discover");
        assert_eq!(tab.icon_style, LiquidTabIconStyle::Plain);

        let badge = LiquidTab::app_badge(crate::icons::APPLE, "WWDC");
        assert_eq!(badge.icon_style, LiquidTabIconStyle::AppBadge);

        let compact = LiquidTab::new(crate::icons::ACCOUNT_CIRCLE, "Account").with_icon_scale(0.72);
        assert!((compact.icon_scale - 0.72).abs() < f32::EPSILON);
        assert_eq!(tab.clone().with_icon_scale(f32::NAN).icon_scale, 1.0);
        assert_eq!(tab.with_icon_scale(2.0).icon_scale, 1.5);
    }

    #[test]
    fn app_badge_geometry_honors_the_tab_optical_scale() {
        let full = app_badge_geometry(1.0);
        let corrected = app_badge_geometry(0.85);
        assert_eq!(full.size, Size::new(20.0, 32.0));
        assert_eq!(corrected.size, Size::new(17.0, 27.2));
        assert!((corrected.glyph.width - 11.9).abs() < 1.0e-5);
        assert!((corrected.corner_radius / full.corner_radius - 0.85).abs() < f32::EPSILON);
        assert!((corrected.stripe.x / full.stripe.x - 0.85).abs() < f32::EPSILON);
        assert!((corrected.glyph.width / full.glyph.width - 0.85).abs() < f32::EPSILON);
    }

    #[test]
    fn base_tab_content_remains_neutral_under_the_moving_selection_layer() {
        let colors = crate::theme::LiquidColors::light(cranpose_ui_graphics::Color::from_rgb_u8(
            0, 122, 255,
        ));
        assert_eq!(tab_base_content_color(colors), colors.label);
        assert_eq!(tab_selection_content_color(colors), colors.accent);
    }

    #[test]
    fn selected_tab_accent_crossfades_into_the_persistent_flight_layer() {
        let colors = crate::theme::LiquidColors::light(cranpose_ui_graphics::Color::from_rgb_u8(
            0, 122, 255,
        ));
        assert_eq!(tab_base_selected_color(colors, 0.0), colors.accent);
        assert_eq!(tab_base_selected_color(colors, 1.0), colors.label);
        let halfway = tab_base_selected_color(colors, 0.5);
        assert!((halfway.r() - (colors.accent.r() + colors.label.r()) * 0.5).abs() < 1.0e-6);
        assert!((halfway.g() - (colors.accent.g() + colors.label.g()) * 0.5).abs() < 1.0e-6);
        assert!((halfway.b() - (colors.accent.b() + colors.label.b()) * 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn flight_overlay_accents_only_the_lens_owned_cell() {
        assert_eq!(
            tab_visual_selection(3, TAB_WIDTH * 2.5, TAB_WIDTH, 4, true),
            2
        );
        assert!(tab_cell_is_visible(2, Some(2), true));
        assert!(!tab_cell_is_visible(3, Some(2), true));
        assert!(tab_cell_is_visible(3, Some(2), false));
    }

    #[test]
    fn selection_mask_and_lens_resolve_the_same_global_sdf() {
        let geometry = TabFlightGeometry {
            center: (212.0, 32.0),
            base_size: Size::new(106.0, 64.0),
            pose: crate::dynamics::LiquidPose::default(),
            lens_position: 160.0,
            lens_activity: 1.0,
            resting_tint: Color::BLACK.with_alpha(0.10),
            accessory_center: None,
        };
        let mask_node = TabFlightNode {
            origin: (0.0, 0.0),
            size: Size::new(328.0, 64.0),
        };
        let lens_node = TabFlightNode {
            origin: (132.0, -22.0),
            size: Size::new(160.0, 108.0),
        };
        let mask = tab_flight_dynamics(geometry, mask_node)
            .morph
            .expect("selection mask morph");
        let lens = tab_flight_dynamics(geometry, lens_node)
            .morph
            .expect("lens morph");
        assert_eq!(
            (
                mask.primary.0 + mask_node.origin.0,
                mask.primary.1 + mask_node.origin.1
            ),
            geometry.center
        );
        assert_eq!(
            (
                lens.primary.0 + lens_node.origin.0,
                lens.primary.1 + lens_node.origin.1
            ),
            geometry.center
        );
        assert_eq!(
            (mask.primary.2, mask.primary.3, mask.primary.4),
            (lens.primary.2, lens.primary.3, lens.primary.4)
        );
        assert_eq!(mask.node_size, (328.0, 64.0));
        assert_eq!(lens.node_size, (160.0, 108.0));
    }

    #[test]
    fn unified_bar_has_no_detached_accessory_gap() {
        assert_eq!(tab_bar_accessory_gap(false), 0.0);
        assert_eq!(tab_bar_accessory_gap(true), 10.0);
    }

    #[test]
    fn flight_lens_only_joins_accessory_after_surface_contact() {
        assert!(!accessory_surfaces_touch(0.01));
        assert!(accessory_surfaces_touch(0.0));
        assert!(accessory_surfaces_touch(-4.0));
    }

    #[test]
    fn one_lens_morphs_between_rest_and_full_flight_footprints() {
        let width = TAB_WIDTH * FLIGHT_LENS_WIDTH_FACTOR;
        let height = BAR_HEIGHT * FLIGHT_LENS_HEIGHT_FACTOR;
        assert!(
            (1.23..=1.27).contains(&(width / TAB_WIDTH)),
            "the undeformed optic must span the target's measured active footprint: {width}"
        );
        assert!(
            (1.18..=1.22).contains(&(height / BAR_HEIGHT)),
            "the active optic must rise beyond the resting bar footprint: {height}"
        );
        assert_eq!(
            tab_lens_base_size(TAB_WIDTH, 0.0),
            (TAB_WIDTH * 1.08, BLOB_HEIGHT)
        );
        assert_eq!(tab_lens_base_size(TAB_WIDTH, 1.0), (width, height));
    }

    #[test]
    fn tab_grid_matches_the_reference_pitch() {
        assert_eq!(TAB_WIDTH, 78.0);
    }

    #[test]
    fn tab_grid_matches_the_reference_inner_inset() {
        assert_eq!(BLOB_MARGIN, 8.0);
    }

    #[test]
    fn flight_lens_uses_the_clear_wcksrd_contract() {
        let glass = tab_flight_lens_material(cranpose_ui_graphics::Color::BLACK);
        let generic_lens = Glass::lens();
        assert!(glass
            .lift
            .is_some_and(|lift| (-0.02..=0.02).contains(&lift)));
        assert!(glass.refraction_depth < generic_lens.refraction_depth);
        assert!(glass.refraction_curve < generic_lens.refraction_curve);
        assert!(glass.dispersion < generic_lens.dispersion);
        assert_eq!(glass.blur_radius, Some(0.0));
        assert!(glass.highlight < generic_lens.highlight);
        assert!(
            glass.shadow,
            "the moving lens needs its target-visible SDF contact outline"
        );
        assert!(glass
            .tint
            .is_some_and(|tint| { tint.r() < 0.05 && (0.125..=0.135).contains(&tint.a()) }));
        assert_eq!(glass.adaptive_frost, 0.0);
    }

    #[test]
    fn flight_lens_retains_neutral_tint_through_direct_motion() {
        assert_eq!(tab_flight_tint_multiplier(0.0), 1.0);
        assert!((tab_flight_tint_multiplier(1.0) - 0.75).abs() < f32::EPSILON);
        assert_eq!(tab_flight_tint_multiplier(-1.0), 1.0);
        assert!((tab_flight_tint_multiplier(2.0) - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn bar_surface_adapts_frost_to_its_foreground() {
        let glass = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK);
        assert_eq!(glass.blur_radius, Some(4.0));
        assert_eq!(glass.saturation, Some(0.95));
        assert_eq!(glass.lift, Some(0.48));
        assert_eq!(glass.refraction_depth, 0.34);
        assert_eq!(glass.adaptive_frost, 0.42);
    }

    #[test]
    fn bar_surface_lift_tracks_the_local_foreground_polarity() {
        let light_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK);
        assert_eq!(light_surface.lift, Some(0.48));

        let dark_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::WHITE);
        assert_eq!(dark_surface.lift, Some(-0.24));
    }

    #[test]
    fn bar_surface_tint_separates_from_same_polarity_backdrops() {
        let light_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK)
            .tint
            .expect("bar tint");
        assert!(light_surface.r() < 0.05);
        assert_eq!(light_surface.a(), 0.0);

        let dark_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::WHITE)
            .tint
            .expect("bar tint");
        assert!(dark_surface.r() > 0.95);
        assert!((0.03..=0.05).contains(&dark_surface.a()));
    }

    #[test]
    fn arrival_contraction_uses_the_measured_fast_settle() {
        let cranpose_animation::AnimationType::Spring(settle) = tab_lens_activity_motion() else {
            panic!("arrival contraction must use a spring");
        };
        assert_eq!(settle.damping_ratio, 1.0);
        assert_eq!(settle.stiffness, 900.0);
    }
}
