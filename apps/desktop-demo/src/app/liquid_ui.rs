//! Liquid UI showcase: the full glass component set over colorful scrolling
//! content — buttons, toggle, slider, segmented control, chips, list cards,
//! the floating tab bar with its liquid selection blob, the morphing menu,
//! and a materials lab with live lens parameters.

#![allow(non_snake_case)]

use cranpose::liquid::material::GlassMorph;
use cranpose::liquid::prelude::*;
use cranpose::text::{SpanStyle, TextStyle, TextUnit};
use cranpose::widgets::{
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Column, ColumnSpec, Row, RowSpec,
    Text,
};
use cranpose::{
    composable, mutableStateOf, remember, Brush, Color, CornerRadii, GraphicsLayer, Modifier,
    Point, Rect, ScrollState, Size,
};
use cranpose_animation::{animateFloatAsState, spring};
use cranpose_core::MutableState;
use cranpose_foundation::{PointerEventKind, PointerId};
use cranpose_ui::{Alignment, HorizontalAlignment, PointerInputScope, VerticalAlignment};
use std::cell::Cell;
use std::rc::Rc;

const PAGE_PADDING: f32 = 18.0;
const PROFILE_KNOT_COUNT: usize = MAX_GLASS_PROFILE_KNOTS;
const PROFILE_POSITION_GAP: f32 = 0.02;
const PROFILE_PLOT_HEIGHT: f32 = 190.0;
const PROFILE_STAGE_HEIGHT: f32 = 190.0;
const PROFILE_DEMO_WIDTH: f32 = 156.0;
const PROFILE_DEMO_HEIGHT: f32 = 84.0;
const PROFILE_DEMO_NODE_WIDTH: f32 = 224.0;
const PROFILE_DEMO_NODE_HEIGHT: f32 = 150.0;
const PROFILE_DISPERSION_AXIS_MAX: f32 = 2.5;
const PROFILE_WIDE_WORKSPACE_MIN_WIDTH: f32 = 680.0;
const PROFILE_WORKSPACE_GAP: f32 = 10.0;
const PROFILE_STAGE_TEXT: &str = "PROFILE 0123456789 | glass over text";
const LIQUID_TAB_BAR_BOTTOM_PADDING: f32 = 19.0;

type ProfilePoints = [(f32, f32); PROFILE_KNOT_COUNT];

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabSwipeReferenceLayout {
    size: Size,
    title: Rect,
    enroll: Rect,
    xcode: Rect,
}

fn tab_swipe_reference_layout() -> TabSwipeReferenceLayout {
    TabSwipeReferenceLayout {
        size: Size::new(440.0, 132.0),
        title: Rect {
            x: 40.0,
            y: 0.0,
            width: 360.0,
            height: 29.0,
        },
        enroll: Rect {
            x: 20.0,
            y: 33.0,
            width: 400.0,
            height: 51.0,
        },
        xcode: Rect {
            x: 32.0,
            y: 69.0,
            width: 376.0,
            height: 50.0,
        },
    }
}

fn tab_swipe_reference_glass() -> Glass {
    Glass::regular().surface_profile(
        GlassSurfaceProfile::regular()
            .with_depth(3.0)
            .expect("tab-swipe reference row depth is valid"),
    )
}

#[derive(Clone, Copy, PartialEq)]
struct GlassProfileEditorState {
    axis: MutableState<usize>,
    knot: MutableState<usize>,
    x_points: MutableState<ProfilePoints>,
    y_points: MutableState<ProfilePoints>,
    depth: MutableState<f32>,
    radial_power: MutableState<f32>,
    axis_coupling: MutableState<f32>,
    aberration: MutableState<f32>,
    dispersion_x: MutableState<f32>,
    dispersion_y: MutableState<f32>,
    frost: MutableState<f32>,
    drag_center: MutableState<(f32, f32)>,
    dragging: MutableState<bool>,
}

fn profile_points(curve: GlassProfileCurve) -> ProfilePoints {
    let knots = curve.knots();
    assert_eq!(
        knots.len(),
        PROFILE_KNOT_COUNT,
        "the profile editor requires the production knot budget"
    );
    let mut points = [(0.0, 0.0); PROFILE_KNOT_COUNT];
    for (point, knot) in points.iter_mut().zip(knots) {
        *point = (knot.position(), knot.height());
    }
    points
}

fn profile_curve(points: ProfilePoints) -> GlassProfileCurve {
    GlassProfileCurve::from_points(&points).expect("editor preserves normalized profile ordering")
}

fn update_profile_position(
    mut points: ProfilePoints,
    index: usize,
    position: f32,
) -> ProfilePoints {
    if index == 0 || index + 1 >= PROFILE_KNOT_COUNT {
        return points;
    }
    let lower = points[index - 1].0 + PROFILE_POSITION_GAP;
    let upper = points[index + 1].0 - PROFILE_POSITION_GAP;
    points[index].0 = position.clamp(lower, upper);
    points
}

fn update_profile_height(mut points: ProfilePoints, index: usize, height: f32) -> ProfilePoints {
    points[index.min(PROFILE_KNOT_COUNT - 1)].1 = height.clamp(0.0, 1.0);
    points
}

fn editor_surface_profile(editor: GlassProfileEditorState) -> GlassSurfaceProfile {
    let x = profile_curve(editor.x_points.get());
    let y = profile_curve(editor.y_points.get());
    GlassSurfaceProfile::new(
        x,
        y,
        editor.depth.get().clamp(0.0, 1.0) * 12.0,
        1.5 + editor.radial_power.get().clamp(0.0, 1.0) * 6.5,
    )
    .and_then(|profile| profile.with_axis_coupling(editor.axis_coupling.get().clamp(0.0, 1.0)))
    .expect("editor keeps both physical cross-sections coherent")
}

fn profile_dispersion_axis(value: f32) -> f32 {
    value.clamp(0.0, 1.0) * PROFILE_DISPERSION_AXIS_MAX
}

fn profile_uses_wide_workspace(width: f32) -> bool {
    width >= PROFILE_WIDE_WORKSPACE_MIN_WIDTH
}

fn profile_wide_pane_width(width: f32) -> f32 {
    ((width - PROFILE_WORKSPACE_GAP) * 0.5).max(1.0)
}

fn profile_drag_center(
    pointer: Point,
    grab_offset: Point,
    stage_width: f32,
    stage_height: f32,
) -> (f32, f32) {
    let x = (pointer.x - grab_offset.x).clamp(
        PROFILE_DEMO_WIDTH * 0.5,
        stage_width - PROFILE_DEMO_WIDTH * 0.5,
    );
    let y = (pointer.y - grab_offset.y).clamp(
        PROFILE_DEMO_HEIGHT * 0.5,
        stage_height - PROFILE_DEMO_HEIGHT * 0.5,
    );
    (x / stage_width, y / stage_height)
}

fn profile_stage_text_rows(stage_height: f32) -> usize {
    const ROW_HEIGHT: f32 = 31.0;
    const VERTICAL_PADDING: f32 = 20.0;
    (((stage_height - VERTICAL_PADDING).max(ROW_HEIGHT) / ROW_HEIGHT).floor() as usize).max(1)
}

