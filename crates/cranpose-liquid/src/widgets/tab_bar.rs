//! The floating glass tab bar: a capsule of tabs over live content with a
//! liquid selection blob (dual-spring stretch), plus an optional detached
//! circular accessory (the iOS 26 search button).

use crate::material::{neutral_surface_tint, Glass, GlassDynamics, GlassMorph, LiquidModifierExt};
use crate::motion::LiquidMotion;
use crate::theme::{liquid_colors, liquid_typography};
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
use cranpose_ui_graphics::{GlassProfileCurve, GlassSurfaceProfile};
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
        self.icon_scale = if scale.is_finite() {
            scale.clamp(0.5, 1.5)
        } else {
            1.0
        };
        self
    }
}

const BAR_HEIGHT: f32 = 64.0;
const BLOB_HEIGHT: f32 = 52.0;
const BLOB_MARGIN: f32 = 8.0;
const FLIGHT_LENS_WIDTH_FACTOR: f32 = 1.17;
const FLIGHT_LENS_HEIGHT_FACTOR: f32 = 1.0;
const FLIGHT_LENS_DEPTH_ACTIVE: f32 = 0.0;
const FLIGHT_LENS_DEPTH_MOTION: f32 = 0.12;
const FLIGHT_SURFACE_RADIAL_POWER: f32 = 1.5;
const FLIGHT_SURFACE_AXIS_COUPLING: f32 = 0.78;
/// Width allotted to each tab inside the pill.
const TAB_WIDTH: f32 = 78.0;
/// Plain icon frame size (its path occupies about 25dp over 11dp labels).
const TAB_ICON_SIZE: f32 = 32.0;
const TAB_LABEL_SIZE: f32 = 11.0;
/// The drag lens overflows the pill vertically like the reference bubble.
const TAP_SLOP: f32 = 6.0;
const ACCESSORY_GAP: f32 = 10.0;
const VISUAL_HANDOFF_TOLERANCE_IN_TABS: f32 = 0.12;

fn flight_surface_profile() -> GlassSurfaceProfile {
    let x_profile = GlassProfileCurve::from_points(&[
        (0.0, 0.50),
        (0.16, 0.508),
        (0.34, 0.54),
        (0.56, 0.44),
        (0.78, 0.34),
        (1.0, 0.24),
    ])
    .expect("tab flight X-Z profile is valid");
    let y_profile = GlassProfileCurve::from_points(&[
        (0.0, 0.50),
        (0.16, 0.50),
        (0.30, 0.50),
        (0.52, 0.515),
        (0.76, 0.558),
        (1.0, 0.51),
    ])
    .expect("tab flight Y-Z profile is valid");
    GlassSurfaceProfile::new(x_profile, y_profile, 30.0, FLIGHT_SURFACE_RADIAL_POWER)
        .and_then(|profile| profile.with_axis_coupling(FLIGHT_SURFACE_AXIS_COUPLING))
        .expect("tab flight surface profile is coherent")
}

fn tab_flight_lens_material(
    foreground: cranpose_ui_graphics::Color,
    accent: cranpose_ui_graphics::Color,
) -> Glass {
    Glass::lens()
        .no_clip()
        .tint(neutral_surface_tint(foreground, 0.08, 0.10))
        .content_recolor(accent, 1.0)
        .blur_radius(2.0)
        .lift(0.0)
        .highlight(0.32)
        .surface_profile(flight_surface_profile())
        .sheen(0.12)
        .chromatic_aberration(1.2)
        .displacement(55.0)
}

fn tab_bar_surface_material(foreground: cranpose_ui_graphics::Color) -> Glass {
    Glass::regular()
        .tint(neutral_surface_tint(foreground, 0.028, 0.04))
        .blur_radius(16.0)
        .saturation(0.95)
        .lift(0.48)
        .highlight(0.20)
        .surface_profile(
            GlassSurfaceProfile::regular()
                .with_depth(3.0)
                .expect("tab bar surface depth is valid"),
        )
        .adaptive_frost(foreground, 0.42)
        .edge_fold(1.0)
}

fn tab_flight_depth_boost(activity: f32, energy: f32) -> f32 {
    activity.clamp(0.0, 1.0)
        * (FLIGHT_LENS_DEPTH_ACTIVE + FLIGHT_LENS_DEPTH_MOTION * energy.clamp(0.0, 1.0))
}

fn tab_flight_tint_multiplier(activity: f32) -> f32 {
    1.0 - 0.60 * activity.clamp(0.0, 1.0)
}

