//! The floating glass tab bar: a capsule of tabs over live content with a
//! liquid selection blob (dual-spring stretch), plus an optional detached
//! circular accessory (the iOS 26 search button).

use crate::material::{
    neutral_surface_lift, neutral_surface_tint, Glass, GlassDynamics, GlassMorph, GlassShadow,
    LiquidModifierExt,
};
use crate::motion::LiquidMotion;
use crate::theme::{liquid_colors, liquid_typography, LiquidTypography};
use cranpose_macros::composable;
use cranpose_ui::text::{FontWeight, SpanStyle, TextStyle};
use cranpose_ui::widgets::{
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Column, ColumnSpec, Row, RowSpec,
    Text,
};
use cranpose_ui::{Brush, Color, CornerRadii, Modifier, PointerInputScope, Rect, Size};
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

/// The settled bubble's face. The reference selected capsule is a crisp
/// MILKY pill brighter than the bar in both schemes (bar_over_orange_
/// purple: a washed lavender-white capsule over the purple tile) — never
/// the gray control fill, which has so little presence over vivid tiles
/// that the rest bubble read as nothing but its own blurred drop shadow.
fn tab_rest_milk(colors: crate::theme::LiquidColors) -> Color {
    let light_scheme = colors.label.r() < 0.5;
    if light_scheme {
        Color::WHITE.with_alpha(0.55)
    } else {
        Color::WHITE.with_alpha(0.16)
    }
}

const BAR_HEIGHT: f32 = 64.0;
/// Resting bubble height: the reference bubble fills the bar to ~4dp
/// insets (56/64 — a 52dp blob read as a floating pill with odd gaps,
/// user screenshots on both bars).
const BLOB_HEIGHT: f32 = 56.0;
const BLOB_MARGIN: f32 = 4.0;
/// Raised bubble growth over the resting blob — VERTICAL ONLY. Measured
/// on the raw hold recording (bottom_bar_click_to_change_then_hold_a_
/// little.mov, held frames f_0260/f_0300): the held bubble keeps its rest
/// width (348px over a 319px cell pitch = the 1.09 rest factor) while its
/// height grows 56dp -> ~82dp (238px against the 185px bar), poking ~9dp
/// past BOTH bar edges. A uniform 1.375x projection made a 1.5-pitch-wide
/// capsule that sat over the neighbor cell and garbled its glyphs for the
/// whole hold.
const FLIGHT_LENS_HEIGHT_PROJECTION: f32 = 68.0 / BLOB_HEIGHT;
/// A whisper of ellipse bow on the raised capsule — enough to soften the
/// corners off a flat-topped rectangle (feedback item 7) without bowing
/// it into a circle/oval. The reference raised bubble is a CURVED ROUNDED
/// RECTANGLE: wider than tall, mostly-straight top and bottom edges,
/// rounded corners (tab-swipe f_028 over "Account"). A 0.55 blend rounded
/// it into a circle (user: "should not be this circle-like round, and not
/// oval, more like curved rounded rectangle").
const FLIGHT_ELLIPSE_BLEND: f32 = 0.25;
/// Resting bubble width over the cell pitch. Measured on the reference
/// (bottom-bar-click f_0000: bubble 96 over pitch 87.5): the bubble is
/// barely wider than its cell, so a CELL-CENTERED rest keeps its edge
/// flush inside the pill even at the end cells — the end overhang
/// (tab·0.05) never exceeds [`BLOB_MARGIN`].
const TAB_LENS_REST_WIDTH_FACTOR: f32 = 1.10;
/// Fraction of the shared droplet stretch the tab bubble surface carries:
/// the reference mid-swipe bubble elongates to ~1.15x its rest width
/// (tab-swipe T400), never a multi-cell worm.
const TAB_STRAIN_RESPONSE: f32 = 0.30;
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