fn heading_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(15.0),
            font_weight: Some(cranpose::text::FontWeight::SEMI_BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn body_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[composable]
fn ProfileSlider(
    label: &'static str,
    value: f32,
    value_label: String,
    on_change: impl Fn(f32) + 'static,
) {
    let colors = liquid_colors();
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                label,
                Modifier::empty().weight(1.0),
                body_style(colors.secondary_label),
            );
            Text(
                value_label.clone(),
                Modifier::empty(),
                body_style(colors.label),
            );
        },
    );
    LiquidSlider(
        Modifier::empty().fill_max_width(),
        value.clamp(0.0, 1.0),
        on_change,
    );
}

#[composable]
fn ProfileCurvePlot(modifier: Modifier, editor: GlassProfileEditorState) {
    let colors = liquid_colors();
    let axis = editor.axis.get().min(1);
    let selected = editor.knot.get().min(PROFILE_KNOT_COUNT - 1);
    let x_curve = profile_curve(editor.x_points.get());
    let y_curve = profile_curve(editor.y_points.get());
    let accent = colors.accent;
    let inactive = colors.secondary_label.with_alpha(0.42);
    let grid = colors.separator.with_alpha(0.55);
    let fill = colors
        .fill
        .with_alpha(if colors.is_dark { 0.60 } else { 0.80 });

    Box(
        modifier
            .height(PROFILE_PLOT_HEIGHT)
            .semantics(|config| {
                config.content_description = Some("Glass profile curves".to_string());
            })
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(fill), CornerRadii::uniform(8.0));
                let padding = 18.0;
                let width = (scope.size().width - padding * 2.0).max(1.0);
                let height = (scope.size().height - padding * 2.0).max(1.0);
                for step in 0..=4 {
                    let fraction = step as f32 / 4.0;
                    scope.draw_rect_at(
                        Rect {
                            x: padding + width * fraction,
                            y: padding,
                            width: 1.0,
                            height,
                        },
                        Brush::solid(grid),
                    );
                    scope.draw_rect_at(
                        Rect {
                            x: padding,
                            y: padding + height * fraction,
                            width,
                            height: 1.0,
                        },
                        Brush::solid(grid),
                    );
                }

                for (curve, color, radius) in [
                    (if axis == 0 { y_curve } else { x_curve }, inactive, 1.25),
                    (if axis == 0 { x_curve } else { y_curve }, accent, 1.65),
                ] {
                    for step in 0..=160 {
                        let position = step as f32 / 160.0;
                        let height_value = curve.evaluate(position).0;
                        scope.draw_circle(
                            Brush::solid(color),
                            Point::new(
                                padding + position * width,
                                padding + (1.0 - height_value) * height,
                            ),
                            radius,
                        );
                    }
                }

                let selected_curve = if axis == 0 { x_curve } else { y_curve };
                let selected_knot = selected_curve.knots()[selected];
                let tangent_start = (selected_knot.position() - 0.09).max(0.0);
                let tangent_end = (selected_knot.position() + 0.09).min(1.0);
                for step in 0..=40 {
                    let t = step as f32 / 40.0;
                    let position = tangent_start + (tangent_end - tangent_start) * t;
                    let tangent_height = selected_knot.height()
                        + selected_knot.tangent() * (position - selected_knot.position());
                    if (0.0..=1.0).contains(&tangent_height) {
                        scope.draw_circle(
                            Brush::solid(accent.with_alpha(0.38)),
                            Point::new(
                                padding + position * width,
                                padding + (1.0 - tangent_height) * height,
                            ),
                            1.0,
                        );
                    }
                }
                for (index, knot) in selected_curve.knots().iter().enumerate() {
                    scope.draw_circle(
                        Brush::solid(if index == selected {
                            colors.label
                        } else {
                            accent
                        }),
                        Point::new(
                            padding + knot.position() * width,
                            padding + (1.0 - knot.height()) * height,
                        ),
                        if index == selected { 5.5 } else { 3.5 },
                    );
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn ProfileGlassStage(modifier: Modifier, editor: GlassProfileEditorState) {
    let active_pointer = remember(|| Rc::new(Cell::new(None::<PointerId>))).with(Rc::clone);
    let grab_offset = remember(|| Rc::new(Cell::new(Point::new(0.0, 0.0)))).with(Rc::clone);

    BoxWithConstraints(
        modifier.height(PROFILE_STAGE_HEIGHT).semantics(|config| {
            config.content_description = Some("Glass profile live stage".to_string());
        }),
        move |scope| {
            let stage_width = scope.constraints().max_width.max(PROFILE_DEMO_WIDTH);
            let stage_height = PROFILE_STAGE_HEIGHT;
            let pointer = Rc::clone(&active_pointer);
            let offset = Rc::clone(&grab_offset);
            let drag_surface = Modifier::empty()
                .size(Size::new(stage_width, stage_height))
                .pointer_input((stage_width.to_bits(), stage_height.to_bits()), {
                    move |input: PointerInputScope| {
                        let pointer = Rc::clone(&pointer);
                        let offset = Rc::clone(&offset);
                        async move {
                            input
                                .await_pointer_event_scope(|events| async move {
                                    loop {
                                        let event = events.await_pointer_event().await;
                                        match event.kind {
                                            PointerEventKind::Down if pointer.get().is_none() => {
                                                let center = editor.drag_center.get();
                                                let center = Point::new(
                                                    center.0 * stage_width,
                                                    center.1 * stage_height,
                                                );
                                                let inside = (event.position.x - center.x).abs()
                                                    <= PROFILE_DEMO_WIDTH * 0.5
                                                    && (event.position.y - center.y).abs()
                                                        <= PROFILE_DEMO_HEIGHT * 0.5;
                                                if inside {
                                                    pointer.set(Some(event.id));
                                                    offset.set(Point::new(
                                                        event.position.x - center.x,
                                                        event.position.y - center.y,
                                                    ));
                                                    editor.dragging.set(true);
                                                    event.consume();
                                                }
                                            }
                                            PointerEventKind::Move
                                                if pointer.get() == Some(event.id) =>
                                            {
                                                let offset = offset.get();
                                                editor.drag_center.set(profile_drag_center(
                                                    event.position,
                                                    offset,
                                                    stage_width,
                                                    stage_height,
                                                ));
                                                event.consume();
                                            }
                                            PointerEventKind::Up | PointerEventKind::Cancel
                                                if pointer.get() == Some(event.id) =>
                                            {
                                                pointer.set(None);
                                                editor.dragging.set(false);
                                                event.consume();
                                            }
                                            _ => {}
                                        }
                                    }
                                })
                                .await;
                        }
                    }
                })
                .draw_behind(move |scope| {
                    let width = scope.size().width;
                    let height = scope.size().height;
                    scope.draw_round_rect(
                        Brush::solid(Color::from_rgb_u8(246, 247, 250)),
                        CornerRadii::uniform(8.0),
                    );
                    let third = width / 3.0;
                    for row in 0..6 {
                        for column in 0..6 {
                            scope.draw_rect_at(
                                Rect {
                                    x: column as f32 * third / 6.0,
                                    y: row as f32 * height / 6.0,
                                    width: third / 6.0 + 0.5,
                                    height: height / 6.0 + 0.5,
                                },
                                Brush::solid(if (row + column) % 2 == 0 {
                                    Color::WHITE
                                } else {
                                    Color::from_rgb_u8(18, 18, 20)
                                }),
                            );
                        }
                    }
                    scope.draw_rect_at(
                        Rect {
                            x: third,
                            y: 0.0,
                            width: third,
                            height,
                        },
                        Brush::linear_gradient_range(
                            vec![
                                Color::from_rgb_u8(52, 199, 89),
                                Color::from_rgb_u8(255, 45, 85),
                            ],
                            Point::new(third, 0.0),
                            Point::new(third * 2.0, height),
                        ),
                    );
                    scope.draw_rect_at(
                        Rect {
                            x: third * 2.0,
                            y: 0.0,
                            width: third,
                            height,
                        },
                        Brush::linear_gradient_range(
                            vec![
                                Color::from_rgb_u8(10, 132, 255),
                                Color::from_rgb_u8(175, 82, 222),
                            ],
                            Point::new(third * 2.0, 0.0),
                            Point::new(width, height),
                        ),
                    );
                });
            Box(drag_surface, BoxSpec::default(), || {});

            Column(
                Modifier::empty().padding_each(8.0, 10.0, 8.0, 10.0),
                ColumnSpec::default(),
                move || {
                    for _ in 0..profile_stage_text_rows(stage_height) {
                        Text(
                            PROFILE_STAGE_TEXT,
                            Modifier::empty(),
                            body_style(Color::from_rgba_u8(12, 12, 15, 190)),
                        );
                    }
                },
            );

            let profile = editor_surface_profile(editor);
            let dynamics = remember_liquid_dynamics();
            let motion_clock = animateFloatAsState(
                if editor.dragging.get() { 1.0 } else { 0.0 },
                if editor.dragging.get() {
                    spring(0.9, 900.0)
                } else {
                    spring(1.0, 120.0)
                },
                "profile-demo-fluid-clock",
            );
            let center_for_layer = editor.drag_center;
            let center_for_dynamics = editor.drag_center;
            let dynamics_for_layer = Rc::clone(&dynamics);
            let demo = Modifier::empty()
                .required_size(Size::new(PROFILE_DEMO_NODE_WIDTH, PROFILE_DEMO_NODE_HEIGHT))
                .graphics_layer(move || {
                    let center = center_for_layer.get();
                    GraphicsLayer {
                        translation_x: center.0 * stage_width - PROFILE_DEMO_NODE_WIDTH * 0.5,
                        translation_y: center.1 * stage_height - PROFILE_DEMO_NODE_HEIGHT * 0.5,
                        ..Default::default()
                    }
                })
                .glass_effect_with(
                    Glass::lens()
                        .surface_profile(profile)
                        .blur_radius(editor.frost.get().clamp(0.0, 1.0) * 18.0)
                        .chromatic_aberration(editor.aberration.get().clamp(0.0, 1.0) * 6.0)
                        .dispersion_axes(
                            profile_dispersion_axis(editor.dispersion_x.get()),
                            profile_dispersion_axis(editor.dispersion_y.get()),
                        )
                        .displacement(26.0)
                        .highlight(0.42)
                        .sheen(0.10)
                        .no_clip(),
                    move || {
                        let _ = motion_clock.get();
                        let center = center_for_dynamics.get();
                        let pose = dynamics_for_layer
                            .update((center.0 * stage_width, center.1 * stage_height));
                        let energy = pose.energy();
                        GlassDynamics {
                            tilt: (pose.axis.0 * energy * 0.14, pose.axis.1 * energy * 0.14),
                            surface_depth_boost: 0.12 * energy,
                            morph: Some(GlassMorph {
                                node_size: (PROFILE_DEMO_NODE_WIDTH, PROFILE_DEMO_NODE_HEIGHT),
                                primary: (
                                    PROFILE_DEMO_NODE_WIDTH * 0.5,
                                    PROFILE_DEMO_NODE_HEIGHT * 0.5,
                                    PROFILE_DEMO_WIDTH,
                                    PROFILE_DEMO_HEIGHT,
                                    -1.0,
                                ),
                                shapes: Vec::new(),
                                glue: 0.0,
                                wobble_amplitude: 0.0,
                                wobble_phase: 0.0,
                                bulge_amplitude: pose.bulge_amplitude.min(6.0),
                                bulge_direction: pose.bulge_direction,
                                ellipse_blend: 0.0,
                                deformation: Some(pose.deformation()),
                            }),
                            ..Default::default()
                        }
                    },
                );
            Box(demo, BoxSpec::default(), || {});
        },
    );
}

#[composable]
fn ProfilePointControls(modifier: Modifier, editor: GlassProfileEditorState) {
    Column(modifier, ColumnSpec::default(), move || {
        let axis_index = editor.axis.get().min(1);
        let knot_index = editor.knot.get().min(PROFILE_KNOT_COUNT - 1);
        let active_points = if axis_index == 0 {
            editor.x_points
        } else {
            editor.y_points
        };
        let point = active_points.get()[knot_index];
        if knot_index > 0 && knot_index + 1 < PROFILE_KNOT_COUNT {
            ProfileSlider(
                "Position",
                point.0,
                format!("{:.2}", point.0),
                move |value| {
                    active_points.set(update_profile_position(
                        active_points.get(),
                        knot_index,
                        value,
                    ));
                },
            );
        }
        let x_points = editor.x_points;
        let y_points = editor.y_points;
        ProfileSlider("Height", point.1, format!("{:.2}", point.1), move |value| {
            active_points.set(update_profile_height(
                active_points.get(),
                knot_index,
                value,
            ));
            if knot_index == 0 {
                x_points.set(update_profile_height(x_points.get(), 0, value));
                y_points.set(update_profile_height(y_points.get(), 0, value));
            }
        });
    });
}

#[composable]
fn ProfileShapeControls(modifier: Modifier, editor: GlassProfileEditorState) {
    Column(modifier, ColumnSpec::default(), move || {
        let depth = editor.depth;
        ProfileSlider(
            "Depth",
            depth.get(),
            format!("{:.1} dp", depth.get() * 12.0),
            move |value| depth.set(value),
        );
        let radial_power = editor.radial_power;
        ProfileSlider(
            "Radial",
            radial_power.get(),
            format!("{:.2}", 1.5 + radial_power.get() * 6.5),
            move |value| radial_power.set(value),
        );
        let axis_coupling = editor.axis_coupling;
        ProfileSlider(
            "Coupling",
            axis_coupling.get(),
            format!("{:.2}", axis_coupling.get()),
            move |value| axis_coupling.set(value),
        );
    });
}

#[composable]
fn ProfileOpticControls(modifier: Modifier, editor: GlassProfileEditorState) {
    Column(modifier, ColumnSpec::default(), move || {
        let aberration = editor.aberration;
        ProfileSlider(
            "Aberration",
            aberration.get(),
            format!("{:.2}", aberration.get() * 6.0),
            move |value| aberration.set(value),
        );
        let frost = editor.frost;
        ProfileSlider(
            "Frost",
            frost.get(),
            format!("{:.1} dp", frost.get() * 18.0),
            move |value| frost.set(value),
        );
    });
}

#[composable]
fn ProfileDispersionControls(modifier: Modifier, editor: GlassProfileEditorState) {
    Column(modifier, ColumnSpec::default(), move || {
        let dispersion_x = editor.dispersion_x;
        ProfileSlider(
            "Dispersion X",
            dispersion_x.get(),
            format!("{:.2}", profile_dispersion_axis(dispersion_x.get())),
            move |value| dispersion_x.set(value),
        );
        let dispersion_y = editor.dispersion_y;
        ProfileSlider(
            "Dispersion Y",
            dispersion_y.get(),
            format!("{:.2}", profile_dispersion_axis(dispersion_y.get())),
            move |value| dispersion_y.set(value),
        );
    });
}

#[composable]
fn ProfileSelectors(editor: GlassProfileEditorState, wide: bool) {
    let axis = editor.axis;
    let knot = editor.knot;
    if wide {
        Row(
            Modifier::empty().fill_max_width(),
            RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
            move || {
                LiquidSegmentedControl(
                    Modifier::empty().weight(1.0),
                    vec!["X-Z".to_string(), "Y-Z".to_string()],
                    axis.get().min(1),
                    move |value| axis.set(value.min(1)),
                );
                Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
                LiquidSegmentedControl(
                    Modifier::empty().weight(1.5),
                    (0..PROFILE_KNOT_COUNT)
                        .map(|index| index.to_string())
                        .collect(),
                    knot.get().min(PROFILE_KNOT_COUNT - 1),
                    move |value| knot.set(value.min(PROFILE_KNOT_COUNT - 1)),
                );
            },
        );
    } else {
        LiquidSegmentedControl(
            Modifier::empty().fill_max_width(),
            vec!["X-Z".to_string(), "Y-Z".to_string()],
            axis.get().min(1),
            move |value| axis.set(value.min(1)),
        );
        Box(Modifier::empty().height(8.0), BoxSpec::default(), || {});
        LiquidSegmentedControl(
            Modifier::empty().fill_max_width(),
            (0..PROFILE_KNOT_COUNT)
                .map(|index| index.to_string())
                .collect(),
            knot.get().min(PROFILE_KNOT_COUNT - 1),
            move |value| knot.set(value.min(PROFILE_KNOT_COUNT - 1)),
        );
    }
}

#[composable]
fn ProfileControlGrid(editor: GlassProfileEditorState, wide: bool) {
    if wide {
        Row(
            Modifier::empty().fill_max_width(),
            RowSpec::default(),
            move || {
                ProfilePointControls(Modifier::empty().weight(1.0), editor);
                Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
                ProfileShapeControls(Modifier::empty().weight(1.0), editor);
                Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
                ProfileOpticControls(Modifier::empty().weight(1.0), editor);
                Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
                ProfileDispersionControls(Modifier::empty().weight(1.0), editor);
            },
        );
    } else {
        ProfilePointControls(Modifier::empty().fill_max_width(), editor);
        ProfileShapeControls(Modifier::empty().fill_max_width(), editor);
        ProfileOpticControls(Modifier::empty().fill_max_width(), editor);
        ProfileDispersionControls(Modifier::empty().fill_max_width(), editor);
    }
}

#[composable]
fn GlassProfilePlayground(editor: GlassProfileEditorState) {
    BoxWithConstraints(Modifier::empty().fill_max_width(), move |scope| {
        let workspace_width = scope.constraints().max_width;
        let wide = profile_uses_wide_workspace(workspace_width);
        let pane_width = profile_wide_pane_width(workspace_width);
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || {
                if wide {
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::default(),
                        move || {
                            ProfileGlassStage(Modifier::empty().width(pane_width), editor);
                            Box(
                                Modifier::empty().width(PROFILE_WORKSPACE_GAP),
                                BoxSpec::default(),
                                || {},
                            );
                            ProfileCurvePlot(Modifier::empty().width(pane_width), editor);
                        },
                    );
                } else {
                    ProfileGlassStage(Modifier::empty().fill_max_width(), editor);
                    Box(Modifier::empty().height(10.0), BoxSpec::default(), || {});
                    ProfileCurvePlot(Modifier::empty().fill_max_width(), editor);
                }
                Box(Modifier::empty().height(10.0), BoxSpec::default(), || {});

                let colors = liquid_colors();
                Column(
                    Modifier::empty()
                        .fill_max_width()
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(colors.fill.with_alpha(if colors.is_dark {
                                    0.60
                                } else {
                                    0.80
                                })),
                                CornerRadii::uniform(8.0),
                            );
                        })
                        .padding(12.0),
                    ColumnSpec::default(),
                    move || {
                        ProfileSelectors(editor, wide);
                        Box(Modifier::empty().height(8.0), BoxSpec::default(), || {});
                        ProfileControlGrid(editor, wide);
                    },
                );
            },
        );
    });
}