fn tab_lens_activity_motion(active: bool) -> cranpose_animation::AnimationType {
    if active {
        LiquidMotion::smooth()
    } else {
        cranpose_animation::spring(1.0, 1400.0)
    }
}

fn tab_lens_left(pointer_x: f32, tab_width: f32, count: usize) -> f32 {
    (pointer_x - tab_width * 0.5).clamp(-tab_width * 0.2, tab_width * (count as f32 - 0.45))
}

fn tab_visual_index(
    selected: usize,
    lens_x: f32,
    tab_width: f32,
    count: usize,
    direct: bool,
) -> usize {
    if count == 0 {
        return 0;
    }
    let selected = selected.min(count - 1);
    if !direct || !lens_x.is_finite() || tab_width <= f32::EPSILON {
        return selected;
    }
    (lens_x / tab_width)
        .round()
        .clamp(0.0, count.saturating_sub(1) as f32) as usize
}

fn tab_lens_owns_visual_selection(
    direct: bool,
    lens_x: f32,
    target_x: f32,
    tab_width: f32,
) -> bool {
    direct
        || (lens_x - target_x).abs()
            > tab_width.max(f32::EPSILON) * VISUAL_HANDOFF_TOLERANCE_IN_TABS
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
                Box(
                    Modifier::empty()
                        .size(Size::new(20.0, FRAME_HEIGHT))
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(Brush::solid(color), CornerRadii::uniform(5.0));
                            scope.draw_rect_at(
                                Rect {
                                    x: 7.0,
                                    y: 4.5,
                                    width: 6.0,
                                    height: 1.5,
                                },
                                Brush::solid(Color::WHITE),
                            );
                        }),
                    BoxSpec::default(),
                    move || {
                        Box(
                            Modifier::empty()
                                .offset(4.0, 11.0)
                                .size(Size::new(12.0, 12.0)),
                            BoxSpec::default(),
                            move || crate::icons::Icon(icon, 12.0, Color::WHITE),
                        );
                    },
                );
            }
        },
    );
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

fn tab_flight_launch_wake(
    pose: crate::dynamics::LiquidPose,
    base_height: f32,
) -> Option<(f32, f32)> {
    let compression = ((1.0 - pose.stretch) / (1.0 - crate::dynamics::STRETCH_MIN)).clamp(0.0, 1.0);
    if compression <= 0.03 {
        return None;
    }
    let amplitude = base_height * (0.04 + 0.04 * compression);
    let trailing = (-pose.axis.0, -pose.axis.1);
    Some((amplitude, trailing.1.atan2(trailing.0)))
}

/// A unified floating glass tab bar with every destination inside one pill.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBar(
    modifier: Modifier,
    tabs: Vec<LiquidTab>,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
) {
    LiquidTabBarLayout(modifier, tabs, selected, on_select, false, || {});
}

/// A floating glass tab bar with a detached accessory to its right.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBarWithAccessory(
    modifier: Modifier,
    tabs: Vec<LiquidTab>,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    accessory: impl FnMut() + 'static,
) {
    LiquidTabBarLayout(modifier, tabs, selected, on_select, true, accessory);
}

