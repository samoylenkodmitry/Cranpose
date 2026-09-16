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
#[allow(non_snake_case)]
fn TabIcon(icon: LiquidTabIcon, style: LiquidTabIconStyle, color: Color, optical_scale: f32) {
    const FRAME_HEIGHT: f32 = 32.0;
    Box(
        Modifier::empty().size(Size::new(TAB_ICON_SIZE, FRAME_HEIGHT)),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || match style {
            LiquidTabIconStyle::Plain => {
                TabGlyph(icon.clone(), TAB_ICON_SIZE * optical_scale, color)
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
    base_color: Color,
    selection: super::vibrancy::InkSelection,
    selected: usize,
    committed_selection: usize,
}

#[composable]
#[allow(non_snake_case)]
fn TabCells(
    modifier: Modifier,
    tabs: Rc<Vec<LiquidTab>>,
    typography: LiquidTypography,
    geometry: TabGeometry,
    spec: TabCellsSpec,
    transform: cranpose_ui_graphics::GraphicsLayer,
) {
    let size = Size::new(geometry.width + BLOB_MARGIN * 2.0, BAR_HEIGHT);
    let mut selection = spec.selection;
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
        count: tabs.len(),
    };
    super::vibrancy::VibrantContent(
        modifier,
        size,
        spec.base_color,
        selection,
        grid,
        move || {
            let tabs = Rc::clone(&tabs);
            let typography = typography.clone();
            Box(
                Modifier::empty()
                    .size(size)
                    .selectable_group()
                    .graphics_layer_value(transform.clone()),
                BoxSpec::default(),
                move || {
                    for (index, tab) in tabs.iter().enumerate() {
                        let color = spec.base_color;
                        let label_for_semantics = tab.label;
                        let icon_offset = tab.icon_offset;
                        let cell = Modifier::empty()
                            .offset(BLOB_MARGIN + index as f32 * geometry.pitch, BLOB_MARGIN)
                            .size(Size::new(geometry.cell_width, BLOB_HEIGHT))
                            .semantics(move |config| {
                                config.role = Some(SemanticsWidgetRole::Tab);
                                config.is_clickable = true;
                                config.selected = Some(index == spec.committed_selection);
                                config.content_description = Some(label_for_semantics.to_string());
                            });
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
        },
    );
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

            let lens_x_outer = cranpose_core::remember(move || {
                cranpose_core::mutableStateOf((
                    0.0f32,
                    0.0f32,
                    0.0f32,
                    Size::new(0.0, 0.0),
                    selected,
                ))
            })
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
            let bar_lift = Modifier::empty().graphics_layer(move || {
                let width = lens_x_outer.get().2 * count as f32 + 2.0 * BLOB_MARGIN;
                tab_bar_transform(width, bar_press.get(), travel.get())
            });
            Box(
                Modifier::empty().height(BAR_HEIGHT),
                BoxSpec::default(),
                move || {
                    let tabs = Rc::clone(&tabs);
                    let typography = typography.clone();
                    let on_select = Rc::clone(&on_select);
                    let contact_motion = Rc::clone(&contact_motion);
                    let pill = bar_lift.clone().height(BAR_HEIGHT);
                    Box(pill, BoxSpec::default(), move || {
                        let on_select = Rc::clone(&on_select);
                        let contact_motion = Rc::clone(&contact_motion);
                        BoxWithConstraints(Modifier::empty(), move |scope| {
                            let on_select = Rc::clone(&on_select);
                            let constrained = scope.constraints().max_width - BLOB_MARGIN * 2.0;
                            let tab_width = if constrained.is_finite() && constrained > 1.0 {
                                (constrained / count as f32).min(spec.max_tab_width)
                            } else {
                                spec.max_tab_width
                            };

                            let geometry = TabGeometry::new(tab_width, count);
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
                            let lens_pressed =
                                cranpose_core::remember(|| cranpose_core::mutableStateOf(false))
                                    .with(|state| *state);
                            let resting_lens_x =
                                tab_lens_resting_left(selected, geometry.pitch, count);
                            let lens_axis = crate::motion::remember_liquid_follow_axis(
                                resting_lens_x,
                                cranpose_animation::spring(0.85, 650.0),
                            );
                            if !lens_pressed.get() {
                                lens_axis.settle_to(resting_lens_x, LiquidMotion::glide());
                            }
                            let lens_x = lens_axis.value();
                            let lens_shape = super::tab_motion::remember_tab_lens_shape();
                            let strain = lens_shape.sample(geometry.lens_surface_position(
                                lens_x,
                                bar_press.get(),
                                travel.get(),
                            ));
                            let lens_activity_anim = contact_motion.lens_state();
                            let lens_activity = lens_activity_anim.get().clamp(0.0, 1.0);
                            let visual_index = crate::motion::liquid_visual_index(
                                selected,
                                lens_x,
                                geometry.pitch,
                                count,
                                crate::motion::liquid_axis_owns_visual_selection(
                                    lens_pressed.get(),
                                    lens_x,
                                    resting_lens_x,
                                    geometry.pitch,
                                ),
                            );
                            if bar_touch.travel.get().abs() > 0.001 && visual_index != selected {
                                contact_motion.crossed_tab();
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

                            let published =
                                (lens_x, lens_activity, tab_width, strain, visual_index);
                            if lens_x_outer.get() != published {
                                lens_x_outer.set(published);
                            }
                        });
                    });

                    let (lens_px, lens_activity, lens_tab_w, strain, visual_index) =
                        lens_x_outer.get();
                    let cells = TabGeometry::new(lens_tab_w, count);
                    let (lens_w, lens_h) = tab_lens_base_size(cells.cell_width, 1.0);
                    let deformation_headroom =
                        crate::dynamics::STRETCH_MAX.max(1.0 / crate::dynamics::STRETCH_MIN);
                    let node_w = lens_w * deformation_headroom + crate::dynamics::BULGE_MAX + 20.0;
                    let node_h = lens_h * deformation_headroom + crate::dynamics::BULGE_MAX + 16.0;
                    let pill_w = cells.width + 2.0 * BLOB_MARGIN;
                    let transform = tab_bar_transform(pill_w, bar_press.get(), travel.get());
                    let local_center_x = BLOB_MARGIN + lens_px + cells.cell_width * 0.5;
                    let lens_center_x = local_center_x;
                    let node_x = lens_center_x - node_w * 0.5;
                    let node_top = tab_lens_node_top(node_h);
                    let (base_w, base_h) = tab_lens_base_size(cells.cell_width, lens_activity);
                    let geometry = TabFlightGeometry {
                        center: (lens_center_x, BAR_HEIGHT * 0.5),
                        base_size: Size::new(base_w, base_h),
                        strain,
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
                    let mut lens_transform = transform.clone();
                    lens_transform.translation_x +=
                        (local_center_x - pill_w * 0.5) * (transform.scale_x - 1.0);
                    let layers = Rc::new(crate::material::cached_glass_content_layers(
                        tab_flight_lens_material(colors.label, lens_activity).resolve(&colors),
                    ));
                    let lens_node_modifier = Modifier::empty()
                        .required_size(lens_node.size)
                        .offset(node_x, node_top)
                        .graphics_layer_value(lens_transform);
                    let background_layers = Rc::clone(&layers);
                    let lens = lens_node_modifier.clone().graphics_layer(move || {
                        background_layers(
                            cranpose_ui::current_density(),
                            tab_flight_dynamics(lens_geometry, lens_node),
                        )[0]
                        .clone()
                    });
                    super::tab_lighting::TabLighting(
                        Size::new(pill_w, BAR_HEIGHT),
                        transform.clone(),
                        bar_touch.position,
                        glow,
                        local_glow_factor,
                    );
                    Box(lens, BoxSpec::default(), || {});
                    TabCells(
                        Modifier::empty(),
                        Rc::clone(&tabs),
                        typography.clone(),
                        cells,
                        TabCellsSpec {
                            base_color: tab_base_content_color(colors),
                            selection: tab_ink_selection(
                                cells,
                                lens_px,
                                lens_activity,
                                strain,
                                tab_selection_content_color(colors),
                            ),
                            selected: visual_index,
                            committed_selection: selected,
                        },
                        transform,
                    );
                    Box(
                        lens_node_modifier.graphics_layer(move || {
                            layers(
                                cranpose_ui::current_density(),
                                tab_flight_dynamics(lens_geometry, lens_node),
                            )[1]
                            .clone()
                        }),
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
    use crate::widgets::tab_motion::tab_lens_activity_motion;

    #[test]
    fn tab_bar_spec_normalizes_the_maximum_cell_width() {
        assert_eq!(LiquidTabBarSpec::default().max_tab_width, TAB_WIDTH);
        assert_eq!(LiquidTabBarSpec::new(85.0).max_tab_width, 85.0);
        assert_eq!(LiquidTabBarSpec::new(0.0).max_tab_width, 1.0);
        assert_eq!(LiquidTabBarSpec::new(f32::NAN).max_tab_width, TAB_WIDTH);
    }

    #[test]
    fn drag_pointer_centers_the_lens_and_preserves_end_overdrag() {
        let geometry = TabGeometry {
            width: 400.0,
            cell_width: 100.0,
            pitch: 100.0,
        };
        assert_eq!(geometry.drag_left(50.0, 4, true), 0.0);
        assert_eq!(geometry.drag_left(250.0, 4, true), 200.0);
        assert_eq!(geometry.drag_left(-100.0, 4, true), -20.0);
        assert_eq!(geometry.drag_left(500.0, 4, true), 355.0);

        assert_eq!(geometry.drag_left(-100.0, 4, false), 0.0);
        assert_eq!(geometry.drag_left(500.0, 4, false), 300.0);
    }

    #[test]
    fn resting_lens_centers_on_its_cell_and_stays_inside_the_pill() {
        let tab = 78.0;
        assert_eq!(tab_lens_resting_left(0, tab, 5), 0.0);
        assert_eq!(tab_lens_resting_left(1, tab, 5), tab);
        assert_eq!(tab_lens_resting_left(3, tab, 5), 3.0 * tab);
        assert_eq!(tab_lens_resting_left(4, tab, 5), 4.0 * tab);
        assert_eq!(tab_lens_resting_left(9, tab, 5), 4.0 * tab);
    }

    #[test]
    fn flight_deformation_keeps_native_optics_in_the_unscaled_capsule() {
        let geometry = TabFlightGeometry {
            center: (80.0, 54.0),
            base_size: Size::new(111.0, 70.0),
            strain: Size::new(14.0 / 111.0, -14.0 / 70.0),
            lens_position: 0.0,
            lens_activity: 1.0,
            resting_tint: Color::TRANSPARENT,
            accessory_center: None,
        };
        let dynamics = tab_flight_dynamics(
            geometry,
            TabFlightNode {
                origin: (0.0, 0.0),
                size: Size::new(160.0, 108.0),
            },
        );
        let shape = dynamics.morph.unwrap().primary;
        assert_eq!((shape.2, shape.3, shape.4), (111.0, 70.0, 35.0));
    }

    #[test]
    fn released_bubble_keeps_its_elastic_rebound() {
        let base = Size::new(104.5, 54.0);
        let strain = Size::new(0.017416525, -0.024551005);
        let actual = tab_lens_deformed_size(base, strain);
        assert!((actual.width - 106.32003).abs() < 0.0001);
        assert!((actual.height - 52.67425).abs() < 0.0001);
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
        assert_eq!(tab.icon, LiquidTabIcon::Vector(crate::icons::STAR));
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
    fn artwork_offsets_preserve_the_tab_and_reject_nonfinite_coordinates() {
        let tab = LiquidTab::new(crate::icons::STAR, "Discover");
        let shifted = tab.clone().with_icon_offset(-1.0 / 3.0, 1.0);
        assert_eq!(shifted.icon_offset, (-1.0 / 3.0, 1.0));
        assert_eq!(shifted.icon, tab.icon);
        assert_eq!(shifted.label, tab.label);
        assert_eq!(
            tab.clone().with_icon_offset(f32::NAN, 1.0).icon_offset,
            (0.0, 1.0)
        );
        assert_eq!(
            tab.with_icon_offset(2.0, f32::INFINITY).icon_offset,
            (2.0, 0.0)
        );
    }

    #[test]
    fn template_tab_keeps_artwork_size_and_label() {
        let bitmap = cranpose_ui_graphics::ImageBitmap::from_rgba8(1, 1, vec![0, 0, 0, 255])
            .expect("template pixel");
        let painter = Painter::from_bitmap(bitmap);
        let size = Size::new(24.0, 28.0);
        let tab = LiquidTab::from_painter(painter.clone(), size, "Saved");
        assert_eq!(tab.icon, LiquidTabIcon::Painter { painter, size });
        assert_eq!(tab.label, "Saved");
        assert_eq!(tab.icon_style, LiquidTabIconStyle::Plain);
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
    fn tab_deformation_preserves_independent_native_axis_motion() {
        let base = Size::new(120.5, 70.0);
        let strain = Size::new(0.1820888, -0.2760726);
        let deformed = tab_lens_deformed_size(base, strain);
        assert!((deformed.width - 142.4417).abs() < 0.001);
        assert!((deformed.height - 50.67492).abs() < 0.001);
        assert_eq!(tab_lens_deformed_size(base, Size::new(0.0, 0.0)), base);
    }

    #[test]
    fn selection_mask_and_lens_resolve_the_same_global_sdf() {
        let geometry = TabFlightGeometry {
            center: (212.0, 32.0),
            base_size: Size::new(106.0, 64.0),
            strain: Size::new(0.0, 0.0),
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
    fn lens_contact_swell_matches_the_raised_reference() {
        for (cell, raised_width) in [(95.0, 111.0), (104.5, 120.5)] {
            assert_eq!(tab_lens_base_size(cell, 0.0), (cell, 54.0));
            let raised = tab_lens_base_size(cell, 1.0);
            assert!((raised.0 - raised_width).abs() < 0.001);
            assert!((raised.1 - 70.0).abs() < 0.001);
        }
    }

    #[test]
    fn held_bar_grows_about_its_center_and_strains_with_travel() {
        let rest = tab_bar_transform(360.0, 0.0, 1.0);
        assert_eq!(rest.scale_x, 1.0);
        assert_eq!(rest.scale_y, 1.0);
        assert_eq!(rest.translation_x, 0.0);
        for travel in [-1.0, 0.0, 1.0] {
            let layer = tab_bar_transform(360.0, 1.0, travel);
            assert_eq!(layer.translation_y, 0.0);
            assert!((360.0 * layer.scale_x - (374.14 + 2.63 * travel.abs())).abs() < 1e-3);
            assert!((62.0 * layer.scale_y - (64.4352 - 0.45294 * travel.abs())).abs() < 1e-3);
            assert!((layer.translation_x - travel * 2.756).abs() < 1e-3);
        }
        for width in [360.0, 398.0] {
            let layer = tab_bar_transform(width, 1.0, 0.0);
            assert!((width * (layer.scale_x - 1.0) - 14.14).abs() < 0.001);
        }
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
    fn flight_lens_uses_the_measured_sequential_warps() {
        let glass = tab_flight_lens_material(cranpose_ui_graphics::Color::BLACK, 1.0);
        let generic_lens = Glass::lens();
        assert!(glass.lift.is_some_and(|lift| (0.0..=0.15).contains(&lift)));
        assert_eq!(glass.refraction_depth_dp, Some(36.0));
        assert_eq!(
            glass.refraction,
            crate::material::GlassRefraction::EdgeLens { reach_dp: 9.0 }
        );
        assert!(glass.refraction_curve < generic_lens.refraction_curve);
        assert!(glass.dispersion * 0.3 < generic_lens.dispersion);
        assert_eq!(glass.blur_radius, Some(0.0));
        assert_eq!(glass.backdrop_blur, Some((4.0, 0.0)));
        assert_eq!(glass.highlight, 1.0);
        assert_eq!(glass.key_fill.map(|light| light.curvature), Some(0.8));
        assert!(
            glass.shadow,
            "the moving lens needs its target-visible SDF contact outline"
        );
        assert!(glass.tint.is_some_and(|tint| tint.a() == 0.0));
        assert_eq!(glass.face_response.unwrap().gain, 0.97);
        assert_eq!(glass.meniscus_absorption, 0.0);
        assert_eq!(glass.adaptive_frost, 0.0);
    }

    #[test]
    fn resting_lens_does_not_blend_two_different_text_projections() {
        let rest = tab_flight_lens_material(Color::BLACK, 0.0);
        let held = tab_flight_lens_material(Color::BLACK, 1.0);
        assert_eq!(rest.optical_zoom, 1.0);
        assert_eq!(rest.backdrop_blur, Some((4.0, 1.0)));
        assert_eq!(rest.refraction_depth_dp, Some(36.0));
        assert_eq!(rest.fold_depth, 0.0);
        assert_eq!(rest.highlight, 0.0);
        assert_eq!(rest.meniscus_absorption, 0.0);
        assert_eq!(rest.shadow_style.unwrap().color.a(), 0.0);
        assert_eq!(held.optical_zoom, 1.0);
        assert_eq!(rest.refraction_depth_dp, held.refraction_depth_dp);
        let dynamics = tab_flight_dynamics(
            TabFlightGeometry {
                center: (50.0, 31.0),
                base_size: Size::new(95.0, 54.0),
                strain: Size::new(0.0, 0.0),
                lens_position: 0.0,
                lens_activity: 0.0,
                resting_tint: Color::TRANSPARENT,
                accessory_center: None,
            },
            TabFlightNode {
                origin: (0.0, 0.0),
                size: Size::new(140.0, 100.0),
            },
        );
        assert_eq!(dynamics.activity, Some(1.0));
    }

    #[test]
    fn flight_lens_retains_neutral_tint_through_direct_motion() {
        assert_eq!(tab_flight_tint_multiplier(0.0), 1.0);
        assert!((tab_flight_tint_multiplier(1.0) - 0.75).abs() < f32::EPSILON);
        assert_eq!(tab_flight_tint_multiplier(-1.0), 1.0);
        assert!((tab_flight_tint_multiplier(2.0) - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn bar_surface_adapts_tone_to_its_foreground() {
        let glass = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK);
        assert_eq!(glass.blur_radius, Some(6.0));
        assert_eq!(glass.saturation, Some(1.0));
        assert_eq!(glass.lift, Some(0.0));
        assert_eq!(glass.refraction_depth, 0.0);
        assert_eq!(glass.refraction_depth_dp, Some(15.5));
        assert_eq!(
            glass.refraction,
            crate::material::GlassRefraction::Surface { reach_dp: 31.0 }
        );
        assert_eq!(glass.transmission_refraction, 1.0);
        assert_eq!(glass.adaptive_frost, 0.0);
        assert!(glass.adaptive_tone);
    }

    #[test]
    fn bar_surface_keeps_its_refraction_after_release() {
        let glass = tab_bar_surface_material(Color::BLACK);
        assert_eq!(glass.refraction_depth_dp, Some(15.5));
        assert_eq!(glass.face_response.unwrap().illumination, 0.0);
    }

    #[test]
    fn bar_surface_tone_tracks_the_local_foreground_polarity() {
        for foreground in [Color::BLACK, Color::WHITE] {
            let surface = tab_bar_surface_material(foreground);
            assert!(surface.adaptive_tone);
            assert_eq!(surface.foreground, Some(foreground));
            assert_eq!(surface.lift, Some(0.0));
        }
    }

    #[test]
    fn bar_surface_tone_keeps_the_face_tint_neutral() {
        for foreground in [Color::BLACK, Color::WHITE] {
            assert_eq!(
                tab_bar_surface_material(foreground).tint,
                Some(Color::TRANSPARENT)
            );
        }
    }

    #[test]
    fn contact_growth_follows_the_native_presentation_clock() {
        let mut samples = 0;
        for pressed in [false, true] {
            for route in 0..4 {
                let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(
                    cranpose_core::DefaultScheduler,
                ));
                let mut activity = cranpose_animation::Animatable::new(
                    if pressed { 0.0 } else { 1.0 },
                    runtime.handle(),
                );
                activity.animate_to_at(
                    f32::from(pressed),
                    tab_lens_activity_motion(pressed),
                    1_000_000_000,
                );
                let mut error = 0.0;
                let mut count = 0;
                for line in include_str!("../../tests/fixtures/native_tab_contact.csv")
                    .lines()
                    .skip(1)
                {
                    let sample = line
                        .split(',')
                        .map(|v| v.parse::<f64>().expect("contact sample"))
                        .collect::<Vec<_>>();
                    if sample[0] != f64::from(pressed) || sample[1] != f64::from(route) {
                        continue;
                    }
                    runtime
                        .handle()
                        .drain_frame_callbacks(1_000_000_000 + (sample[2] * 1e9) as u64);
                    error += (activity.state().value().clamp(0.0, 1.0) - sample[3] as f32).powi(2);
                    count += 1;
                }
                assert!(count >= 30, "every native route must execute");
                let rms = (error / count as f32).sqrt();
                assert!(
                    rms < 0.025,
                    "native contact progress pressed={pressed} route={route} RMS: {rms}"
                );
                samples += count;
            }
        }
        assert!(samples >= 300);
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
    fn native_cells_overlap_without_crossing_the_strip_ends() {
        let geometry = TabGeometry::new(88.0, 4);
        assert_eq!(geometry.width, 352.0);
        assert!((geometry.cell_width - 95.0).abs() < 1e-4);
        for (index, center) in [72.5, 158.16667, 243.83333, 329.5].into_iter().enumerate() {
            let actual =
                25.0 + tab_lens_resting_left(index, geometry.pitch, 4) + geometry.cell_width * 0.5;
            assert!((actual - center).abs() < 1e-4);
        }
        for count in [0, 1, 2, 4, 8] {
            for allocation in [1.0, 24.0, 78.0, 120.0, 400.0] {
                let geometry = TabGeometry::new(allocation, count);
                let last = tab_lens_resting_left(count, geometry.pitch, count);
                assert!((last + geometry.cell_width - geometry.width).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn selection_width_uses_the_destination_allocation() {
        for (allocation, native_width) in [(88.0, 95.0), (97.5, 104.5)] {
            assert!((tab_lens_rest_width(allocation) - native_width).abs() < 1e-4);
        }
    }
    #[test]
    fn held_drag_coordinates_follow_the_lifted_cell_in_both_directions() {
        let geometry = TabGeometry::new(88.0, 4);
        for (travel, physical_x) in [(1.0, 304.66666), (-1.0, 47.333332)] {
            let transform = tab_bar_transform(360.0, 1.0, travel);
            let center = geometry.width * 0.5;
            let local_x =
                (physical_x - center - transform.translation_x) / transform.scale_x + center;
            let optical_x = geometry.optical_pointer_x(local_x, 1.0, travel);
            assert!(
                (optical_x - physical_x).abs() < 0.001,
                "pointer drift: {}",
                optical_x - physical_x
            );
            assert_eq!(
                geometry.optical_pointer_x(physical_x, 0.0, travel),
                physical_x
            );
        }
    }
    #[test]
    fn ink_selection_uses_the_same_bounded_shape_before_face_magnification() {
        let geometry = TabGeometry::new(88.0, 4);
        for activity in [0.0, 0.5, 1.0] {
            for value in [-1000.0, -0.1, 0.0, 0.1, 1000.0] {
                let strain = Size::new(value, -value * 1.5);
                let selected = tab_ink_selection(geometry, 120.0, activity, strain, Color::BLUE);
                let (width, height) = tab_lens_base_size(geometry.cell_width, activity);
                let size = tab_lens_deformed_size(Size::new(width, height), strain);
                assert_eq!(selected.content_zoom, 1.0 + 0.16 * activity);
                assert!((selected.bounds.width - size.width).abs() < 0.0001);
                assert!((selected.bounds.height - size.height).abs() < 0.0001);
                assert!((selected.bounds.x + selected.bounds.width * 0.5 - 167.5).abs() < 0.0001);
                assert!(selected.bounds.width > 0.0 && selected.bounds.height > 0.0);
            }
        }
    }
}