fn tab_flight_lens_material(foreground: cranpose_ui_graphics::Color, accent: Color) -> Glass {
    Glass::lens()
        .no_clip()
        .tint(neutral_surface_tint(foreground, 0.06, 0.05))
        // The color-mask act is the LENS's own optic: dark ink transmitted
        // through the bubble takes the accent (reference behavior); the
        // cells beneath keep their honest colors.
        .ink_recolor(accent, 0.85)
        .blur_radius(0.0)
        // The raised-bubble optic, judged by vision against the store-bar
        // reference raised frames: a CLEAR face, a THIN vivid rim band,
        // the bar's edges pulled INWARD through the glass, the ridden
        // glyph "a little bigger". Composed from the one field:
        //  - a thin band (toggle-class depth): a full-face field ran the
        //    descending branch across the whole face and inflated
        //    everything (the "ours is bigger" fault) — the reference face
        //    is flat with the re-image living in a narrow rim zone;
        .refraction_depth(0.30)
        .refraction_curve(0.25)
        //  - full transmission INSIDE that band keeps the compressed rim
        //    re-image and its chromatic split (a damped branch erased the
        //    reference's vivid fringes);
        .transmission_refraction(1.0)
        //  - the rim fold re-images outside content inward at the edge;
        .fold_depth(11.0)
        //  - the PINCH is the whole-dome minify: the bar seen through the
        //    bubble samples outward across the entire geometric interior
        //    and READS SMALLER — the user's thrice-repeated requirement.
        //    A rim-band fold alone never compressed the bar's mid-face,
        //    and an apex zoom > 1 enlarged it (shipped once; wrong). The
        //    reference raised frame squeezes neighbor glyphs and the
        //    bar's white bottom edge INTO the rim — the pinch is overt,
        //    not homeopathic.
        .dome_zoom(0.84)
        //  - the anchor-centered apex core magnifies the ridden glyph
        //    (the reference rocket swells well past its cell); the core is
        //    tight enough that the label row below stays in the pinch.
        .optical_zoom(1.3)
        // The ride's rainbow (user feedback item 3): the riding bubble's
        // caps carry the toggle-class chromatic split; the REST bubble
        // stays the verified subtle look because dispersion and the fold
        // ride press_depth (shallow floor at rest, deep on the ride).
        .dispersion(1.1)
        // Raised milk: the reference's held bubble face lifts modestly
        // toward white as it rises (on-white-click-hold sheet, held rows
        // f_0240+); lift scales by activity, so the verified resting look
        // is untouched. Kept subtle — the fully clear face read flat, the
        // heavy wash of earlier rounds read foggy.
        .lift(0.02)
        .highlight(0.06)
        // Bottom-biased contact shadow (user feedback item 3: the ride
        // bubble casts a visible soft shadow on the white bar; the default
        // lens spread erased it on white-on-white).
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
        // 4dp let bold section headers read through the face as strong
        // dark smears; the reference face (bar_over_headers) drowns them
        // to a faint ghost while the color wash survives. The flat-tile
        // composite solve below is blur-invariant, so the measured
        // tint/saturation/lift stay pinned.
        .blur_radius(9.0)
        // Measured on bar_over_orange_purple: tile (242,150,77) reads
        // (253,210,168) through the bar. Saturation deepens the channels
        // before the screen lift, so the lift knob runs higher than the
        // per-channel solve (~0.52); this pair lands the measured composite
        // and drowns the fold ghosts the way the reference does.
        // Saturation preserved through the frost (the reference README:
        // "orange stays orange through the glass") — 0.60 lift flattened
        // vivid tiles and their bright features to near-white where the
        // reference bar face keeps a warm washed hue.
        .saturation(1.24)
        .lift(neutral_surface_lift(foreground, 0.52, -0.24))
        .highlight(0.20)
        // The reference bar folds nearby content inside its long edges:
        // section headers under the top edge render mirrored upside-down
        // (bar_headers_folded) — the same pure-displacement fold as the
        // toggle, shallower.
        .fold_depth(8.0)
        .adaptive_frost(foreground, 0.28)
}

fn tab_flight_tint_multiplier(activity: f32) -> f32 {
    1.0 - 0.25 * activity.clamp(0.0, 1.0)
}

