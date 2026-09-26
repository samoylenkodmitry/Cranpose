use std::{cell::RefCell, rc::Rc};

use cranpose_macros::composable;
use cranpose_ui::{
    Brush, Color, CornerRadii, Modifier, PointerInputScope, Rect, SemanticsWidgetRole, Size,
    text::{FontWeight, SpanStyle, TextStyle},
    widgets::{
        Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, ContentScale, Image, Painter,
        Row, RowSpec, Text,
    },
};
use cranpose_ui_layout::{Alignment, VerticalAlignment};

use crate::{
    material::{
        Glass, GlassDynamics, GlassFaceResponse, GlassKeyFill, GlassMorph, GlassShadow,
        LiquidModifierExt, neutral_surface_tint,
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

/// Artwork displayed in a tab's shared icon frame.
#[derive(Clone, Debug, PartialEq)]
pub enum LiquidTabIcon {
    /// Vector path data in a 24×24 view box.
    Vector(&'static str),
    /// Template artwork drawn at its supplied logical size and tinted with the tab's color.
    Painter {
        /// Image or custom painter supplying the template's alpha mask.
        painter: Painter,
        /// Artwork dimensions in logical points before the tab's optical correction.
        size: Size,
    },
}

/// One tab's artwork, label, and icon treatment.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidTab {
    /// Vector or template artwork shown above the label.
    pub icon: LiquidTabIcon,
    pub label: &'static str,
    pub icon_style: LiquidTabIconStyle,
    /// Optical correction for symbols whose path bounds do not fill the
    /// shared icon frame uniformly.
    pub icon_scale: f32,
    /// Optical displacement of the artwork inside the shared icon frame, in points.
    pub icon_offset: (f32, f32),
}

impl LiquidTab {
    pub fn new(icon: &'static str, label: &'static str) -> Self {
        Self {
            icon: LiquidTabIcon::Vector(icon),
            label,
            icon_style: LiquidTabIconStyle::Plain,
            icon_scale: 1.0,
            icon_offset: (0.0, 0.0),
        }
    }

    /// A tab using template artwork and its logical size.
    pub fn from_painter(painter: Painter, size: Size, label: &'static str) -> Self {
        Self {
            icon: LiquidTabIcon::Painter { painter, size },
            label,
            icon_style: LiquidTabIconStyle::Plain,
            icon_scale: 1.0,
            icon_offset: (0.0, 0.0),
        }
    }

    pub fn app_badge(icon: &'static str, label: &'static str) -> Self {
        Self {
            icon: LiquidTabIcon::Vector(icon),
            label,
            icon_style: LiquidTabIconStyle::AppBadge,
            icon_scale: 1.0,
            icon_offset: (0.0, 0.0),
        }
    }

    pub fn with_icon_scale(mut self, scale: f32) -> Self {
        self.icon_scale = normalize_icon_scale(scale);
        self
    }

    /// Moves the artwork without changing its cell or hit bounds; nonfinite offsets become zero.
    pub fn with_icon_offset(mut self, x: f32, y: f32) -> Self {
        self.icon_offset = (
            if x.is_finite() { x } else { 0.0 },
            if y.is_finite() { y } else { 0.0 },
        );
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

const BAR_HEIGHT: f32 = 62.0;
const BLOB_HEIGHT: f32 = 54.0;
const BLOB_MARGIN: f32 = 4.0;
const FLIGHT_LENS_INFLATION: f32 = 16.0;
const TAB_LENS_OVERLAP: f32 = 7.0;
const TAB_WIDTH: f32 = 78.0;
const TAB_ICON_SIZE: f32 = 32.0;
const TAB_LABEL_SIZE: f32 = 10.0;
const TAP_SLOP: f32 = 6.0;
const ACCESSORY_GAP: f32 = 10.0;

/// Layout parameters for a [`LiquidTabBar`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidTabBarSpec {
    max_tab_width: f32,
}

impl LiquidTabBarSpec {
    /// Sets the maximum cell allocation; nonfinite values use the default and finite values are at least one point.
    pub fn new(max_tab_width: f32) -> Self {
        Self {
            max_tab_width: if max_tab_width.is_finite() {
                max_tab_width.max(1.0)
            } else {
                TAB_WIDTH
            },
        }
    }

    fn cell_width(self, available_width: f32, count: usize) -> f32 {
        let constrained = available_width - BLOB_MARGIN * 2.0;
        if constrained.is_finite() && constrained > 1.0 {
            (constrained / count as f32).min(self.max_tab_width)
        } else {
            self.max_tab_width
        }
    }
}

impl Default for LiquidTabBarSpec {
    fn default() -> Self {
        Self::new(TAB_WIDTH)
    }
}

fn tab_flight_lens_material(foreground: cranpose_ui_graphics::Color, activity: f32) -> Glass {
    let activity = activity.clamp(0.0, 1.0);
    Glass::lens()
        .face_lighting(false)
        .key_fill(GlassKeyFill {
            height_dp: 1.0,
            curvature: 0.7 + 0.1 * activity,
            angle_radians: std::f32::consts::FRAC_PI_4,
            saturation: 1.5,
            luma_gain: 0.4367,
            offset: 1.125,
            scale_with_surface: true,
        })
        .no_clip()
        .face_response(GlassFaceResponse {
            gain: 1.0 - 0.03 * activity,
            start_dp: 1.0 / 3.0,
            end_dp: 2.0 / 3.0,
            illumination: 0.0,
        })
        .tint(neutral_surface_tint(
            foreground,
            0.07333333 * (1.0 - activity),
            0.1440678 * (1.0 - activity),
        ))
        .blur_radius(0.0)
        .backdrop_blur(4.0, 1.0 - activity)
        .edge_refraction(9.0 * activity)
        .refraction_depth_dp(36.0)
        .refraction_curve(0.25)
        .optical_zoom(1.0)
        .meniscus_absorption(0.0)
        .inner_shadow(GlassShadow::new(
            Color::BLACK.with_alpha(0.12 * activity),
            3.0,
            7.0,
            0.0,
        ))
        .fold_depth(0.0)
        .edge_spectrum(crate::material::GlassSpectrum {
            angle_radians: std::f32::consts::FRAC_PI_2
                - std::f32::consts::PI * 7.0 / 12.0 * activity,
            step_dp: (4.068 / (112.24138 / 36.0)) / (std::f32::consts::PI / 12.0).cos()
                * (-5.0 + 8.684211 * activity)
                / 3.6842105,
            vertical_scale: 0.8265,
            opacity_near: activity,
            opacity_far: 1.0 - activity,
            fade_depth_dp: 14.0 * activity,
            extent_dp: 38.88889 * activity,
        })
        .dispersion(0.0)
        .lift(0.0)
        .highlight(activity)
        .rim_reflection(activity)
        .shadow_style(GlassShadow::new(
            cranpose_ui_graphics::Color::BLACK.with_alpha(0.1 * activity),
            8.0,
            7.0,
            0.0,
        ))
}

fn tab_bar_surface_material(foreground: cranpose_ui_graphics::Color) -> Glass {
    Glass::regular()
        .face_lighting(false)
        .key_fill(GlassKeyFill {
            height_dp: 1.0,
            curvature: 0.7,
            angle_radians: std::f32::consts::FRAC_PI_4,
            saturation: 1.5,
            luma_gain: 0.1,
            offset: 0.9,
            scale_with_surface: false,
        })
        .face_response(GlassFaceResponse {
            gain: 0.97,
            start_dp: 1.0 / 3.0,
            end_dp: 2.0 / 3.0,
            illumination: 0.0,
        })
        .tint(Color::TRANSPARENT)
        .blur_radius(6.0)
        .surface_refraction(31.0)
        .refraction_depth_dp(15.5)
        .transmission_refraction(1.0)
        .saturation(1.0)
        .lift(0.0)
        .contrast(1.0)
        .highlight(1.0)
        .adaptive_frost(foreground, 0.0)
        .adaptive_tone(foreground)
        .shadow_style(GlassShadow::new(
            Color::BLACK.with_alpha(0.162),
            32.0,
            8.0,
            1.5,
        ))
}

fn tab_flight_tint_multiplier(activity: f32) -> f32 {
    1.0 - 0.25 * activity.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct TabGeometry {
    width: f32,
    cell_width: f32,
    pitch: f32,
}

impl TabGeometry {
    fn new(allocation: f32, count: usize) -> Self {
        let width = allocation * count.max(1) as f32;
        let cell_width = tab_lens_rest_width(allocation).min(width);
        let pitch = if count > 1 {
            (width - cell_width) / (count - 1) as f32
        } else {
            cell_width
        };
        Self {
            width,
            cell_width,
            pitch,
        }
    }

    fn drag_left(self, pointer_x: f32, count: usize, has_accessory: bool) -> f32 {
        let last_tab = self.pitch * count.saturating_sub(1) as f32;
        let min = if has_accessory {
            -self.pitch * 0.2
        } else {
            0.0
        };
        let max = if has_accessory {
            last_tab + self.pitch * 0.55
        } else {
            last_tab
        };
        (pointer_x - self.cell_width * 0.5).clamp(min, max)
    }

    fn optical_pointer_x(self, pointer_x: f32, press: f32, travel: f32) -> f32 {
        let transform = tab_bar_transform(self.width + 2.0 * BLOB_MARGIN, press, travel);
        let center = self.width * 0.5;
        pointer_x + (pointer_x - center) * (transform.scale_x - 1.0) + transform.translation_x
    }

    fn lens_surface_position(self, position: f32, press: f32, travel: f32) -> f32 {
        let transform = tab_bar_transform(self.width + 2.0 * BLOB_MARGIN, press, travel);
        (position + self.cell_width * 0.5 - self.width * 0.5) * transform.scale_x
    }
}

/// The settled cell origin for a selected index and the distance between cell centers.
pub fn tab_lens_resting_left(selected: usize, tab_pitch: f32, count: usize) -> f32 {
    tab_pitch * selected.min(count.saturating_sub(1)) as f32
}

/// The resting selection width for one destination's allocation in the content strip.
/// Cells overlap; their pitch is the remaining strip width divided by the number of gaps.
pub fn tab_lens_rest_width(tab_allocation: f32) -> f32 {
    tab_allocation + TAB_LENS_OVERLAP
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
fn TabIcon(icon: LiquidTabIcon, style: LiquidTabIconStyle, color: Color, optical_scale: f32) {
    const FRAME_HEIGHT: f32 = 32.0;
    Box(
        Modifier::empty().size(Size::new(TAB_ICON_SIZE, FRAME_HEIGHT)),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || match style {
            LiquidTabIconStyle::Plain => {
                TabGlyph(icon.clone(), TAB_ICON_SIZE * optical_scale, color);
            }
            LiquidTabIconStyle::AppBadge => {
                let icon = icon.clone();
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
                        let icon = icon.clone();
                        Box(
                            Modifier::empty()
                                .offset(geometry.glyph.x, geometry.glyph.y)
                                .size(Size::new(geometry.glyph.width, geometry.glyph.height)),
                            BoxSpec::default(),
                            move || TabGlyph(icon.clone(), geometry.glyph.width, Color::WHITE),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn TabGlyph(icon: LiquidTabIcon, size: f32, color: Color) {
    match icon {
        LiquidTabIcon::Vector(path) => crate::icons::Icon(path, None, size, color),
        LiquidTabIcon::Painter {
            painter,
            size: intrinsic,
        } => {
            let scale = size / TAB_ICON_SIZE;
            Image(
                painter,
                None,
                Modifier::empty()
                    .size(Size::new(intrinsic.width * scale, intrinsic.height * scale)),
                Alignment::CENTER,
                ContentScale::FillBounds,
                1.0,
                Some(cranpose_ui_graphics::ColorFilter::tint(color)),
            );
        }
    }
}

fn tab_ink_selection(
    geometry: TabGeometry,
    position: f32,
    activity: f32,
    strain: Size,
    color: Color,
) -> super::vibrancy::InkSelection {
    let (width, height) = tab_lens_base_size(geometry.cell_width, activity);
    let base = Size::new(width, height);
    let deformed = tab_lens_deformed_size(base, strain);
    let width = deformed.width;
    let height = deformed.height;
    super::vibrancy::InkSelection {
        bounds: Rect {
            x: position + geometry.cell_width * 0.5 - width * 0.5,
            y: BLOB_HEIGHT * 0.5 - height * 0.5,
            width,
            height,
        },
        color,
        content_zoom: 1.0 + 0.16 * activity.clamp(0.0, 1.0),
        activity: activity.clamp(0.0, 1.0),
        optical_scale: 1.0,
        projection: Size::new(deformed.width / base.width, deformed.height / base.height),
    }
}

#[derive(Clone, Copy, PartialEq)]
struct TabCellsSpec {
    geometry: TabGeometry,
    base_color: Color,
    selected: usize,
    committed_selection: usize,
}

/// Where the lens's ink falls on the cells this frame, under the bar's lift.
fn lifted_ink(
    geometry: TabGeometry,
    count: usize,
    selection: super::vibrancy::InkSelection,
    transform: &cranpose_ui_graphics::GraphicsLayer,
) -> (super::vibrancy::InkSelection, super::vibrancy::InkGrid) {
    let size = Size::new(geometry.width + BLOB_MARGIN * 2.0, BAR_HEIGHT);
    let mut selection = selection;
    let center = selection.bounds.x + selection.bounds.width * 0.5 + BLOB_MARGIN;
    selection.bounds.width *= transform.scale_x;
    selection.bounds.height *= transform.scale_y;
    selection.optical_scale = transform.scale_x.min(transform.scale_y);
    selection.bounds.x = (center - size.width * 0.5) * transform.scale_x
        + size.width * 0.5
        + transform.translation_x
        - selection.bounds.width * 0.5;
    selection.bounds.y = (BAR_HEIGHT - selection.bounds.height) * 0.5;
    let grid = super::vibrancy::InkGrid {
        first_center: (
            (BLOB_MARGIN + geometry.cell_width * 0.5 - size.width * 0.5) * transform.scale_x
                + size.width * 0.5
                + transform.translation_x,
            BAR_HEIGHT * 0.5,
        ),
        pitch: geometry.pitch * transform.scale_x,
        count,
    };
    (selection, grid)
}

#[composable]
fn TabCells(
    modifier: Modifier,
    tabs: Rc<Vec<LiquidTab>>,
    typography: LiquidTypography,
    spec: TabCellsSpec,
    selection: impl Fn() -> super::vibrancy::InkSelection + 'static,
    transform: impl Fn() -> cranpose_ui_graphics::GraphicsLayer + 'static,
    on_select: impl Fn(usize) + 'static,
) {
    let on_select: Rc<dyn Fn(usize)> = Rc::new(on_select);
    let transform: Rc<dyn Fn() -> cranpose_ui_graphics::GraphicsLayer> = Rc::new(transform);
    let geometry = spec.geometry;
    let size = Size::new(geometry.width + BLOB_MARGIN * 2.0, BAR_HEIGHT);
    let count = tabs.len();
    let ink = {
        let transform = Rc::clone(&transform);
        move || lifted_ink(geometry, count, selection(), &transform())
    };
    super::vibrancy::VibrantContent(modifier, size, spec.base_color, ink, move || {
        let tabs = Rc::clone(&tabs);
        let typography = typography.clone();
        let on_select = Rc::clone(&on_select);
        let transform = Rc::clone(&transform);
        Box(
            Modifier::empty()
                .size(size)
                .selectable_group()
                .role(SemanticsWidgetRole::TabBar)
                .graphics_layer(move || transform()),
            BoxSpec::default(),
            move || {
                for (index, tab) in tabs.iter().enumerate() {
                    let color = spec.base_color;
                    let label_for_semantics = tab.label;
                    let icon_offset = tab.icon_offset;
                    let on_select = Rc::clone(&on_select);
                    let cell = Modifier::empty()
                        .offset(BLOB_MARGIN + index as f32 * geometry.pitch, BLOB_MARGIN)
                        .size(Size::new(geometry.cell_width, BLOB_HEIGHT))
                        .semantics(super::selection::selection_semantics(
                            label_for_semantics.to_string(),
                            SemanticsWidgetRole::Tab,
                            index,
                            spec.committed_selection,
                            on_select,
                        ))
                        .focusable();
                    let icon = tab.icon.clone();
                    let icon_style = tab.icon_style;
                    let icon_scale = tab.icon_scale;
                    let label = tab.label;
                    let label_style = TextStyle {
                        span_style: SpanStyle {
                            color: Some(color),
                            font_size: cranpose_ui::text::TextUnit::Sp(TAB_LABEL_SIZE),
                            font_weight: Some(if spec.selected == index {
                                FontWeight::SEMI_BOLD
                            } else {
                                FontWeight::MEDIUM
                            }),
                            ..typography.caption1.span_style.clone()
                        },
                        paragraph_style: cranpose_ui::text::ParagraphStyle {
                            line_height: cranpose_ui::text::TextUnit::Sp(12.0),
                            ..typography.caption1.paragraph_style.clone()
                        },
                    };
                    Box(cell, BoxSpec::default(), move || {
                        let label_style = label_style.clone();
                        let icon = icon.clone();
                        Box(
                            Modifier::empty()
                                .offset(
                                    (geometry.cell_width - TAB_ICON_SIZE) * 0.5 + icon_offset.0,
                                    3.0 + icon_offset.1,
                                )
                                .size(Size::new(TAB_ICON_SIZE, TAB_ICON_SIZE)),
                            BoxSpec::default(),
                            move || TabIcon(icon.clone(), icon_style, color, icon_scale),
                        );
                        Box(
                            Modifier::empty()
                                .offset(0.0, 35.0)
                                .size(Size::new(geometry.cell_width, 12.0)),
                            BoxSpec::default().content_alignment(Alignment::CENTER),
                            move || {
                                Text(label, Modifier::empty(), label_style.clone());
                            },
                        );
                    });
                }
            },
        );
    });
}

fn tab_lens_node_top(node_height: f32) -> f32 {
    (BAR_HEIGHT - node_height) * 0.5
}

#[derive(Clone, Copy)]
struct TabBarTouch {
    position: cranpose_core::MutableState<(f32, f32)>,
    origin: cranpose_core::MutableState<Option<f32>>,
    travel: cranpose_core::MutableState<f32>,
}

impl TabBarTouch {
    fn pressed(self, down: bool) {
        if down {
            self.origin.set(None);
            self.travel.set(0.0);
        }
    }

    fn update(self, x: f32, y: f32, span: f32) {
        self.position.set((x + BLOB_MARGIN, y + BLOB_MARGIN));
        if let Some(origin) = self.origin.get() {
            self.travel
                .set(((x - origin) / span.max(1.0)).clamp(-1.0, 1.0));
        } else {
            self.origin.set(Some(x));
        }
    }
}

#[composable]
fn remember_tab_bar_touch() -> TabBarTouch {
    TabBarTouch {
        position: cranpose_core::remember(|| cranpose_core::mutableStateOf((0.0f32, 0.0f32)))
            .with(|state| *state),
        origin: cranpose_core::remember(|| cranpose_core::mutableStateOf(None::<f32>))
            .with(|state| *state),
        travel: cranpose_core::remember(|| cranpose_core::mutableStateOf(0.0f32))
            .with(|state| *state),
    }
}

fn tab_bar_transform(width: f32, press: f32, travel: f32) -> cranpose_ui_graphics::GraphicsLayer {
    let press = press.max(0.0);
    let travel = travel.clamp(-1.0, 1.0);
    let rise = 14.14 / width.max(1.0) * press;
    let strain = 2.63 / width.max(1.0) * travel.abs() * press;
    cranpose_ui_graphics::GraphicsLayer {
        scale_x: 1.0 + rise + strain,
        scale_y: 1.0 + rise - strain,
        translation_x: 2.756 * travel * press,
        ..Default::default()
    }
}

fn tab_bar_accessory_gap(has_accessory: bool) -> f32 {
    if has_accessory { ACCESSORY_GAP } else { 0.0 }
}

fn accessory_surfaces_touch(edge_gap: f32) -> bool {
    edge_gap <= 0.0
}

fn tab_lens_base_size(cell_width: f32, activity: f32) -> (f32, f32) {
    let inflation = FLIGHT_LENS_INFLATION * activity.clamp(0.0, 1.0);
    (cell_width + inflation, BLOB_HEIGHT + inflation)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabFlightGeometry {
    center: (f32, f32),
    base_size: Size,
    strain: Size,
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

fn tab_lens_deformed_size(base: Size, strain: Size) -> Size {
    Size::new(
        base.width * (1.0 + strain.width.clamp(-0.35, 0.35)),
        base.height * (1.0 + strain.height.clamp(-0.35, 0.35)),
    )
}

fn tab_flight_dynamics(geometry: TabFlightGeometry, node: TabFlightNode) -> GlassDynamics {
    let activity = geometry.lens_activity.clamp(0.0, 1.0);
    let size = geometry.base_size;
    let deformed = tab_lens_deformed_size(size, geometry.strain);
    let radius = size.width.min(size.height) * 0.5;
    let glue = 20.0;
    let shapes = geometry
        .accessory_center
        .filter(|(x, _)| {
            let edge_gap = (*x - geometry.center.0).abs() - size.width * 0.5 - BAR_HEIGHT * 0.5;
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
        edge_return_depth_dp: Some(7.0 * activity),
        optical_projection: Some((deformed.width / size.width, deformed.height / size.height)),
        morph: Some(GlassMorph {
            node_size: (node.size.width, node.size.height),
            primary: (
                geometry.center.0 - node.origin.0,
                geometry.center.1 - node.origin.1,
                size.width,
                size.height,
                radius,
            ),
            shapes,
            glue,
            wobble_amplitude: 0.0,
            wobble_phase: geometry.lens_position * 0.045,
            bulge_amplitude: 0.0,
            bulge_direction: 0.0,
            ellipse_blend: 0.0,
            capsule_smoothing_dp: ((size.width - size.height).abs() * 0.5).min(radius * 0.6)
                * activity,
            deformation: None,
            zoom_anchor: (0.0, 0.0),
        }),
        activity: Some(1.0),
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
    let tint_amount = crate::theme::liquid_glass_tint_amount();
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

            let tab_width = cranpose_core::remember(|| cranpose_core::mutableStateOf(0.0f32))
                .with(|state| *state);
            let lens_pressed = cranpose_core::remember(|| cranpose_core::mutableStateOf(false))
                .with(|state| *state);
            let bar_touch = remember_tab_bar_touch();
            let contact_motion = super::tab_motion::remember_tab_contact_motion();
            let bar_press = contact_motion.bar_state();
            let glow = contact_motion.glow_state();
            let local_glow_factor = contact_motion.local_glow_factor_state();
            let travel = cranpose_animation::animateFloatAsState(
                bar_touch.travel.get(),
                cranpose_animation::spring(0.85, 650.0),
                "tabbar-travel",
            );
            let motion = TabBarMotion {
                axis: cranpose_core::remember(|| Rc::new(RefCell::new(None))).with(Rc::clone),
                lens_shape: super::tab_motion::remember_tab_lens_shape(),
                bar_press,
                travel,
                lens_activity: contact_motion.lens_state(),
            };
            let cells = TabGeometry::new(tab_width.get(), count);
            let resting_lens_x = tab_lens_resting_left(selected, cells.pitch, count);
            let visual_index = {
                let motion = motion.clone();
                cranpose_core::derivedStateOf(move || {
                    let lens_x = motion.lens_x(resting_lens_x);
                    crate::motion::liquid_visual_index(
                        selected,
                        lens_x,
                        cells.pitch,
                        count,
                        crate::motion::liquid_axis_owns_visual_selection(
                            lens_pressed.get(),
                            lens_x,
                            resting_lens_x,
                            cells.pitch,
                        ),
                    )
                })
                .get()
            };
            if bar_touch.travel.get_non_reactive().abs() > 0.001 && visual_index != selected {
                contact_motion.crossed_tab();
            }
            let pill_w = cells.width + 2.0 * BLOB_MARGIN;
            let bar_lift = Modifier::empty()
                .graphics_layer(move || tab_bar_transform(pill_w, bar_press.get(), travel.get()));
            Box(
                Modifier::empty().height(BAR_HEIGHT),
                BoxSpec::default(),
                move || {
                    let tabs = Rc::clone(&tabs);
                    let typography = typography.clone();
                    let on_select = Rc::clone(&on_select);
                    let semantic_selection = Rc::clone(&on_select);
                    let contact_motion = Rc::clone(&contact_motion);
                    let published_axis = Rc::clone(&motion.axis);
                    let pill = bar_lift.clone().height(BAR_HEIGHT);
                    Box(pill, BoxSpec::default(), move || {
                        let on_select = Rc::clone(&on_select);
                        let contact_motion = Rc::clone(&contact_motion);
                        let published_axis = Rc::clone(&published_axis);
                        BoxWithConstraints(Modifier::empty(), move |scope| {
                            let on_select = Rc::clone(&on_select);
                            let measured_width =
                                spec.cell_width(scope.constraints().max_width, count);
                            if tab_width.get_non_reactive() != measured_width {
                                tab_width.set(measured_width);
                            }

                            let geometry = TabGeometry::new(measured_width, count);
                            Box(
                                Modifier::empty()
                                    .glass_effect(
                                        tab_bar_surface_material(colors.label)
                                            .tint_amount(tint_amount),
                                    )
                                    .size(Size::new(
                                        geometry.width + BLOB_MARGIN * 2.0,
                                        BAR_HEIGHT,
                                    )),
                                BoxSpec::default(),
                                || {},
                            );
                            let resting_lens_x =
                                tab_lens_resting_left(selected, geometry.pitch, count);
                            let lens_axis = crate::motion::remember_liquid_follow_axis(
                                resting_lens_x,
                                cranpose_animation::spring(0.85, 650.0),
                            );
                            if !lens_pressed.get() {
                                lens_axis.settle_to(resting_lens_x, LiquidMotion::glide());
                            }
                            let published = published_axis
                                .borrow()
                                .as_ref()
                                .is_some_and(|held| Rc::ptr_eq(held, &lens_axis));
                            if !published {
                                *published_axis.borrow_mut() = Some(Rc::clone(&lens_axis));
                            }
                            let row_width = geometry.width;
                            let gesture = Modifier::empty()
                                .offset(BLOB_MARGIN, BLOB_MARGIN)
                                .size(Size::new(row_width, BLOB_HEIGHT))
                                .pointer_input(selected, {
                                    let on_select = Rc::clone(&on_select);
                                    let lens_axis = Rc::clone(&lens_axis);
                                    let contact_motion = Rc::clone(&contact_motion);
                                    move |scope: PointerInputScope| {
                                        let on_select = Rc::clone(&on_select);
                                        let lens_axis = Rc::clone(&lens_axis);
                                        let contact_motion = Rc::clone(&contact_motion);
                                        crate::motion::liquid_lens_gesture(
                                            scope,
                                            crate::motion::LiquidLensGesture {
                                                axis: lens_axis,
                                                cell_width: geometry.pitch,
                                                cell_offset: (geometry.cell_width - geometry.pitch)
                                                    * 0.5,
                                                count,
                                                tap_slop: TAP_SLOP,
                                                drag_left: Rc::new(move |x| {
                                                    geometry.drag_left(
                                                        geometry.optical_pointer_x(
                                                            x,
                                                            bar_press.get(),
                                                            travel.get(),
                                                        ),
                                                        count,
                                                        has_accessory,
                                                    )
                                                }),
                                                rest_left: Rc::new(move |index| {
                                                    tab_lens_resting_left(
                                                        index,
                                                        geometry.pitch,
                                                        count,
                                                    )
                                                }),
                                                selected,
                                                on_pressed: Rc::new(move |down, time| {
                                                    lens_pressed.set(down);
                                                    contact_motion.pressed(down, time);
                                                    bar_touch.pressed(down);
                                                }),
                                                on_touch: Rc::new(move |x, y| {
                                                    bar_touch.update(
                                                        geometry.optical_pointer_x(
                                                            x,
                                                            bar_press.get(),
                                                            travel.get(),
                                                        ),
                                                        y,
                                                        geometry.pitch
                                                            * count.saturating_sub(1) as f32,
                                                    );
                                                }),
                                                on_select,
                                            },
                                        )
                                    }
                                });
                            Box(gesture, BoxSpec::default(), || {});
                        });
                    });

                    let (lens_w, lens_h) = tab_lens_base_size(cells.cell_width, 1.0);
                    let deformation_headroom =
                        crate::dynamics::STRETCH_MAX.max(1.0 / crate::dynamics::STRETCH_MIN);
                    let node_w = lens_w * deformation_headroom + crate::dynamics::BULGE_MAX + 20.0;
                    let node_h = lens_h * deformation_headroom + crate::dynamics::BULGE_MAX + 16.0;
                    let node_top = tab_lens_node_top(node_h);
                    let node_size = Size::new(node_w, node_h);
                    let lens = TabLensPlacement {
                        cells,
                        resting_lens_x,
                        pill_w,
                        node_size,
                        node_top,
                        resting_tint: colors.fill,
                        accessory_center: has_accessory.then_some((
                            pill_w + tab_bar_accessory_gap(true) + BAR_HEIGHT * 0.5,
                            BAR_HEIGHT * 0.5,
                        )),
                    };
                    let transform: Rc<dyn Fn() -> cranpose_ui_graphics::GraphicsLayer> = {
                        let motion = motion.clone();
                        Rc::new(move || {
                            tab_bar_transform(pill_w, motion.bar_press.get(), motion.travel.get())
                        })
                    };
                    let lens_node_modifier = Modifier::empty()
                        .required_size(node_size)
                        .offset(0.0, node_top)
                        .graphics_layer({
                            let motion = motion.clone();
                            move || lens.layer(&motion)
                        });
                    let material_layers: Rc<RefCell<Option<HeldTabLensLayers>>> =
                        cranpose_core::remember(|| Rc::new(RefCell::new(None))).with(Rc::clone);
                    let lens_layers = {
                        let motion = motion.clone();
                        move |layer: usize| {
                            let frame = motion.frame(lens.cells, lens.resting_lens_x);
                            let key = frame.activity.to_bits();
                            let layers = {
                                let mut held = material_layers.borrow_mut();
                                match &*held {
                                    Some((held_key, layers)) if *held_key == key => {
                                        Rc::clone(layers)
                                    }
                                    _ => {
                                        let layers: Rc<TabLensLayers> =
                                            Rc::new(crate::material::cached_glass_content_layers(
                                                tab_flight_lens_material(
                                                    colors.label,
                                                    frame.activity,
                                                )
                                                .resolve(&colors),
                                            ));
                                        *held = Some((key, Rc::clone(&layers)));
                                        layers
                                    }
                                }
                            };
                            layers(
                                cranpose_ui::current_density(),
                                tab_flight_dynamics(lens.geometry(frame), lens.node(frame)),
                            )[layer]
                                .clone()
                        }
                    };
                    let background_layers = lens_layers.clone();
                    let lens_background = lens_node_modifier
                        .clone()
                        .graphics_layer(move || background_layers(0));
                    let lighting_transform = Rc::clone(&transform);
                    super::tab_lighting::TabLighting(
                        Size::new(pill_w, BAR_HEIGHT),
                        move || lighting_transform(),
                        bar_touch.position,
                        glow,
                        local_glow_factor,
                    );
                    Box(lens_background, BoxSpec::default(), || {});
                    let selection_color = tab_selection_content_color(colors);
                    let selection: Rc<dyn Fn() -> super::vibrancy::InkSelection> = {
                        let motion = motion.clone();
                        Rc::new(move || {
                            let frame = motion.frame(cells, resting_lens_x);
                            tab_ink_selection(
                                cells,
                                frame.lens_x,
                                frame.activity,
                                frame.strain,
                                selection_color,
                            )
                        })
                    };
                    TabCells(
                        Modifier::empty(),
                        Rc::clone(&tabs),
                        typography,
                        TabCellsSpec {
                            geometry: cells,
                            base_color: tab_base_content_color(colors),
                            selected: visual_index,
                            committed_selection: selected,
                        },
                        move || selection(),
                        move || transform(),
                        move |index| semantic_selection(index),
                    );
                    Box(
                        lens_node_modifier.graphics_layer(move || lens_layers(1)),
                        BoxSpec::default(),
                        || {},
                    );
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

/// The flying lens's two glass layers for one material, kept while the lens's
/// activity, which picks the material, holds.
type TabLensLayers = dyn Fn(f32, GlassDynamics) -> [cranpose_ui_graphics::GraphicsLayer; 2];

/// The lens layers held for the activity, by its bits, that picked them.
type HeldTabLensLayers = (u32, Rc<TabLensLayers>);

/// The tab bar's moving parts, read where they are drawn: a glide or a press
/// then redraws the bar each frame and recomposes it only when the tab under
/// the lens changes.
#[derive(Clone)]
struct TabBarMotion {
    axis: Rc<RefCell<Option<Rc<crate::motion::LiquidDragAxis>>>>,
    lens_shape: Rc<super::tab_motion::TabLensShape>,
    bar_press: cranpose_core::State<f32>,
    travel: cranpose_core::State<f32>,
    lens_activity: cranpose_core::State<f32>,
}

/// What the lens looks like this frame.
#[derive(Clone, Copy)]
struct TabLensFrame {
    lens_x: f32,
    activity: f32,
    strain: Size,
    press: f32,
    travel: f32,
}

impl TabBarMotion {
    /// The lens's left edge, or where it rests before the bar has laid out.
    fn lens_x(&self, resting_lens_x: f32) -> f32 {
        self.axis
            .borrow()
            .as_ref()
            .map_or(resting_lens_x, |axis| axis.value())
    }

    /// Reads every moving part, so the caller redraws when any of them moves.
    /// The lens shape takes the lens's position once a frame, whichever
    /// reader draws first, and every reader sees the strain it holds.
    fn frame(&self, cells: TabGeometry, resting_lens_x: f32) -> TabLensFrame {
        let press = self.bar_press.get();
        let travel = self.travel.get();
        let activity = self.lens_activity.get().clamp(0.0, 1.0);
        let lens_x = self.lens_x(resting_lens_x);
        let strain = self
            .lens_shape
            .sample(cells.lens_surface_position(lens_x, press, travel));
        TabLensFrame {
            lens_x,
            activity,
            strain,
            press,
            travel,
        }
    }
}

/// Where the flying lens sits in the bar: everything about it that does not
/// move with the lens.
#[derive(Clone, Copy)]
struct TabLensPlacement {
    cells: TabGeometry,
    resting_lens_x: f32,
    pill_w: f32,
    node_size: Size,
    node_top: f32,
    resting_tint: Color,
    accessory_center: Option<(f32, f32)>,
}

impl TabLensPlacement {
    fn center_x(self, frame: TabLensFrame) -> f32 {
        BLOB_MARGIN + frame.lens_x + self.cells.cell_width * 0.5
    }

    fn node(self, frame: TabLensFrame) -> TabFlightNode {
        TabFlightNode {
            origin: (
                self.center_x(frame) - self.node_size.width * 0.5,
                self.node_top,
            ),
            size: self.node_size,
        }
    }

    fn geometry(self, frame: TabLensFrame) -> TabFlightGeometry {
        let (base_w, base_h) = tab_lens_base_size(self.cells.cell_width, frame.activity);
        TabFlightGeometry {
            center: (self.center_x(frame), BAR_HEIGHT * 0.5),
            base_size: Size::new(base_w, base_h),
            strain: frame.strain,
            lens_position: frame.lens_x,
            lens_activity: frame.activity,
            resting_tint: self.resting_tint,
            accessory_center: self.accessory_center,
        }
    }

    /// The lens node's layer: the bar's lift about the pill's center, and the
    /// node carried to the lens in the same translation.
    fn layer(self, motion: &TabBarMotion) -> cranpose_ui_graphics::GraphicsLayer {
        let frame = motion.frame(self.cells, self.resting_lens_x);
        let mut layer = tab_bar_transform(self.pill_w, frame.press, frame.travel);
        let center_x = self.center_x(frame);
        layer.translation_x +=
            (center_x - self.pill_w * 0.5) * (layer.scale_x - 1.0) + self.node(frame).origin.0;
        layer
    }
}

/// The standard detached accessory: a circular glass search button.
#[composable]
pub fn LiquidTabBarSearchAccessory(on_click: impl Fn() + 'static) {
    crate::widgets::GlassIconButton(
        Modifier::empty().content_description("Search"),
        crate::widgets::GlassButtonSpec::glass(),
        BAR_HEIGHT * 0.94,
        on_click,
        crate::icons::SEARCH,
    );
}

#[cfg(test)]
#[path = "tests/tab_bar_tests.rs"]
mod tests;