/// A vivid gradient block — the kind of backdrop glass is made for.
#[composable]
fn GradientStripe(colors: [Color; 2], height: f32) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(height)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect(
                    Brush::linear_gradient_range(
                        vec![colors[0], colors[1]],
                        Point::new(0.0, 0.0),
                        Point::new(size.width, size.height),
                    ),
                    CornerRadii::uniform(22.0),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn TabSwipeReferenceBackdrop() {
    let layout = tab_swipe_reference_layout();
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(layout.size.height),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::CenterHorizontally,
            VerticalAlignment::Top,
        )),
        move || {
            Box(
                Modifier::empty()
                    .size(layout.size)
                    .offset(4.0, 0.0)
                    .draw_behind(|scope| {
                        scope.draw_rect(Brush::solid(Color::from_rgb_u8(245, 245, 251)));
                    }),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .size(Size::new(layout.xcode.width, layout.xcode.height))
                            .offset(layout.xcode.x, layout.xcode.y)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color::from_rgb_u8(250, 250, 252)),
                                    CornerRadii::uniform(18.0),
                                );
                            }),
                        BoxSpec::default(),
                        || {
                            Text(
                                "Xcode",
                                Modifier::empty().offset(30.0, 8.0),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(Color::from_rgb_u8(208, 208, 214)),
                                        font_size: TextUnit::Sp(18.0),
                                        font_weight: Some(cranpose::text::FontWeight::BOLD),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );
                        },
                    );
                    GlassSurface(
                        Modifier::empty()
                            .size(Size::new(layout.enroll.width, layout.enroll.height))
                            .offset(layout.enroll.x, layout.enroll.y),
                        tab_swipe_reference_glass(),
                        || {
                            Box(
                                Modifier::empty().fill_max_size(),
                                BoxSpec::default(),
                                || {
                                    Text(
                                        "Enroll Now",
                                        Modifier::empty().offset(20.0, 12.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color::from_rgb_u8(0, 122, 255)),
                                                font_size: TextUnit::Sp(17.0),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                },
                            );
                        },
                    );
                    Text(
                        "Apple Developer Program",
                        Modifier::empty().offset(layout.title.x, layout.title.y),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color::from_rgb_u8(124, 124, 130)),
                                font_size: TextUnit::Sp(17.0),
                                font_weight: Some(cranpose::text::FontWeight::MEDIUM),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn BackdropTile(
    modifier: Modifier,
    label: &'static str,
    label_color: Color,
    start: Color,
    end: Color,
    content: impl FnMut() + 'static,
) {
    Box(
        modifier.draw_behind(move |scope| {
            scope.draw_round_rect(
                Brush::linear_gradient_range(
                    vec![start, end],
                    Point::new(0.0, 0.0),
                    Point::new(0.0, scope.size().height),
                ),
                CornerRadii::uniform(16.0),
            );
        }),
        BoxSpec::default(),
        move || {
            Text(
                label,
                Modifier::empty().offset(12.0, 76.0),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(label_color),
                        font_size: TextUnit::Sp(24.0),
                        font_weight: Some(cranpose::text::FontWeight::BOLD),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            content();
        },
    );
}

#[composable]
fn CalculatorBackdropGraphic() {
    Box(
        Modifier::empty()
            .size(Size::new(52.0, 64.0))
            .offset(108.0, 3.0)
            .graphics_layer(|| GraphicsLayer {
                rotation_z: 5.0,
                ..Default::default()
            })
            .drop_shadow(
                cranpose_ui_graphics::LayerShape::Rounded(
                    cranpose_ui_graphics::RoundedCornerShape::uniform(7.0),
                ),
                |scope| {
                    scope.radius = 8.0;
                    scope.offset.y = 4.0;
                    scope.color = Color::BLACK.with_alpha(0.18);
                },
            )
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::linear_gradient_range(
                        vec![
                            Color::from_rgb_u8(226, 228, 220),
                            Color::from_rgb_u8(173, 178, 171),
                        ],
                        Point::new(0.0, 0.0),
                        Point::new(scope.size().width, scope.size().height),
                    ),
                    CornerRadii::uniform(7.0),
                );
                scope.draw_rect_at(
                    Rect {
                        x: 6.0,
                        y: 7.0,
                        width: 40.0,
                        height: 15.0,
                    },
                    Brush::solid(Color::from_rgb_u8(86, 103, 73)),
                );
                for row in 0..3 {
                    for column in 0..4 {
                        scope.draw_circle(
                            Brush::solid(if column == 3 {
                                Color::from_rgb_u8(247, 191, 48)
                            } else {
                                Color::from_rgb_u8(112, 116, 111)
                            }),
                            Point::new(10.0 + column as f32 * 10.5, 31.0 + row as f32 * 10.5),
                            3.5,
                        );
                    }
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn SpheresBackdropGraphic() {
    Box(
        Modifier::empty()
            .size(Size::new(92.0, 67.0))
            .offset(92.0, 0.0)
            .draw_behind(|scope| {
                scope.draw_circle(
                    Brush::linear_gradient_range(
                        vec![
                            Color::from_rgb_u8(255, 78, 105),
                            Color::from_rgb_u8(196, 35, 70),
                        ],
                        Point::new(35.0, 2.0),
                        Point::new(50.0, 54.0),
                    ),
                    Point::new(43.0, 28.0),
                    27.0,
                );
                scope.draw_circle(
                    Brush::linear_gradient_range(
                        vec![
                            Color::from_rgb_u8(73, 220, 239),
                            Color::from_rgb_u8(38, 107, 188),
                        ],
                        Point::new(48.0, 18.0),
                        Point::new(76.0, 62.0),
                    ),
                    Point::new(65.0, 43.0),
                    23.0,
                );
                scope.draw_circle(
                    Brush::linear_gradient_range(
                        vec![
                            Color::from_rgb_u8(244, 173, 255),
                            Color::from_rgb_u8(113, 61, 210),
                        ],
                        Point::new(12.0, 32.0),
                        Point::new(42.0, 66.0),
                    ),
                    Point::new(29.0, 49.0),
                    18.0,
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn BottomBarBackdrop() {
    Row(
        Modifier::empty()
            .width(440.0)
            .height(112.0)
            .padding_symmetric(20.0, 0.0),
        RowSpec::default(),
        || {
            BackdropTile(
                Modifier::empty().weight(1.0).fill_max_height(),
                "Outrageous",
                Color::from_rgba_u8(88, 42, 12, 220),
                Color::from_rgb_u8(255, 190, 119),
                Color::from_rgb_u8(221, 119, 31),
                CalculatorBackdropGraphic,
            );
            Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
            BackdropTile(
                Modifier::empty().weight(1.0).fill_max_height(),
                "Custom Networking",
                Color::from_rgba_u8(255, 255, 255, 220),
                Color::from_rgb_u8(173, 154, 248),
                Color::from_rgb_u8(86, 65, 182),
                SpheresBackdropGraphic,
            );
        },
    );
}

#[composable]
fn StoreBottomBarExample(selected: usize, on_select: impl Fn(usize) + 'static) {
    let on_select: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(on_select);
    Box(
        Modifier::empty().width(440.0).height(112.0),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::CenterHorizontally,
            VerticalAlignment::Bottom,
        )),
        {
            let on_select = std::rc::Rc::clone(&on_select);
            move || {
                BottomBarBackdrop();
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .padding_each(0.0, 0.0, 0.0, 10.0),
                    BoxSpec::default().content_alignment(Alignment::new(
                        HorizontalAlignment::CenterHorizontally,
                        VerticalAlignment::Bottom,
                    )),
                    {
                        let on_select = std::rc::Rc::clone(&on_select);
                        move || {
                            let on_select = std::rc::Rc::clone(&on_select);
                            LiquidTabBar(
                                Modifier::empty(),
                                vec![
                                    LiquidTab::new(icons::DOCUMENT, "Today"),
                                    LiquidTab::new(icons::ROCKET, "Games"),
                                    LiquidTab::new(icons::LAYERS, "Apps"),
                                    LiquidTab::new(icons::JOYSTICK, "Arcade"),
                                    LiquidTab::new(icons::SEARCH, "Search"),
                                ],
                                selected,
                                move |index| on_select(index),
                            );
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn SectionTitle(text: &'static str) {
    let colors = liquid_colors();
    Text(
        text,
        Modifier::empty().padding_each(2.0, 18.0, 0.0, 8.0),
        heading_style(colors.secondary_label),
    );
}

/// One WWDC-style session card: dark glyph thumbnail, bold title, gray
/// subtitle — the list the reference floating tab bar hovers over.
#[composable]
fn SessionCard(icon: &'static str, title: &'static str, subtitle: &'static str) {
    LiquidCard(Modifier::empty().fill_max_width(), move || {
        Row(
            Modifier::empty().fill_max_width().padding(14.0),
            RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
            move || {
                let colors = liquid_colors();
                Box(
                    Modifier::empty()
                        .size(cranpose::Size::new(68.0, 68.0))
                        .draw_behind(move |scope| {
                            let size = scope.size();
                            scope.draw_round_rect(
                                Brush::linear_gradient_range(
                                    vec![
                                        Color::from_rgb_u8(44, 44, 52),
                                        Color::from_rgb_u8(8, 8, 12),
                                    ],
                                    Point::new(0.0, 0.0),
                                    Point::new(size.width, size.height),
                                ),
                                CornerRadii::uniform(18.0),
                            );
                        }),
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || {
                        icons::Icon(icon, 34.0, Color::from_rgb_u8(214, 222, 240));
                    },
                );
                Box(Modifier::empty().width(16.0), BoxSpec::default(), || {});
                Column(Modifier::empty().weight(1.0), ColumnSpec::default(), {
                    move || {
                        Text(
                            title,
                            Modifier::empty(),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(colors.label),
                                    font_size: TextUnit::Sp(21.0),
                                    font_weight: Some(cranpose::text::FontWeight::BOLD),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        Box(Modifier::empty().height(3.0), BoxSpec::default(), || {});
                        Text(
                            subtitle,
                            Modifier::empty(),
                            body_style(colors.secondary_label),
                        );
                    }
                });
            },
        );
    });
    Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});
}

/// The showcase page.
#[composable]
pub fn LiquidUiTab() {
    let dark = remember(|| mutableStateOf(false)).with(|s| *s);
    let selected_tab = remember(|| mutableStateOf(0usize)).with(|s| *s);
    super::TEST_LIQUID_SELECTED_TAB_STATE.with(|cell| {
        *cell.borrow_mut() = Some(selected_tab);
    });
    let store_tab = remember(|| mutableStateOf(4usize)).with(|s| *s);
    let toggle_a = remember(|| mutableStateOf(true)).with(|s| *s);
    let toggle_b = remember(|| mutableStateOf(false)).with(|s| *s);
    let slider = remember(|| mutableStateOf(0.6f32)).with(|s| *s);
    let segment = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let chip = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let menu_open = remember(|| mutableStateOf(false)).with(|s| *s);
    let menu_mode = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let menu_gesture = remember_liquid_menu_gesture();
    let clicks = remember(|| mutableStateOf(0u32)).with(|s| *s);
    let profile_template = GlassSurfaceProfile::lens();
    let profile_editor = GlassProfileEditorState {
        axis: remember(|| mutableStateOf(0usize)).with(|state| *state),
        knot: remember(|| mutableStateOf(3usize)).with(|state| *state),
        x_points: remember(move || mutableStateOf(profile_points(profile_template.x_profile())))
            .with(|state| *state),
        y_points: remember(move || mutableStateOf(profile_points(profile_template.y_profile())))
            .with(|state| *state),
        depth: remember(|| mutableStateOf(5.5 / 12.0)).with(|state| *state),
        radial_power: remember(|| mutableStateOf((3.2 - 1.5) / 6.5)).with(|state| *state),
        axis_coupling: remember(move || mutableStateOf(profile_template.axis_coupling()))
            .with(|state| *state),
        aberration: remember(|| mutableStateOf(0.40)).with(|state| *state),
        dispersion_x: remember(|| mutableStateOf(0.40)).with(|state| *state),
        dispersion_y: remember(|| mutableStateOf(0.40)).with(|state| *state),
        frost: remember(|| mutableStateOf(0.0)).with(|state| *state),
        drag_center: remember(|| mutableStateOf((0.50, 0.54))).with(|state| *state),
        dragging: remember(|| mutableStateOf(false)).with(|state| *state),
    };

    let scheme = if dark.get() {
        SchemeMode::Dark
    } else {
        SchemeMode::Light
    };

    LiquidTheme(
        LiquidThemeSpec {
            scheme,
            ..LiquidThemeSpec::default()
        },
        move || {
            let menu_gesture = menu_gesture.clone();
            let colors = liquid_colors();
            let scroll = remember(|| ScrollState::new(0.0)).with(|s| s.clone());
            let scroll_for_bar = scroll.clone();

            cranpose::widgets::Box(
                Modifier::empty().fill_max_size().draw_behind(move |scope| {
                    scope.draw_rect(Brush::solid(colors.background));
                }),
                cranpose::widgets::BoxSpec::default(),
                move || {
                    // ---- Scrollable content (slides under the bars) ----
                    let dark_for_chip = dark;
                    let toggle_a = toggle_a;
                    let toggle_b = toggle_b;
                    let slider_state = slider;
                    let segment_state = segment;
                    let chip_state = chip;
                    let store_tab_state = store_tab;
                    let clicks_state = clicks;
                    let profile_editor = profile_editor;
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .vertical_scroll(scroll.clone(), false)
                            .padding_each(
                                PAGE_PADDING,
                                liquid_nav_bar_expanded_height() + 8.0,
                                PAGE_PADDING,
                                120.0,
                            ),
                        ColumnSpec::default(),
                        move || {
                            SectionTitle("PHYSICAL PROFILE");
                            GlassProfilePlayground(profile_editor);

                            SectionTitle("TAB SWIPE");
                            TabSwipeReferenceBackdrop();
                            Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});

                            // WWDC-style sessions list: the content the
                            // floating tab bar and morphing menu hover over.
                            for (icon, title, subtitle) in [
                                (
                                    icons::GRID,
                                    "iPadOS",
                                    "Unlock the full potential of iPadOS.",
                                ),
                                (
                                    icons::FOLDER,
                                    "macOS",
                                    "Take full advantage of the powerful capabilities of Mac.",
                                ),
                                (
                                    icons::STAR,
                                    "Metal",
                                    "Explore the latest in Metal tools, technologies, and resources.",
                                ),
                                (
                                    icons::EDIT,
                                    "Swift",
                                    "Discover updates to Swift and related tools and frameworks.",
                                ),
                                (
                                    icons::HOME,
                                    "SwiftUI",
                                    "Design and build your apps like never before.",
                                ),
                            ] {
                                SessionCard(icon, title, subtitle);
                            }

                            SectionTitle("BOTTOM BAR");
                            let store_state = store_tab_state;
                            StoreBottomBarExample(store_state.get(), move |index| {
                                store_state.set(index);
                            });
                            Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});

                            // Vivid backdrop content for the glass to lens.
                            GradientStripe(
                                [
                                    Color::from_rgb_u8(255, 94, 58),
                                    Color::from_rgb_u8(255, 149, 0),
                                ],
                                120.0,
                            );

                            SectionTitle("BUTTONS");
                            Row(
                                Modifier::empty(),
                                RowSpec::default()
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                {
                                    let clicks = clicks_state;
                                    move || {
                                        let clicks_a = clicks;
                                        GlassButton(
                                            Modifier::empty(),
                                            GlassButtonSpec::glass(),
                                            move || clicks_a.set(clicks_a.get() + 1),
                                            || {
                                                GlassButtonLabel("Glass", GlassButtonSpec::glass());
                                            },
                                        );
                                        Box(
                                            Modifier::empty().width(12.0),
                                            BoxSpec::default(),
                                            || {},
                                        );
                                        let clicks_b = clicks;
                                        GlassButton(
                                            Modifier::empty(),
                                            GlassButtonSpec::prominent(),
                                            move || clicks_b.set(clicks_b.get() + 1),
                                            || {
                                                GlassButtonLabel(
                                                    "Prominent",
                                                    GlassButtonSpec::prominent(),
                                                );
                                            },
                                        );
                                        Box(
                                            Modifier::empty().width(12.0),
                                            BoxSpec::default(),
                                            || {},
                                        );
                                        let clicks_c = clicks;
                                        GlassIconButton(
                                            Modifier::empty(),
                                            GlassButtonSpec::glass(),
                                            44.0,
                                            move || clicks_c.set(clicks_c.get() + 1),
                                            icons::PLUS,
                                        );
                                    }
                                },
                            );

                            SectionTitle("CONTROLS");
                            LiquidCard(Modifier::empty().fill_max_width(), {
                                let toggle_a = toggle_a;
                                let toggle_b = toggle_b;
                                let slider_state = slider_state;
                                move || {
                                    let toggle_a = toggle_a;
                                    let toggle_b = toggle_b;
                                    let slider_state = slider_state;
                                    Column(
                                        Modifier::empty().padding(16.0),
                                        ColumnSpec::default(),
                                        move || {
                                            let colors = liquid_colors();
                                            let toggle_a2 = toggle_a;
                                            Row(
                                                Modifier::empty().fill_max_width(),
                                                RowSpec::default().vertical_alignment(
                                                    VerticalAlignment::CenterVertically,
                                                ),
                                                move || {
                                                    Text(
                                                        "Wi-Fi",
                                                        Modifier::empty().weight(1.0),
                                                        body_style(colors.label),
                                                    );
                                                    let t = toggle_a2;
                                                    LiquidToggle(
                                                        Modifier::empty(),
                                                        t.get(),
                                                        move |value| t.set(value),
                                                    );
                                                },
                                            );
                                            Box(
                                                Modifier::empty().height(12.0),
                                                BoxSpec::default(),
                                                || {},
                                            );
                                            let toggle_b2 = toggle_b;
                                            Row(
                                                Modifier::empty().fill_max_width(),
                                                RowSpec::default().vertical_alignment(
                                                    VerticalAlignment::CenterVertically,
                                                ),
                                                move || {
                                                    Text(
                                                        "Airplane Mode",
                                                        Modifier::empty().weight(1.0),
                                                        body_style(colors.label),
                                                    );
                                                    let t = toggle_b2;
                                                    LiquidToggle(
                                                        Modifier::empty(),
                                                        t.get(),
                                                        move |value| t.set(value),
                                                    );
                                                },
                                            );
                                            Box(
                                                Modifier::empty().height(16.0),
                                                BoxSpec::default(),
                                                || {},
                                            );
                                            let s = slider_state;
                                            LiquidSlider(
                                                Modifier::empty().fill_max_width(),
                                                s.get(),
                                                move |value| s.set(value),
                                            );
                                        },
                                    );
                                }
                            });

                            SectionTitle("SEGMENTED");
                            let seg = segment_state;
                            LiquidSegmentedControl(
                                Modifier::empty().fill_max_width(),
                                vec![
                                    "All".to_string(),
                                    "Receipts".to_string(),
                                    "Docs".to_string(),
                                ],
                                seg.get(),
                                move |index| seg.set(index),
                            );

                            SectionTitle("CHIPS");
                            Row(Modifier::empty(), RowSpec::default(), {
                                let chip_state = chip_state;
                                let dark_for_chip = dark_for_chip;
                                move || {
                                    for (index, label) in
                                        ["All", "Recent", "Favorites"].iter().enumerate()
                                    {
                                        let chip_state2 = chip_state;
                                        LiquidChip(
                                            Modifier::empty().padding_each(0.0, 0.0, 8.0, 0.0),
                                            chip_state.get() == index,
                                            move || chip_state2.set(index),
                                            *label,
                                        );
                                    }
                                    let dark2 = dark_for_chip;
                                    LiquidChip(
                                        Modifier::empty(),
                                        dark_for_chip.get(),
                                        move || dark2.set(!dark2.get()),
                                        "Dark mode",
                                    );
                                }
                            });

                            GradientStripe(
                                [
                                    Color::from_rgb_u8(0, 122, 255),
                                    Color::from_rgb_u8(88, 86, 214),
                                ],
                                120.0,
                            );

                            SectionTitle("LIST");
                            LiquidListSection(Modifier::empty().fill_max_width(), "Library", {
                                move || {
                                    for (index, (icon, title)) in [
                                        (icons::DOCUMENT, "Receipt 2026-06-30"),
                                        (icons::CAMERA, "Idea Marketi doo"),
                                        (icons::FOLDER, "Groceries"),
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    {
                                        LiquidListRow(
                                            Modifier::empty(),
                                            LiquidListRowSpec::default().with_separator(index < 2),
                                            || {},
                                            move || {
                                                let colors = liquid_colors();
                                                Row(
                                                    Modifier::empty().fill_max_width(),
                                                    RowSpec::default().vertical_alignment(
                                                        VerticalAlignment::CenterVertically,
                                                    ),
                                                    move || {
                                                        icons::Icon(icon, 20.0, colors.accent);
                                                        Box(
                                                            Modifier::empty().width(12.0),
                                                            BoxSpec::default(),
                                                            || {},
                                                        );
                                                        Text(
                                                            title,
                                                            Modifier::empty().weight(1.0),
                                                            body_style(colors.label),
                                                        );
                                                        icons::Icon(
                                                            icons::CHEVRON_RIGHT,
                                                            16.0,
                                                            colors.tertiary_label,
                                                        );
                                                    },
                                                );
                                            },
                                        );
                                    }
                                }
                            });
                            GradientStripe(
                                [
                                    Color::from_rgb_u8(175, 82, 222),
                                    Color::from_rgb_u8(255, 204, 0),
                                ],
                                160.0,
                            );
                        },
                    );

                    // ---- Nav bar over the content ----
                    // The menu anchors to the REAL composited rects of the
                    // trailing buttons (window coords via report_window_rect)
                    // — never guessed offsets.
                    let menu_anchor_rect = cranpose_core::remember(|| {
                        std::rc::Rc::new(std::cell::Cell::new(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                        }))
                    })
                    .with(std::rc::Rc::clone);
                    let menu_absorbed_rect = cranpose_core::remember(|| {
                        std::rc::Rc::new(std::cell::Cell::new(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                        }))
                    })
                    .with(std::rc::Rc::clone);
                    let menu_for_nav = menu_open;
                    let menu_gesture_for_nav = menu_gesture.clone();
                    let anchor_sink = std::rc::Rc::clone(&menu_anchor_rect);
                    let absorbed_sink = std::rc::Rc::clone(&menu_absorbed_rect);
                    let absorbed_button_spec = GlassButtonSpec::glass()
                        .with_glass(
                            Glass::regular()
                                .lift(0.05)
                                .highlight(0.30)
                                .surface_profile(
                                    GlassSurfaceProfile::regular()
                                        .with_depth(2.8)
                                        .expect("absorbed button surface depth is valid"),
                                )
                                .adaptive_frost(colors.label, 0.35),
                        )
                        .with_icon_backplate(colors.accent)
                        .with_content_color(Color::WHITE);
                    let nav_absorbed_button_spec = absorbed_button_spec.clone();
                    LiquidNavBar(
                        Modifier::empty().fill_max_width(),
                        LiquidNavBarSpec::new("WWDC"),
                        scroll_for_bar.clone(),
                        || {},
                        move || {
                            let menu = menu_for_nav;
                            let menu_gesture = menu_gesture_for_nav.clone();
                            let anchor_sink = std::rc::Rc::clone(&anchor_sink);
                            let absorbed_sink = std::rc::Rc::clone(&absorbed_sink);
                            let absorbed_button_spec = nav_absorbed_button_spec.clone();
                            Row(
                                Modifier::empty(),
                                RowSpec::default()
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                move || {
                                    let menu_gesture = menu_gesture.clone();
                                    LiquidMenuAbsorbedIconButton(
                                        Modifier::empty()
                                            .report_window_rect(std::rc::Rc::clone(&absorbed_sink)),
                                        absorbed_button_spec.clone(),
                                        44.0,
                                        menu.get(),
                                        || {},
                                        icons::FILTER,
                                    );
                                    Box(Modifier::empty().width(8.0), BoxSpec::default(), || {});
                                    LiquidMenuIconButton(
                                        Modifier::empty()
                                            .report_window_rect(std::rc::Rc::clone(&anchor_sink)),
                                        GlassButtonSpec::glass(),
                                        44.0,
                                        menu.get(),
                                        menu_gesture,
                                        move || menu.set(true),
                                        icons::MORE_HORIZ,
                                    );
                                },
                            );
                        },
                    );

                    // ---- Morphing menu ----
                    let menu_state = menu_open;
                    let menu_dismiss = menu_open;
                    let menu_selection = menu_mode;
                    let menu_anchor = menu_anchor_rect.get();
                    let menu_absorbed = menu_absorbed_rect.get();
                    LiquidMenu(
                        menu_state.get(),
                        menu_anchor,
                        vec![LiquidMenuAbsorbedSource::new(
                            menu_absorbed,
                            absorbed_button_spec,
                            44.0,
                            icons::FILTER,
                        )],
                        vec![
                            LiquidMenuItem::new("List")
                                .icon(icons::LIST_OUTLINE)
                                .checked(menu_selection.get() == 0),
                            LiquidMenuItem::new("Grid")
                                .icon(icons::GRID_OUTLINE)
                                .checked(menu_selection.get() == 1),
                        ],
                        menu_gesture.clone(),
                        move |index| menu_selection.set(index),
                        move || menu_dismiss.set(false),
                    );

                    // ---- Floating tab bar ----
                    let tab_state = selected_tab;
                    Box(
                        Modifier::empty().fill_max_size().padding_each(
                            PAGE_PADDING,
                            0.0,
                            PAGE_PADDING,
                            LIQUID_TAB_BAR_BOTTOM_PADDING,
                        ),
                        BoxSpec::default().content_alignment(Alignment::new(
                            HorizontalAlignment::CenterHorizontally,
                            VerticalAlignment::Bottom,
                        )),
                        move || {
                            let tab_state2 = tab_state;
                            LiquidTabBarWithAccessory(
                                Modifier::empty(),
                                vec![
                                    LiquidTab::new(icons::STAR, "Discover"),
                                    LiquidTab::new(icons::SHOPPING_BAG, "Browse"),
                                    LiquidTab::app_badge(icons::APPLE, "WWDC"),
                                    LiquidTab::new(icons::ACCOUNT_CIRCLE, "Account")
                                        .with_icon_scale(0.82),
                                ],
                                tab_state.get(),
                                move |index| tab_state2.set(index),
                                || {
                                    LiquidTabBarSearchAccessory(|| {});
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_editor_round_trips_the_production_lens_curves() {
        let profile = GlassSurfaceProfile::lens();
        let x = profile_points(profile.x_profile());
        let y = profile_points(profile.y_profile());
        assert_eq!(profile_curve(x), profile.x_profile());
        assert_eq!(profile_curve(y), profile.y_profile());
    }

    #[test]
    fn tab_swipe_reference_backdrop_uses_target_geometry() {
        let layout = tab_swipe_reference_layout();
        assert_eq!(layout.size, Size::new(440.0, 132.0));
        assert_eq!(
            layout.title,
            Rect {
                x: 40.0,
                y: 0.0,
                width: 360.0,
                height: 29.0,
            }
        );
        assert_eq!(
            layout.enroll,
            Rect {
                x: 20.0,
                y: 33.0,
                width: 400.0,
                height: 51.0,
            }
        );
        assert_eq!(
            layout.xcode,
            Rect {
                x: 32.0,
                y: 69.0,
                width: 376.0,
                height: 50.0,
            }
        );
    }

    #[test]
    fn tab_swipe_reference_row_is_physical_glass() {
        let glass = tab_swipe_reference_glass();
        assert_eq!(glass.variant, GlassVariant::Regular);
        assert!(glass.shadow);
        assert_eq!(glass.surface_profile.depth(), 3.0);
        assert!(glass.blur_radius.is_none());
        assert!(glass.tint.is_none());
    }

    #[test]
    fn floating_tab_bar_matches_the_reference_bottom_inset() {
        assert_eq!(LIQUID_TAB_BAR_BOTTOM_PADDING, 19.0);
    }

    #[test]
    fn profile_position_edit_preserves_strict_normalized_ordering() {
        let points = profile_points(GlassSurfaceProfile::lens().x_profile());
        let moved_left = update_profile_position(points, 3, 0.0);
        assert_eq!(moved_left[3].0, moved_left[2].0 + PROFILE_POSITION_GAP);
        let moved_right = update_profile_position(points, 3, 1.0);
        assert_eq!(moved_right[3].0, moved_right[4].0 - PROFILE_POSITION_GAP);
        assert_eq!(update_profile_position(points, 0, 0.4), points);
        assert_eq!(
            update_profile_position(points, PROFILE_KNOT_COUNT - 1, 0.4),
            points
        );
        assert!(GlassProfileCurve::from_points(&moved_left).is_ok());
        assert!(GlassProfileCurve::from_points(&moved_right).is_ok());
    }

    #[test]
    fn profile_height_edit_is_normalized() {
        let points = profile_points(GlassSurfaceProfile::lens().y_profile());
        assert_eq!(update_profile_height(points, 2, -1.0)[2].1, 0.0);
        assert_eq!(update_profile_height(points, 2, 2.0)[2].1, 1.0);
    }

    #[test]
    fn profile_workspace_keeps_the_optic_and_curves_in_one_viewport() {
        assert!(
            PROFILE_STAGE_HEIGHT <= 200.0,
            "the draggable optic must not consume a viewport by itself"
        );
        assert!(
            PROFILE_PLOT_HEIGHT <= 200.0,
            "the physical curves must remain visible beside the optic"
        );
        assert!(profile_uses_wide_workspace(824.0));
        assert!(!profile_uses_wide_workspace(360.0));
        assert_eq!(profile_wide_pane_width(824.0), 407.0);
        let rows = profile_stage_text_rows(PROFILE_STAGE_HEIGHT);
        assert_eq!(rows, 5);
        assert!(rows as f32 * 31.0 + 20.0 <= PROFILE_STAGE_HEIGHT);
        assert!(PROFILE_STAGE_TEXT.len() <= 40);
    }

    #[test]
    fn profile_demo_drag_preserves_grab_offset_and_clamps_to_the_stage() {
        let center = profile_drag_center(
            Point::new(260.0, 120.0),
            Point::new(20.0, -5.0),
            400.0,
            190.0,
        );
        assert_eq!(center, (0.6, 125.0 / 190.0));

        let top_left = profile_drag_center(
            Point::new(-100.0, -100.0),
            Point::new(0.0, 0.0),
            400.0,
            190.0,
        );
        assert_eq!(
            top_left,
            (
                PROFILE_DEMO_WIDTH * 0.5 / 400.0,
                PROFILE_DEMO_HEIGHT * 0.5 / 190.0,
            )
        );
    }

    #[test]
    fn profile_dispersion_controls_are_normalized_but_resolve_physically() {
        assert_eq!(profile_dispersion_axis(-1.0), 0.0);
        assert_eq!(profile_dispersion_axis(0.4), 1.0);
        assert_eq!(profile_dispersion_axis(2.0), PROFILE_DISPERSION_AXIS_MAX);
    }
}