fn tab_lens_activity_motion(raised: bool) -> cranpose_animation::AnimationType {
    if raised {
        // Continuous contact rise on the toggle's calibrated spring (~10
        // frames to full, the reference on-white gesture rows).
        cranpose_animation::spring(0.9, 1400.0)
    } else {
        // The return into the bar keeps its slower material drain.
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
/// rounded end). The rest width ([`TAB_LENS_REST_WIDTH_FACTOR`]) is what
/// keeps the end cells legal — its overhang stays within [`BLOB_MARGIN`].
/// Public so alignment tests assert the same rule the widget settles to.
pub fn tab_lens_resting_left(selected: usize, tab_width: f32, count: usize) -> f32 {
    tab_width * selected.min(count.saturating_sub(1)) as f32
}

/// The resting bubble's width for a cell pitch — the second half of the
/// public resting contract ([`tab_lens_resting_left`] gives the position).
pub fn tab_lens_rest_width(tab_width: f32) -> f32 {
    tab_width * TAB_LENS_REST_WIDTH_FACTOR
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
    let rest_width = tab_width * TAB_LENS_REST_WIDTH_FACTOR;
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
    // The rest bubble is a capsule (r = h/2); the raised bubble keeps that
    // full corner and additionally blends its SDF toward an ellipse
    // (activity-scaled at the material layer), so the tall held body reads
    // as the reference's continuously curved oval — a reduced corner
    // radius squares the silhouette into a rounded rectangle instead.
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
            // 1.1 was calibrated for the old 1.5-pitch-wide bubble; on the
            // cell-width body the same amplitude curls the rim into marble
            // swirls mid-travel where the reference keeps clean arc smears.
            wobble_amplitude: 0.5 * energy,
            wobble_phase: geometry.lens_position * 0.045,
            bulge_amplitude: geometry.pose.bulge_amplitude.min(8.0) * activity,
            bulge_direction: geometry.pose.bulge_direction,
            ellipse_blend: FLIGHT_ELLIPSE_BLEND,
            deformation: Some(crate::material::GlassDeformation::incompressible(
                geometry.pose.axis,
                // Reference mid-swipe bubble elongates to ~1.15x its rest
                // width (tab-swipe T400) — the raw droplet stretch (up to
                // 1.5) read as a two-cell worm.
                1.0 + (geometry.pose.stretch - 1.0) * activity * TAB_STRAIN_RESPONSE,
            )),
            // The optical axis rides the GLYPH row, not the bubble center:
            // the reference re-images the raised cell around its icon (the
            // rocket swells from its own center; the label below squeezes
            // into the rim with the rest of the bar).
            zoom_anchor: (0.0, -5.0),
        }),
        activity: Some(activity),
        // Ride depth: the raised bubble runs the full vivid rim (fold +
        // dispersion at strength); the resting bubble keeps a shallow
        // floor so its verified look is untouched.
        press_depth: Some(0.3 + 0.7 * activity),
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
                ))
            })
            .with(|state| *state);
            // Held glass comes CLOSER and lights up: while a finger rests
            // on the bar the whole surface scales up a touch and the shader
            // concentrates saturation + a gradient highlight under the
            // finger (user-observed reference behavior of every touched
            // glass surface).
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
            // The rise transforms the WHOLE stack — pill, cells, and the
            // floating lens are one optical body. Lifting only the pill
            // scaled the cells away from the unscaled lens, drifting the
            // bubble off its cell in proportion to the cell's distance
            // from the pill center.
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
            // The stack is pinned to the pill height so the mounting lens
            // (taller than the bar) can never inflate it — an unpinned stack
            // grew on press and the centering Row shifted the whole bar down.
            Box(
                bar_lift.then(Modifier::empty().height(BAR_HEIGHT)),
                BoxSpec::default(),
                move || {
                    let tabs = Rc::clone(&tabs);
                    let typography = typography.clone();
                    let on_select = Rc::clone(&on_select);
                    // The bar's edge is defined by shadow and contrast, not a
                    // bright rim stroke.
                    let pill = Modifier::empty()
                        .glass_effect_with(
                            // Dark labels must never sink into dark content
                            // scrolling beneath: the glass lifts adaptively over
                            // dark backdrops (inert over the light ones the
                            // pinned captures use).
                            tab_bar_surface_material(colors.label),
                            move || {
                                let press = bar_press.get().clamp(0.0, 1.0);
                                let (touch_x, touch_y) = bar_touch.get();
                                // The risen bubble is a WINDOW through the
                                // bar's milk: the reference tile reads MORE
                                // vividly inside the raised bubble than
                                // through the surrounding frost.
                                let (lens_px, lens_activity, lens_tab_w, _) = lens_x_outer.get();
                                let (window_w, window_h) =
                                    tab_lens_base_size(lens_tab_w, lens_activity);
                                let clear_window = (lens_activity > 0.001).then(|| {
                                    crate::material::GlassClearWindow {
                                        center: (
                                            BLOB_MARGIN + lens_px + lens_tab_w * 0.5,
                                            BAR_HEIGHT * 0.5,
                                        ),
                                        size: (window_w, window_h),
                                        radius: -1.0,
                                        strength: lens_activity,
                                    }
                                });
                                GlassDynamics {
                                    highlight_boost: 0.45 * press,
                                    saturation_boost: 0.12 * press,
                                    touch: (press > 0.01).then_some((touch_x, touch_y, press)),
                                    clear_window,
                                    ..Default::default()
                                }
                            },
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
                            let resting_lens_x = tab_lens_resting_left(selected, tab_width, count);
                            let lens_axis =
                                crate::motion::remember_liquid_drag_axis(resting_lens_x);
                            // Controlled-state restore only: while a finger
                            // holds the bar the axis belongs to the gesture
                            // (an unconditional settle re-targeted the lens
                            // back every frame and cancelled the touch-down
                            // attract — live report).
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
                            // The reference raises the surface CONTINUOUSLY:
                            // depth, chroma and scale rise together over ~10
                            // frames (on-white gesture rows) — contact and
                            // return ride the same animated channel.
                            let lens_activity = lens_activity_anim.get();
                            // The color-mask act under the LIVE bubble is the
                            // LENS MATERIAL's own optic — the shader recolors
                            // the ink it transmits — never a recolor of the
                            // cells themselves (an element recolor gets
                            // refracted into accent smears around the bubble
                            // rim). The accented CELL follows the lens center
                            // crossing, not the committed model: a click
                            // promotes the destination the instant `selected`
                            // snaps while the bubble is still at the origin
                            // (on-white-click 0ms: Conversation teal, bubble
                            // parked at Translate — the reference hands the
                            // accent off mid-flight).
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

                            // Swipe/tap surface across the whole pill interior:
                            // the shared lens gesture (crate::motion) with the
                            // bar's clamp rules and hold feedback.
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

                            // Publish the lens springs for the overlay rendered
                            // ABOVE the finished bar (outside this glass layer, so
                            // the lens magnifies icons and glass together).
                            let published = (lens_x, lens_activity, tab_width, lens_pose);
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
                    let (lens_px, lens_activity, lens_tab_w, pose) = lens_x_outer.get();
                    let lens_w = lens_tab_w * TAB_LENS_REST_WIDTH_FACTOR;
                    let lens_h = BLOB_HEIGHT * FLIGHT_LENS_HEIGHT_PROJECTION;
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
                        resting_tint: tab_rest_milk(colors),
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
                        // required_size: the stack is pinned to BAR_HEIGHT so
                        // the taller lens can never inflate the bar; the node
                        // still measures (and draws) at its full size and the
                        // offset centers it on the pill.
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
    fn resting_lens_centers_on_its_cell_and_stays_inside_the_pill() {
        let tab = 78.0;
        // Every cell settles cell-centered, the reference behavior
        // (bottom-bar-click f_0000: end bubble center == cell center).
        assert_eq!(tab_lens_resting_left(0, tab, 5), 0.0);
        assert_eq!(tab_lens_resting_left(1, tab, 5), tab);
        assert_eq!(tab_lens_resting_left(3, tab, 5), 3.0 * tab);
        assert_eq!(tab_lens_resting_left(4, tab, 5), 4.0 * tab);
        assert_eq!(tab_lens_resting_left(9, tab, 5), 4.0 * tab);
        // What makes the cell-centered end legal: the rest bubble's
        // overhang past its cell never exceeds the pill inset, so the
        // bubble edge lands flush inside the pill's rounded end instead
        // of crossing it (the reference gap).
        let overhang = tab * (TAB_LENS_REST_WIDTH_FACTOR - 1.0) * 0.5;
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
        // Measured on the raw hold recording: the held bubble keeps its
        // rest width (1.09x pitch held vs 1.10x at rest — no growth) and
        // stands ~82dp against the 64dp bar, ~9dp past both edges.
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
        // The bubble fills the bar to ~4dp insets (56/64, re-judged
        // against the user's reference frames).
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
        // A modest raised milk (activity-scaled) — clear enough to keep
        // the wcKSRD face readable, lifted enough to match the held rows.
        assert!(glass.lift.is_some_and(|lift| (0.0..=0.15).contains(&lift)));
        // The etalon's full-face field: interior spans edge to center so a
        // gap-centered flight frame pulls both neighbor icons in instead of
        // transmitting blank bar white (the two-cell milk blob).
        assert!(glass.refraction_depth <= 0.5, "thin rim band, flat face");
        assert!(glass.refraction_curve < generic_lens.refraction_curve);
        // The ride runs toggle-class dispersion; the REST bubble stays
        // subtle because the dynamics floor press_depth 0.3 scales it —
        // the effective resting split sits below the generic default.
        assert!(
            glass.dispersion > 0.0,
            "the ride carries the chromatic split"
        );
        assert_eq!(glass.blur_radius, Some(0.0));
        assert!(glass.highlight < generic_lens.highlight);
        assert!(
            glass.shadow,
            "the moving lens needs its target-visible SDF contact outline"
        );
        // Structural, not a tuned appearance number (per the vision-match
        // rule: judge the tint by eye, don't pin its alpha): a clear,
        // near-transparent glass so the covered icon shows through recolored
        // and fringed, never a milky sticker.
        assert!(glass
            .tint
            .is_some_and(|tint| tint.a() < 0.2 && tint.r() < 0.5));
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
        // 9dp drowns bold section headers to the reference's faint ghost
        // (bar_over_headers) — 4dp let them read as strong dark smears.
        // Structural (vision rule): a heavy legibility blur, boosted
        // saturation so tiles stay vivid through the frost, a positive
        // milk lift over light foregrounds, a shallow field, some frost.
        assert!(glass.blur_radius.is_some_and(|radius| radius > 4.0));
        assert!(glass.saturation.is_some_and(|saturation| saturation > 1.0));
        assert!(glass.lift.is_some_and(|lift| lift > 0.0));
        assert!(glass.refraction_depth < 1.0);
        assert!(glass.adaptive_frost > 0.0);
    }

    #[test]
    fn bar_surface_lift_tracks_the_local_foreground_polarity() {
        let light_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK);
        assert!(light_surface.lift.is_some_and(|lift| lift > 0.0));

        let dark_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::WHITE);
        assert!(dark_surface.lift.is_some_and(|lift| lift < 0.0));
    }

    #[test]
    fn bar_surface_tint_separates_from_same_polarity_backdrops() {
        let light_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK)
            .tint
            .expect("bar tint");
        // Structural polarity, not tuned alphas (judge the wash by eye):
        // a light foreground bar takes a dark, fully-transparent-at-rest
        // tint; a dark bar takes a light tint with a whisper of alpha.
        assert!(light_surface.r() < 0.5);

        let dark_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::WHITE)
            .tint
            .expect("bar tint");
        assert!(dark_surface.r() > 0.5);
        assert!(dark_surface.a() < 0.2);
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
}
