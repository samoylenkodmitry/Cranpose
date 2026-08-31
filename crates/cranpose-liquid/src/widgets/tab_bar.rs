use std::{cell::RefCell, rc::Rc};

use cranpose_macros::composable;
use cranpose_ui::{
    Brush, Color, CornerRadii, Modifier, PointerInputScope, Rect, SemanticsWidgetRole, Size,
    text::{FontWeight, SpanStyle, TextStyle},
    widgets::{
        Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Column, ColumnSpec, Row,
        RowSpec, Text,
    },
};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, VerticalAlignment};

use crate::{
    material::{
        Glass, GlassDynamics, GlassMorph, GlassShadow, LiquidModifierExt, neutral_surface_lift,
        neutral_surface_tint,
    },
    motion::LiquidMotion,
    theme::{LiquidTypography, liquid_colors, liquid_typography},
    widgets::content_scope::ScopeContent,
};

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

const BAR_HEIGHT: f32 = 64.0;
const BLOB_HEIGHT: f32 = 56.0;
const BLOB_MARGIN: f32 = 4.0;
const FLIGHT_LENS_HEIGHT_PROJECTION: f32 = 68.0 / BLOB_HEIGHT;
const FLIGHT_ELLIPSE_BLEND: f32 = 0.25;
const TAB_LENS_REST_WIDTH_FACTOR: f32 = 1.10;
const TAB_STRAIN_RESPONSE: f32 = 0.30;
const TAB_WIDTH: f32 = 78.0;
const TAB_ICON_SIZE: f32 = 32.0;
const TAB_LABEL_SIZE: f32 = 11.0;
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

fn tab_flight_lens_material(foreground: cranpose_ui_graphics::Color, accent: Color) -> Glass {
    Glass::lens()
        .no_clip()
        .tint(neutral_surface_tint(foreground, 0.06, 0.05))
        .ink_recolor(accent, 0.85)
        .blur_radius(0.0)
        .refraction_depth(1.0)
        .refraction_curve(0.25)
        .optical_zoom(1.22)
        .fold_depth(2.5)
        .dispersion(0.9)
        .lift(0.05)
        .highlight(0.18)
        .shadow_style(GlassShadow::new(
            cranpose_ui_graphics::Color::BLACK.with_alpha(0.14),
            12.0,
            4.0,
            -2.0,
        ))
}

fn tab_bar_surface_material(foreground: cranpose_ui_graphics::Color) -> Glass {
    Glass::regular()
        .tint(neutral_surface_tint(foreground, 0.0, 0.04))
        .blur_radius(9.0)
        .saturation(1.15)
        .lift(neutral_surface_lift(foreground, 0.60, -0.24))
        .highlight(0.20)
        .fold_depth(8.0)
        .adaptive_frost(foreground, 0.28)
}

fn tab_flight_tint_multiplier(activity: f32) -> f32 {
    1.0 - 0.25 * activity.clamp(0.0, 1.0)
}