#[composable]
#[allow(non_snake_case)]
fn LiquidTabBarLayout(
    modifier: Modifier,
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
                                (constrained / count as f32).min(TAB_WIDTH)
                            } else {
                                TAB_WIDTH
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
                                tab_lens_activity_motion(lens_activity_target > 0.5),
                                "tabbar-lens-activity",
                            );
                            let lens_activity = move || {
                                if lens_activity_target > 0.5 {
                                    1.0
                                } else {
                                    lens_activity_anim.get()
                                }
                            };

                            let cells_tabs = Rc::clone(&tabs);
                            let visual_index = tab_visual_index(
                                selected,
                                lens_x,
                                tab_width,
                                count,
                                tab_lens_owns_visual_selection(
                                    lens_pressed.get(),
                                    lens_x,
                                    resting_lens_x,
                                    tab_width,
                                ),
                            );
                            Row(Modifier::empty(), RowSpec::default(), move || {
                                for (index, tab) in cells_tabs.iter().enumerate() {
                                    let color = if index == visual_index {
                                        colors.accent
                                    } else {
                                        colors.label
                                    };
                                    let label_for_semantics = tab.label;
                                    let cell = Modifier::empty()
                                        .size(Size::new(tab_width, BLOB_HEIGHT))
                                        .semantics(move |config| {
                                            config.is_button = true;
                                            config.is_clickable = true;
                                            config.content_description =
                                                Some(label_for_semantics.to_string());
                                        });
                                    let icon = tab.icon;
                                    let icon_style = tab.icon_style;
                                    let icon_scale = tab.icon_scale;
                                    let label = tab.label;
                                    let label_style = TextStyle {
                                        span_style: SpanStyle {
                                            color: Some(color),
                                            font_size: cranpose_ui::text::TextUnit::Sp(
                                                TAB_LABEL_SIZE,
                                            ),
                                            font_weight: Some(FontWeight::MEDIUM),
                                            ..typography.caption1.span_style.clone()
                                        },
                                        ..typography.caption1.clone()
                                    };
                                    Box(
                                        cell,
                                        BoxSpec::default().content_alignment(Alignment::CENTER),
                                        move || {
                                            let label_style = label_style.clone();
                                            Column(
                                                Modifier::empty(),
                                                ColumnSpec::default().horizontal_alignment(
                                                    HorizontalAlignment::CenterHorizontally,
                                                ),
                                                move || {
                                                    TabIcon(icon, icon_style, color, icon_scale);
                                                    Text(
                                                        label,
                                                        Modifier::empty(),
                                                        label_style.clone(),
                                                    );
                                                },
                                            );
                                        },
                                    );
                                }
                            });

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
                            let published = (lens_x, lens_activity(), tab_width, lens_pose);
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
                    {
                        let lens_w = lens_tab_w * FLIGHT_LENS_WIDTH_FACTOR;
                        let lens_h = BAR_HEIGHT * FLIGHT_LENS_HEIGHT_FACTOR;
                        // Node headroom for the deformation extremes (max axis
                        // stretch + leading bulge, max ortho swell) and rim glow.
                        let deformation_headroom =
                            crate::dynamics::STRETCH_MAX.max(1.0 / crate::dynamics::STRETCH_MIN);
                        let node_w =
                            lens_w * deformation_headroom + crate::dynamics::BULGE_MAX + 20.0;
                        let node_h =
                            lens_h * deformation_headroom + crate::dynamics::BULGE_MAX + 16.0;
                        let lens_center_x = BLOB_MARGIN + lens_px + lens_tab_w * 0.5;
                        let node_x = lens_center_x - node_w * 0.5;
                        let pill_w = lens_tab_w * count as f32 + 2.0 * BLOB_MARGIN;
                        // Accessory circle center in lens-node-local coords.
                        let accessory_cx =
                            pill_w + tab_bar_accessory_gap(has_accessory) + BAR_HEIGHT * 0.5
                                - node_x;
                        let accessory_cy = node_h * 0.5;
                        let lens = Modifier::empty()
                            // required_size: the stack is pinned to BAR_HEIGHT so
                            // the taller lens can never inflate the bar; the node
                            // still measures (and draws) at its full size and the
                            // offset centers it on the pill.
                            .required_size(Size::new(node_w, node_h))
                            .offset(node_x, tab_lens_node_top(node_h))
                            .glass_effect_with(
                                tab_flight_lens_material(colors.label, colors.accent),
                                move || {
                                    let (base_w, base_h) =
                                        tab_lens_base_size(lens_tab_w, lens_activity);
                                    let energy = pose.energy();
                                    // Continuous-curvature read: the resting lens
                                    // is a flattened squircle, not a stadium; it
                                    // rounds toward a capsule only at speed.
                                    let radius = base_h * (0.48 + 0.02 * energy);
                                    // The search circle joins the liquid field only
                                    // when the lens edge actually gets within glue
                                    // reach — passing glue, not a permanent overdraw
                                    // (a far shape re-rendered by this node would
                                    // paint a spurious rim over the real button).
                                    let glue = 20.0;
                                    let edge_gap = (accessory_cx - node_w * 0.5).abs()
                                        - base_w * pose.stretch.max(pose.ortho) * 0.5
                                        - BAR_HEIGHT * 0.5;
                                    // Join only when nearly touching: a parked lens
                                    // one cell away must not repaint the circle.
                                    let shapes = if edge_gap < 10.0 {
                                        vec![(
                                            accessory_cx,
                                            accessory_cy,
                                            BAR_HEIGHT,
                                            BAR_HEIGHT,
                                            -1.0,
                                        )]
                                    } else {
                                        Vec::new()
                                    };
                                    let (bulge_amplitude, bulge_direction) =
                                        tab_flight_launch_wake(pose, base_h).unwrap_or((
                                            pose.bulge_amplitude.min(8.0),
                                            pose.bulge_direction,
                                        ));
                                    GlassDynamics {
                                        morph: Some(GlassMorph {
                                            node_size: (node_w, node_h),
                                            primary: (
                                                node_w * 0.5,
                                                node_h * 0.5,
                                                base_w,
                                                base_h,
                                                radius,
                                            ),
                                            shapes,
                                            glue,
                                            // A whisper: the reference outline
                                            // stays one smooth curve in every
                                            // frame — strong lobes read as a
                                            // lumpy peanut.
                                            wobble_amplitude: 1.1 * energy,
                                            wobble_phase: lens_px * 0.045,
                                            bulge_amplitude,
                                            bulge_direction,
                                            ellipse_blend: 0.0,
                                            deformation: Some(pose.deformation()),
                                        }),
                                        surface_depth_boost: tab_flight_depth_boost(
                                            lens_activity,
                                            energy,
                                        ),
                                        tint_alpha_multiplier: Some(tab_flight_tint_multiplier(
                                            lens_activity,
                                        )),
                                        optical_strength: 0.18 + 0.67 * lens_activity,
                                        ..Default::default()
                                    }
                                },
                            );
                        Box(lens, BoxSpec::default(), || {});
                    }
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
    fn drag_pointer_centers_the_lens_and_preserves_end_overdrag() {
        let width = 100.0;
        assert_eq!(tab_lens_left(50.0, width, 4), 0.0);
        assert_eq!(tab_lens_left(250.0, width, 4), 200.0);
        assert_eq!(tab_lens_left(-100.0, width, 4), -20.0);
        assert_eq!(tab_lens_left(500.0, width, 4), 355.0);
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
    fn direct_drag_selects_the_visual_tab_under_the_lens() {
        assert_eq!(tab_visual_index(2, 0.0, TAB_WIDTH, 4, false), 2);
        assert_eq!(tab_visual_index(0, 2.0 * TAB_WIDTH, TAB_WIDTH, 4, true), 2);
        assert_eq!(tab_visual_index(0, 99.0 * TAB_WIDTH, TAB_WIDTH, 4, true), 3);
        assert!(tab_lens_owns_visual_selection(
            false, TAB_WIDTH, 0.0, TAB_WIDTH
        ));
        assert!(!tab_lens_owns_visual_selection(
            false,
            TAB_WIDTH * 0.05,
            0.0,
            TAB_WIDTH
        ));
    }

    #[test]
    fn unified_bar_has_no_detached_accessory_gap() {
        assert_eq!(tab_bar_accessory_gap(false), 0.0);
        assert_eq!(tab_bar_accessory_gap(true), 10.0);
    }

    #[test]
    fn one_lens_morphs_between_rest_and_full_flight_footprints() {
        let width = TAB_WIDTH * FLIGHT_LENS_WIDTH_FACTOR;
        let height = BAR_HEIGHT * FLIGHT_LENS_HEIGHT_FACTOR;
        assert!(
            (1.165..=1.175).contains(&(width / TAB_WIDTH)),
            "the undeformed optic must reach the target width after physical flight stretch: {width}"
        );
        assert!(
            (0.98..=1.02).contains(&(height / BAR_HEIGHT)),
            "the moving optic shares the target bar's 64dp vertical footprint: {height}"
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
    fn flight_lens_couples_principal_profiles_into_rounded_side_lobes() {
        let coupling = flight_surface_profile().axis_coupling();
        assert!(
            (0.70..=0.85).contains(&coupling),
            "the target's rounded side lobes require both principal profiles off-axis; full toric isolation leaves upright icon columns, got {coupling}"
        );
    }

    #[test]
    fn flight_lens_uses_clear_target_range_optics() {
        let accent = cranpose_ui_graphics::Color::rgb(0.0, 0.48, 1.0);
        let glass = tab_flight_lens_material(cranpose_ui_graphics::Color::BLACK, accent);
        assert!(glass
            .lift
            .is_some_and(|lift| (-0.02..=0.02).contains(&lift)));
        assert!((1.1..=1.3).contains(&glass.chromatic_aberration));
        assert_eq!(glass.blur_radius, Some(2.0));
        assert!((0.28..=0.36).contains(&glass.highlight));
        assert!((52.0..=58.0).contains(&glass.displacement));
        assert!((29.0..=31.0).contains(&glass.surface_profile.depth()));
        assert_eq!(glass.surface_profile.radial_power(), 1.5);
        assert_eq!(glass.surface_profile.axis_coupling(), 0.78);
        assert!(
            glass.shadow,
            "the moving lens needs its target-visible SDF contact outline"
        );
        assert!(glass
            .tint
            .is_some_and(|tint| { tint.r() < 0.05 && (0.075..=0.085).contains(&tint.a()) }));
        assert!(glass
            .sheen
            .is_some_and(|sheen| (0.08..=0.16).contains(&sheen)));
        assert!(glass.content_recolor.is_some_and(|(color, strength)| {
            color == accent && (0.95..=1.0).contains(&strength)
        }));
        assert!(
            (-9.0..=-5.0).contains(
                &(glass.surface_profile.y_profile().evaluate(0.94).1
                    * glass.surface_profile.depth())
            ),
            "the short-axis outer return must form the target's deep crown ridge"
        );
        let depth = glass.surface_profile.depth();
        let x_return_inner = glass.surface_profile.x_profile().evaluate(0.453).1 * depth;
        let x_return_mid = glass.surface_profile.x_profile().evaluate(0.705).1 * depth;
        let y_inner_shoulder = glass.surface_profile.y_profile().evaluate(0.22).1 * depth;
        let y_shoulder = glass.surface_profile.y_profile().evaluate(0.65).1 * depth;
        let y_outer = glass.surface_profile.y_profile().evaluate(0.90).1 * depth;
        assert!(
            (-19.0..=-15.0).contains(&x_return_inner)
                && (-15.0..=-11.0).contains(&x_return_mid)
                && y_inner_shoulder.abs() <= 0.5
                && y_shoulder > 0.0
                && (-9.0..=-5.0).contains(&y_outer),
            "the long-axis return must allocate broad side lobes while the short axis magnifies through its inner shoulder and keeps its crown return: x=({x_return_inner}, {x_return_mid}), y=({y_inner_shoulder}, {y_shoulder}, {y_outer})"
        );
        assert_eq!(
            glass
                .surface_profile
                .y_profile()
                .knots()
                .last()
                .map(|knot| knot.height()),
            Some(0.51)
        );
        assert_eq!(glass.surface_profile, flight_surface_profile());
        let curve = glass.surface_profile.x_profile();
        let epsilon = 0.001;
        let center_curvature = (curve.evaluate(epsilon).1 - curve.evaluate(0.0).1) / epsilon;
        let half_width = TAB_WIDTH * FLIGHT_LENS_WIDTH_FACTOR * 0.5;
        let bend = 1.0 - 1.0 / 1.5;
        let active_optical_strength = 0.18 + 0.67;
        let source_scale = 1.0
            - center_curvature
                * glass.surface_profile.depth()
                * bend
                * glass.displacement
                * active_optical_strength
                / (half_width * half_width);
        let magnification = 1.0 / source_scale;
        assert!(
            (1.20..=1.35).contains(&magnification),
            "the authored surface must physically magnify the tab content, got {magnification}x"
        );
        let moving_depth = glass.surface_profile.depth() * (1.0 + tab_flight_depth_boost(1.0, 1.0));
        let flight_strain = 1.07;
        let moving_half_width = half_width * flight_strain;
        let moving_source_scale = 1.0
            - center_curvature * moving_depth * bend * glass.displacement * active_optical_strength
                / (moving_half_width * moving_half_width);
        let moving_magnification = 1.0 / moving_source_scale;
        assert!(
            (1.15..=1.35).contains(&moving_magnification),
            "the fully resolved moving optic must stay in the measured target range, got {moving_magnification}x"
        );
        let horizontal_mapping = |normalized_x: f32| {
            normalized_x
                - curve.evaluate(normalized_x).1
                    * moving_depth
                    * bend
                    * glass.displacement
                    * active_optical_strength
                    / (moving_half_width * moving_half_width)
        };
        assert!(
            (0.58..=0.61).contains(&horizontal_mapping(0.466))
                && (0.79..=0.82).contains(&horizontal_mapping(0.705)),
            "the long-axis return must map the source badge into the target lobe bounds: inner={}, mid={}",
            horizontal_mapping(0.466),
            horizontal_mapping(0.705)
        );
        for step in 1..=49 {
            let normalized_x = step as f32 * 0.02;
            let source_scale = (horizontal_mapping(normalized_x + epsilon)
                - horizontal_mapping(normalized_x - epsilon))
                / (2.0 * epsilon);
            assert!(
                source_scale > 0.05,
                "the full target lens must remain a single monotonic image at x={normalized_x}, got {source_scale}"
            );
        }
        let vertical_mapping = |normalized_y: f32| {
            let gradient = glass
                .surface_profile
                .sample_normalized((0.68, normalized_y))
                .gradient
                .1;
            let half_height = BAR_HEIGHT * FLIGHT_LENS_HEIGHT_FACTOR * 0.5;
            let normalized_gain =
                moving_depth * glass.displacement * bend * active_optical_strength
                    / (half_height * half_height);
            normalized_y - gradient * normalized_gain
        };
        let vertical_source_scale =
            (vertical_mapping(0.22 + epsilon) - vertical_mapping(0.22 - epsilon)) / (2.0 * epsilon);
        assert!(
            (0.95..=1.05).contains(&vertical_source_scale),
            "the side-band vertical Jacobian must preserve the target glyph's native proportions, got {vertical_source_scale}"
        );
        assert!(
            (0.49..=0.51).contains(&vertical_mapping(0.58)),
            "the outer Y-Z zone must expand the source badge into the target lobe height, got {}",
            vertical_mapping(0.58)
        );
        for step in 1..=35 {
            let normalized_y = step as f32 * 0.02;
            let source_scale = (vertical_mapping(normalized_y + epsilon)
                - vertical_mapping(normalized_y - epsilon))
                / (2.0 * epsilon);
            assert!(
                source_scale > 0.05,
                "the visible biconic interior must stay fold-free at y={normalized_y}, got {source_scale}"
            );
        }
        assert_eq!(tab_flight_depth_boost(0.0, 1.0), 0.0);
        let still = tab_flight_depth_boost(1.0, 0.0);
        let cruise = tab_flight_depth_boost(1.0, 1.0);
        assert_eq!(still, 0.0);
        assert!((0.11..=0.13).contains(&cruise));
    }

    #[test]
    fn flight_lens_retains_neutral_tint_through_direct_motion() {
        assert_eq!(tab_flight_tint_multiplier(0.0), 1.0);
        assert!((tab_flight_tint_multiplier(1.0) - 0.4).abs() < f32::EPSILON);
        assert_eq!(tab_flight_tint_multiplier(-1.0), 1.0);
        assert!((tab_flight_tint_multiplier(2.0) - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn bar_surface_frosts_color_inside_a_bright_folded_body() {
        let glass = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK);
        assert_eq!(glass.blur_radius, Some(16.0));
        assert_eq!(glass.saturation, Some(0.95));
        assert_eq!(glass.lift, Some(0.48));
        assert_eq!(glass.edge_fold, 1.0);
        assert!((2.8..=3.2).contains(&glass.surface_profile.depth()));
    }

    #[test]
    fn bar_surface_tint_separates_from_same_polarity_backdrops() {
        let light_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK)
            .tint
            .expect("bar tint");
        assert!(light_surface.r() < 0.05);
        assert!((0.02..=0.04).contains(&light_surface.a()));

        let dark_surface = tab_bar_surface_material(cranpose_ui_graphics::Color::WHITE)
            .tint
            .expect("bar tint");
        assert!(dark_surface.r() > 0.95);
        assert!((0.03..=0.05).contains(&dark_surface.a()));
    }

    #[test]
    fn arrival_contraction_uses_the_measured_fast_settle() {
        let cranpose_animation::AnimationType::Spring(settle) = tab_lens_activity_motion(false)
        else {
            panic!("arrival contraction must use a spring");
        };
        assert_eq!(settle.damping_ratio, 1.0);
        assert!((1300.0..=1500.0).contains(&settle.stiffness));
    }

    #[test]
    fn launch_inertia_drives_one_trailing_perimeter_lobe() {
        let launch = crate::dynamics::LiquidPose {
            stretch: 0.84,
            ortho: 1.0 / 0.84,
            axis: (-1.0, 0.0),
            ..Default::default()
        };
        let (amplitude, direction) = tab_flight_launch_wake(launch, 72.0)
            .expect("accelerating fluid needs a trailing inertia lobe");
        assert!((3.0..=7.0).contains(&amplitude));
        assert!(
            direction.abs() < 1e-4,
            "leftward flight must trail to the right"
        );

        let cruise = crate::dynamics::LiquidPose {
            stretch: 1.02,
            ortho: 1.0 / 1.02,
            axis: (-1.0, 0.0),
            ..Default::default()
        };
        assert!(tab_flight_launch_wake(cruise, 72.0).is_none());
    }
}