fn tab_lens_activity_motion(raised: bool) -> cranpose_animation::AnimationType {
    if raised {
        cranpose_animation::spring(0.9, 1400.0)
    } else {
        cranpose_animation::spring(1.0, 900.0)
    }
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

/// The settled lens position for a selected cell: CELL-CENTERED at every
/// index, exactly like the reference (bottom-bar-click f_0000 measures the
/// end bubble's center on its cell center, its edge flush with the pill's
/// rounded end). The rest width (`TAB_LENS_REST_WIDTH_FACTOR`) is what
/// keeps the end cells legal — its overhang stays within `BLOB_MARGIN`.
/// Public so alignment tests assert the same rule the widget settles to.
pub fn tab_lens_resting_left(selected: usize, tab_width: f32, count: usize) -> f32 {
    tab_width * selected.min(count.saturating_sub(1)) as f32
}

/// The resting bubble's width for a cell pitch — the second half of the
/// public resting contract ([`tab_lens_resting_left`] gives the position).
///
/// A cell-centered bubble on an end cell overhangs the cell strip by half its
/// excess, and the pill only extends `BLOB_MARGIN` past that strip, so the
/// overhang is what decides whether the end bubble lands flush inside the
/// pill's rounded end or crosses it. `TAB_LENS_REST_WIDTH_FACTOR` sets the
/// reference proportion; the margin caps it, so a bar built with wide cells
/// keeps its ends legal instead of poking the bubble outside the pill.
pub fn tab_lens_rest_width(tab_width: f32) -> f32 {
    let overhang = (tab_width * (TAB_LENS_REST_WIDTH_FACTOR - 1.0) * 0.5).min(BLOB_MARGIN);
    tab_width + 2.0 * overhang
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
                    config.role = Some(SemanticsWidgetRole::Button);
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
    if has_accessory { ACCESSORY_GAP } else { 0.0 }
}

fn accessory_surfaces_touch(edge_gap: f32) -> bool {
    edge_gap <= 0.0
}

fn tab_lens_base_size(tab_width: f32, activity: f32) -> (f32, f32) {
    let activity = activity.clamp(0.0, 1.0);
    let ease = activity * activity * (3.0 - 2.0 * activity);
    let rest_width = tab_lens_rest_width(tab_width);
    let projection = 1.0 + (FLIGHT_LENS_HEIGHT_PROJECTION - 1.0) * ease;
    (rest_width, BLOB_HEIGHT * projection)
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
            let effective_stretch =
                1.0 + (geometry.pose.stretch.max(geometry.pose.ortho) - 1.0) * TAB_STRAIN_RESPONSE;
            let edge_gap = (*x - geometry.center.0).abs()
                - geometry.base_size.width * effective_stretch * 0.5
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
            wobble_amplitude: 0.5 * energy,
            wobble_phase: geometry.lens_position * 0.045,
            bulge_amplitude: geometry.pose.bulge_amplitude.min(8.0) * activity,
            bulge_direction: geometry.pose.bulge_direction,
            ellipse_blend: FLIGHT_ELLIPSE_BLEND,
            deformation: Some(crate::material::GlassDeformation::incompressible(
                geometry.pose.axis,
                1.0 + (geometry.pose.stretch - 1.0) * activity * TAB_STRAIN_RESPONSE,
            )),
            zoom_anchor: (0.0, 0.0),
        }),
        activity: Some(activity),
        press_depth: Some(0.3 + 0.7 * activity),
        resting_tint: Some(geometry.resting_tint),
        tint_alpha_multiplier: Some(tab_flight_tint_multiplier(geometry.lens_activity)),
        ..Default::default()
    }
}

/// The scope a tab bar's destinations are declared in.
///
/// Each call adds one destination, in the order it is made, which is also the
/// order the indices passed to `on_select` count in.
pub struct LiquidTabBarScope {
    tabs: ScopeContent<LiquidTab>,
}

impl LiquidTabBarScope {
    /// A destination showing `icon` above `label`.
    pub fn tab(&self, icon: &'static str, label: &'static str) {
        self.push(LiquidTab::new(icon, label));
    }

    /// A destination whose icon is drawn as an application badge.
    pub fn app_badge(&self, icon: &'static str, label: &'static str) {
        self.push(LiquidTab::app_badge(icon, label));
    }

    /// A destination built out, for the optical corrections a particular
    /// symbol needs — see [`LiquidTab`].
    pub fn push(&self, tab: LiquidTab) {
        self.tabs.push(tab);
    }
}

fn collect_tabs(content: impl FnOnce(&LiquidTabBarScope)) -> Vec<LiquidTab> {
    ScopeContent::collect(|tabs| LiquidTabBarScope { tabs }, content)
}

/// A unified floating glass tab bar with every destination inside one pill.
///
/// ```rust,ignore
/// LiquidTabBar(Modifier::empty(), LiquidTabBarSpec::default(), selected, on_select, |scope| {
///     scope.tab(icons::DOCUMENT, "Today");
///     scope.tab(icons::SEARCH, "Search");
/// });
/// ```
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBar(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    content: impl FnOnce(&LiquidTabBarScope),
) {
    LiquidTabBarLayout(
        modifier,
        spec,
        collect_tabs(content),
        selected,
        on_select,
        false,
        || {},
    );
}

/// A floating glass tab bar with a detached accessory to its right.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBarWithAccessory(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    content: impl FnOnce(&LiquidTabBarScope),
    accessory: impl FnMut() + 'static,
) {
    LiquidTabBarLayout(
        modifier,
        spec,
        collect_tabs(content),
        selected,
        on_select,
        true,
        accessory,
    );
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

            let lens_x_outer = cranpose_core::remember(|| {
                cranpose_core::mutableStateOf((
                    0.0f32,
                    0.0f32,
                    0.0f32,
                    crate::dynamics::LiquidPose::default(),
                ))
            })
            .with(|state| *state);
            let bar_touch =
                cranpose_core::remember(|| cranpose_core::mutableStateOf((0.0f32, 0.0f32)))
                    .with(|state| *state);
            let bar_held = cranpose_core::remember(|| cranpose_core::mutableStateOf(false))
                .with(|state| *state);
            let bar_press = cranpose_animation::animateFloatAsState(
                if bar_held.get() { 1.0 } else { 0.0 },
                cranpose_animation::spring(1.0, 600.0),
                "tabbar-hold-press",
            );
            let bar_lift = Modifier::empty().graphics_layer(move || {
                let press = bar_press.get().clamp(0.0, 1.0);
                let rise = 1.0 + 0.03 * press;
                cranpose_ui_graphics::GraphicsLayer {
                    scale_x: rise,
                    scale_y: rise,
                    translation_y: -2.5 * press,
                    ..Default::default()
                }
            });
            Box(
                bar_lift.then(Modifier::empty().height(BAR_HEIGHT)),
                BoxSpec::default(),
                move || {
                    let tabs = Rc::clone(&tabs);
                    let typography = typography.clone();
                    let on_select = Rc::clone(&on_select);
                    let pill = Modifier::empty()
                        .glass_effect_with(tab_bar_surface_material(colors.label), move || {
                            let press = bar_press.get().clamp(0.0, 1.0);
                            let (touch_x, touch_y) = bar_touch.get();
                            GlassDynamics {
                                highlight_boost: 0.45 * press,
                                saturation_boost: 0.12 * press,
                                touch: (press > 0.01).then_some((touch_x, touch_y, press)),
                                press_depth: Some(press),
                                ..Default::default()
                            }
                        })
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

                            let lens_pressed =
                                cranpose_core::remember(|| cranpose_core::mutableStateOf(false))
                                    .with(|state| *state);
                            let resting_lens_x = tab_lens_resting_left(selected, tab_width, count);
                            let lens_axis =
                                crate::motion::remember_liquid_drag_axis(resting_lens_x);
                            if !lens_pressed.get() {
                                lens_axis.settle_to(resting_lens_x, LiquidMotion::glide());
                            }
                            let lens_x = lens_axis.value();
                            let lens_pose = lens_axis.liquid_pose();
                            let lens_in_flight = !lens_axis.is_dragging()
                                && (lens_x - resting_lens_x).abs() > tab_width * 0.15;
                            let lens_raised = lens_pressed.get() || lens_in_flight;
                            let lens_activity_target = if lens_raised { 1.0 } else { 0.0 };
                            let lens_activity_anim = cranpose_animation::animateFloatAsState(
                                lens_activity_target,
                                tab_lens_activity_motion(lens_raised),
                                "tabbar-lens-activity",
                            );
                            let lens_activity = lens_activity_anim.get();
                            let visual_index = crate::motion::liquid_visual_index(
                                selected,
                                lens_x,
                                tab_width,
                                count,
                                crate::motion::liquid_axis_owns_visual_selection(
                                    lens_pressed.get(),
                                    lens_x,
                                    resting_lens_x,
                                    tab_width,
                                ),
                            );
                            TabCells(
                                Modifier::empty(),
                                Rc::clone(&tabs),
                                typography.clone(),
                                tab_width,
                                TabCellsSpec {
                                    base_color: tab_base_content_color(colors),
                                    selected: Some(visual_index),
                                    selected_color: tab_selection_content_color(colors),
                                    interactive: true,
                                    selection_only: false,
                                },
                            );

                            let row_width = tab_width * count as f32;
                            let gesture = Modifier::empty()
                                .size(Size::new(row_width, BLOB_HEIGHT))
                                .pointer_input(selected, {
                                    let on_select = Rc::clone(&on_select);
                                    let lens_axis = Rc::clone(&lens_axis);
                                    move |scope: PointerInputScope| {
                                        let on_select = Rc::clone(&on_select);
                                        let lens_axis = Rc::clone(&lens_axis);
                                        crate::motion::liquid_lens_gesture(
                                            scope,
                                            crate::motion::LiquidLensGesture {
                                                axis: lens_axis,
                                                cell_width: tab_width,
                                                count,
                                                tap_slop: TAP_SLOP,
                                                drag_left: Rc::new(move |x| {
                                                    tab_lens_left(
                                                        x,
                                                        tab_width,
                                                        count,
                                                        has_accessory,
                                                    )
                                                }),
                                                rest_left: Rc::new(move |index| {
                                                    tab_lens_resting_left(index, tab_width, count)
                                                }),
                                                selected,
                                                on_pressed: Rc::new(move |down| {
                                                    lens_pressed.set(down);
                                                    bar_held.set(down);
                                                }),
                                                on_touch: Rc::new(move |x, y| {
                                                    bar_touch
                                                        .set((x + BLOB_MARGIN, y + BLOB_MARGIN));
                                                }),
                                                on_select,
                                            },
                                        )
                                    }
                                });
                            Box(gesture, BoxSpec::default(), || {});

                            let published = (lens_x, lens_activity, tab_width, lens_pose);
                            if lens_x_outer.get() != published {
                                lens_x_outer.set(published);
                            }
                        });
                    });

                    let (lens_px, lens_activity, lens_tab_w, pose) = lens_x_outer.get();
                    let lens_w = tab_lens_rest_width(lens_tab_w);
                    let lens_h = BLOB_HEIGHT * FLIGHT_LENS_HEIGHT_PROJECTION;
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
                    let lens_node = TabFlightNode {
                        origin: (node_x, node_top),
                        size: Size::new(node_w, node_h),
                    };

                    let lens_geometry = geometry;
                    let lens = Modifier::empty()
                        .required_size(lens_node.size)
                        .offset(node_x, node_top)
                        .glass_effect_with(
                            tab_flight_lens_material(
                                colors.label,
                                tab_selection_content_color(colors),
                            ),
                            move || tab_flight_dynamics(lens_geometry, lens_node),
                        );
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
    fn resting_lens_centers_on_its_cell_and_stays_inside_the_pill() {
        let tab = 78.0;
        assert_eq!(tab_lens_resting_left(0, tab, 5), 0.0);
        assert_eq!(tab_lens_resting_left(1, tab, 5), tab);
        assert_eq!(tab_lens_resting_left(3, tab, 5), 3.0 * tab);
        assert_eq!(tab_lens_resting_left(4, tab, 5), 4.0 * tab);
        assert_eq!(tab_lens_resting_left(9, tab, 5), 4.0 * tab);
        let overhang = (tab_lens_rest_width(tab) - tab) * 0.5;
        assert!(overhang <= BLOB_MARGIN + 1.0e-4);
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
    fn lens_contact_swell_is_vertical_only() {
        let resting = tab_lens_base_size(TAB_WIDTH, 0.0);
        let raised = tab_lens_base_size(TAB_WIDTH, 1.0);
        assert_eq!(
            resting,
            (TAB_WIDTH * TAB_LENS_REST_WIDTH_FACTOR, BLOB_HEIGHT)
        );
        assert_eq!(raised.0, resting.0);
        assert!((raised.1 / resting.1 - FLIGHT_LENS_HEIGHT_PROJECTION).abs() < 0.001);
        assert!((raised.1 - 68.0).abs() < 0.5);
    }

    #[test]
    fn tab_grid_matches_the_reference_pitch() {
        assert_eq!(TAB_WIDTH, 78.0);
    }

    #[test]
    fn tab_grid_matches_the_reference_inner_inset() {
        assert_eq!(BLOB_MARGIN, 4.0);
        assert_eq!(BLOB_HEIGHT + 2.0 * BLOB_MARGIN, BAR_HEIGHT);
    }

    #[test]
    fn flight_lens_uses_the_clear_wcksrd_contract() {
        let glass = tab_flight_lens_material(
            cranpose_ui_graphics::Color::BLACK,
            cranpose_ui_graphics::Color::from_rgb_u8(0, 122, 255),
        );
        let generic_lens = Glass::lens();
        assert!(glass.lift.is_some_and(|lift| (0.0..=0.15).contains(&lift)));
        assert_eq!(glass.refraction_depth, 1.0);
        assert!(glass.refraction_curve < generic_lens.refraction_curve);
        assert!(glass.dispersion * 0.3 < generic_lens.dispersion);
        assert_eq!(glass.blur_radius, Some(0.0));
        assert!(glass.highlight < generic_lens.highlight);
        assert!(
            glass.shadow,
            "the moving lens needs its target-visible SDF contact outline"
        );
        assert!(
            glass
                .tint
                .is_some_and(|tint| { tint.r() < 0.05 && (0.055..=0.065).contains(&tint.a()) })
        );
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
        assert_eq!(glass.blur_radius, Some(9.0));
        assert_eq!(glass.saturation, Some(1.15));
        assert_eq!(glass.lift, Some(0.60));
        assert_eq!(glass.refraction_depth, 0.0);
        assert_eq!(glass.transmission_refraction, 0.0);
        assert_eq!(glass.adaptive_frost, 0.28);
    }

    #[test]
    fn bar_surface_lift_tracks_the_local_foreground_polarity() {
        let light_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK);
        assert_eq!(light_surface.lift, Some(0.60));

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
    fn contact_rises_continuously_and_returns_on_the_measured_settle() {
        let cranpose_animation::AnimationType::Spring(rise) = tab_lens_activity_motion(true) else {
            panic!("contact rise must use a spring");
        };
        assert_eq!(rise.stiffness, 1400.0);
        let cranpose_animation::AnimationType::Spring(settle) = tab_lens_activity_motion(false)
        else {
            panic!("arrival contraction must use a spring");
        };
        assert_eq!(settle.damping_ratio, 1.0);
        assert_eq!(settle.stiffness, 900.0);
    }

    #[test]
    fn a_scope_declares_destinations_in_order() {
        let tabs = collect_tabs(|scope| {
            scope.tab("M0 0", "Discover");
            scope.app_badge("M1 1", "WWDC");
            scope.push(LiquidTab::new("M2 2", "Account").with_icon_scale(0.95));
        });

        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[0].label, "Discover");
        assert_eq!(tabs[0].icon_style, LiquidTabIconStyle::Plain);
        assert_eq!(tabs[0].icon_scale, 1.0);
        assert_eq!(tabs[1].icon_style, LiquidTabIconStyle::AppBadge);
        assert_eq!(tabs[2].icon_scale, 0.95);
    }

    #[test]
    fn a_bar_with_no_destinations_declares_none() {
        assert!(collect_tabs(|_| {}).is_empty());
    }

    #[test]
    fn a_resting_lens_overhangs_its_cell_within_the_legal_margin() {
        for tab_width in [24.0_f32, 44.0, 78.0, 80.0, 120.0, 400.0] {
            let width = tab_lens_rest_width(tab_width);
            assert!(
                width > tab_width,
                "the bubble reads wider than its cell ({width} vs {tab_width})"
            );
            let overhang = (width - tab_width) * 0.5;
            assert!(
                overhang <= BLOB_MARGIN + 1.0e-4,
                "an end cell's overhang ({overhang}) at a {tab_width}dp cell \
                 has to stay inside the pill inset ({BLOB_MARGIN})"
            );
        }
    }

    #[test]
    fn a_reference_width_cell_keeps_the_measured_rest_proportion() {
        for tab_width in [24.0_f32, 44.0, 78.0, 80.0] {
            assert!(
                (tab_lens_rest_width(tab_width) - tab_width * TAB_LENS_REST_WIDTH_FACTOR).abs()
                    < 1.0e-4,
                "a {tab_width}dp cell still rests at the measured 1.10 factor"
            );
        }
        assert!(
            tab_lens_rest_width(200.0) < 200.0 * TAB_LENS_REST_WIDTH_FACTOR,
            "past the inset the margin governs, not the factor"
        );
    }

    #[test]
    fn a_resting_lens_stays_centered_on_the_cell_it_rests_in() {
        let tab_width = 72.0;
        let width = tab_lens_rest_width(tab_width);
        for selected in 0..4 {
            let left = tab_lens_resting_left(selected, tab_width, 4);
            let cell_center = left + tab_width * 0.5;
            let lens_center = left - (width - tab_width) * 0.5 + width * 0.5;
            assert!(
                (lens_center - cell_center).abs() < 1e-4,
                "cell {selected}: lens center {lens_center} vs cell center {cell_center}"
            );
        }
    }
}
