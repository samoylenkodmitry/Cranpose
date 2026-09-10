#![allow(non_snake_case)]

use std::sync::{Arc, OnceLock};

use cranpose_animation::{animateFloatAsState, spring, AnimationType, Spring};
use cranpose_core::{key, rememberMutableStateOf};
use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer,
    LinearArrangement, Modifier, Point, PointerEventKind, PointerInputScope, Row, RowSpec, Size,
    Text, TextStyle, TransformOrigin, VerticalAlignment,
};
use cranpose_ui_graphics::{
    CompositingStrategy, RenderEffect, RuntimeShader, RUNTIME_SHADER_PRELUDE_WGSL,
};

const HALF_WIDTH: f32 = 232.0;
const SCREEN_WIDTH: f32 = HALF_WIDTH * 2.0;
const SCREEN_HEIGHT: f32 = 322.0;
const BEZEL: f32 = 7.0;
const CORNER: f32 = 22.0;
const HINGE_WIDTH: f32 = 5.0;
/// How far the folding half throws its shadow across the half that stays.
const CREASE_SHADOW: f32 = 30.0;
/// How far the folding half swings. Past a right angle it presents its back,
/// which is the shell of the closed device.
const SHUT_ANGLE: f32 = 158.0;
const RIGHT_ANGLE: f32 = 90.0;
const STRAIGHT_ANGLE: f32 = 180.0;
const CAMERA_DISTANCE: f32 = 15.0;
/// The device turns a little further from the reader as it closes, the way a
/// hand turns it while folding.
const OPEN_TILT: f32 = -15.0;
const SHUT_TILT: f32 = -32.0;
const STAGE_WIDTH: f32 = 760.0;
const STAGE_HEIGHT: f32 = 470.0;
const MAX_BLUR_PX: f32 = 16.0;
const PANEL_DIM: f32 = 0.95;
const GLASS_SHEEN: f32 = 0.34;
/// A flick of this much of the fold per second carries the panel the rest of
/// the way on its own.
const FLICK_FOLD: f32 = 1.6;

/// How far the device is folded, from flat open to shut.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Fold {
    shut: f32,
}

impl Fold {
    fn at(shut: f32) -> Self {
        Self {
            shut: shut.clamp(0.0, 1.0),
        }
    }

    fn angle(self) -> f32 {
        self.shut * SHUT_ANGLE
    }

    /// Before a right angle the folding half still faces the reader; past it,
    /// the back of the device does.
    fn shows_screen(self) -> bool {
        self.angle() < RIGHT_ANGLE
    }

    /// How far the panel stands off the flat plane, which is what the blur and
    /// the crease light follow.
    fn bend(self) -> f32 {
        self.angle().to_radians().sin().abs()
    }

    /// How much light the folding half has turned away from. It loses most of
    /// it early: a panel only a little off square already faces away from the
    /// window the light comes through.
    fn dim(self) -> f32 {
        let turned = (self.angle() * 0.5).to_radians().sin().abs();
        (turned.powf(0.65) * PANEL_DIM).clamp(0.0, 1.0)
    }

    fn tilt(self) -> f32 {
        OPEN_TILT + (SHUT_TILT - OPEN_TILT) * self.shut
    }

    fn reading(self) -> String {
        if self.shut <= 0.0 {
            "Open".to_string()
        } else if self.shut >= 1.0 {
            "Shut".to_string()
        } else {
            format!("{} % folded", (self.shut * 100.0).round() as i32)
        }
    }
}

/// A drag of one half's width folds the device all the way; dragging left
/// closes it.
fn fold_after_drag(shut: f32, delta_x: f32) -> f32 {
    (shut - delta_x / HALF_WIDTH).clamp(0.0, 1.0)
}

/// Where a released panel settles: past halfway, or carried over it by a flick.
fn settled_fold(shut: f32, folds_per_second: f32) -> f32 {
    if folds_per_second <= -FLICK_FOLD {
        0.0
    } else if folds_per_second >= FLICK_FOLD || shut >= 0.5 {
        1.0
    } else {
        0.0
    }
}

/// A half is rounded on the outside and square where the hinge holds it.
fn outer_corners(hinge_at_right: bool) -> CornerRadii {
    if hinge_at_right {
        CornerRadii {
            top_left: CORNER,
            top_right: 0.0,
            bottom_right: 0.0,
            bottom_left: CORNER,
        }
    } else {
        CornerRadii {
            top_left: 0.0,
            top_right: CORNER,
            bottom_right: CORNER,
            bottom_left: 0.0,
        }
    }
}

fn stage_color() -> Color {
    Color(0.784, 0.780, 0.792, 1.0)
}

fn shell_color() -> Color {
    Color(0.118, 0.118, 0.126, 1.0)
}

fn bezel_color() -> Color {
    Color(0.043, 0.043, 0.047, 1.0)
}

fn edge_color() -> Color {
    Color(0.72, 0.72, 0.75, 1.0)
}

fn clock_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.97, 0.97, 0.98, 1.0)),
            font_size: TextUnit::Sp(84.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn date_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.94, 0.94, 0.96, 1.0)),
            font_size: TextUnit::Sp(15.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn title_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.12, 0.12, 0.13, 1.0)),
            font_size: TextUnit::Sp(28.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn caption_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.36, 0.36, 0.38, 1.0)),
            font_size: TextUnit::Sp(13.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn foldable_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    SOURCE
        .get_or_init(|| {
            Arc::<str>::from(format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}{}",
                include_str!("foldable.wgsl")
            ))
        })
        .clone()
}

/// The picture for one window on to the open screen, given where that window
/// starts and how much of the screen it spans.
fn wallpaper_effect(origin: f32, span: f32) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(foldable_wgsl());
    shader.set_float(0, 0.0);
    shader.set_float(1, origin);
    shader.set_float(2, span);
    RenderEffect::runtime_shader(shader)
}

struct GlassUniforms {
    bend: f32,
    dim: f32,
    hinge_at_left: bool,
}

fn glass_effect(uniforms: &GlassUniforms) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(foldable_wgsl());
    shader.set_float(0, 1.0);
    shader.set_float(1, uniforms.bend);
    shader.set_float(2, MAX_BLUR_PX);
    shader.set_float(3, uniforms.dim);
    shader.set_float(4, GLASS_SHEEN);
    shader.set_float(5, if uniforms.hinge_at_left { 1.0 } else { 0.0 });
    RenderEffect::runtime_shader(shader)
}

#[composable]
pub(crate) fn FoldableTab() {
    let shut = rememberMutableStateOf(|| 0.0f32);
    let dragging = rememberMutableStateOf(|| false);

    let animated = animateFloatAsState(shut.get(), fold_animation(dragging.get()), "foldable_shut");
    let fold = Fold::at(animated.value());

    Column(
        Modifier::empty()
            .fill_max_width()
            .background(stage_color())
            .padding_symmetric(2.0, 6.0),
        ColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(10.0))
            .horizontal_alignment(cranpose_ui::HorizontalAlignment::CenterHorizontally),
        move || {
            FoldHeader(fold);
            Stage(fold, shut, dragging);
            Text(
                "Drag across the screen to fold it. The picture and the clock are one surface: \
                 the crease runs straight through them.",
                Modifier::empty().padding_symmetric(24.0, 0.0),
                caption_style(),
            );
        },
    );
}

fn fold_animation(dragging: bool) -> AnimationType {
    if dragging {
        spring(Spring::DampingRatioNoBouncy, Spring::StiffnessHigh)
    } else {
        spring(Spring::DampingRatioNoBouncy, Spring::StiffnessMediumLow)
    }
}

#[composable]
fn FoldHeader(fold: Fold) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_symmetric(8.0, 4.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text("Foldable", Modifier::empty(), title_style());
            Text(fold.reading(), Modifier::empty(), caption_style());
        },
    );
}

#[composable]
fn Stage(
    fold: Fold,
    shut: cranpose_core::MutableState<f32>,
    dragging: cranpose_core::MutableState<bool>,
) {
    let reading = fold.reading();

    Box(
        Modifier::empty()
            .size_points(STAGE_WIDTH, STAGE_HEIGHT)
            .semantics(move |config: &mut SemanticsConfiguration| {
                config.content_description = Some("Foldable screen".to_string());
                config.state_description = Some(reading.clone());
            })
            .pointer_input((), move |scope: PointerInputScope| async move {
                scope
                    .await_pointer_event_scope(|events| async move {
                        let mut last = Point::default();
                        let mut started = 0.0f32;
                        let mut travelled = 0.0f32;
                        loop {
                            let event = events.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Down => {
                                    last = event.position;
                                    started = shut.get();
                                    travelled = 0.0;
                                    dragging.set(true);
                                    event.consume();
                                }
                                PointerEventKind::Move => {
                                    if dragging.get() {
                                        travelled += event.position.x - last.x;
                                        last = event.position;
                                        shut.set(fold_after_drag(started, travelled));
                                        event.consume();
                                    }
                                }
                                PointerEventKind::Up | PointerEventKind::Cancel => {
                                    if dragging.get() {
                                        dragging.set(false);
                                        let flick = -(event.position.x - last.x) / HALF_WIDTH;
                                        shut.set(settled_fold(shut.get(), flick));
                                    }
                                }
                                PointerEventKind::Exit => {
                                    if dragging.get() {
                                        dragging.set(false);
                                        shut.set(settled_fold(shut.get(), 0.0));
                                    }
                                }
                                _ => {}
                            }
                        }
                    })
                    .await;
            }),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Device(fold);
        },
    );
}

#[composable]
fn Device(fold: Fold) {
    let tilt = fold.tilt();

    Box(
        Modifier::empty().size_points(SCREEN_WIDTH, SCREEN_HEIGHT),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            key("still", || StillHalf(tilt));
            Hinge(fold, tilt);
            key("folding", || FoldingHalf(fold, tilt));
        },
    );
}

/// Every part of the device turns about the one line the hinge runs down, so
/// each part carries its own turn about that line: how the reader is holding
/// the device and, for the folding half, the fold on top of it. That is one
/// layer each, sharing a camera, rather than a turned layer inside a turned
/// layer, which samples in tiles and leaves steps along the device's edge.
fn hinge_layer(degrees: f32, origin: TransformOrigin) -> GraphicsLayer {
    GraphicsLayer {
        rotation_y: degrees,
        camera_distance: CAMERA_DISTANCE,
        transform_origin: origin,
        ..Default::default()
    }
}

/// The half the hinge holds still, which keeps its picture square to the
/// reader for the whole fold.
#[composable]
fn StillHalf(tilt: f32) {
    Box(
        Modifier::empty()
            .absolute_offset(HALF_WIDTH, 0.0)
            .size_points(HALF_WIDTH, SCREEN_HEIGHT)
            .graphics_layer(move || hinge_layer(tilt, TransformOrigin::new(0.0, 0.5))),
        BoxSpec::default(),
        move || {
            HalfFace(HALF_WIDTH, false);
        },
    );
}

/// The half that swings on the hinge. Past a right angle it has turned its
/// screen away and the reader sees the shell of the device instead.
#[composable]
fn FoldingHalf(fold: Fold, tilt: f32) {
    let screen = fold.shows_screen();
    let angle = if screen {
        tilt + fold.angle()
    } else {
        tilt + fold.angle() - STRAIGHT_ANGLE
    };
    let origin = if screen {
        TransformOrigin::new(1.0, 0.5)
    } else {
        TransformOrigin::new(0.0, 0.5)
    };
    let x = if screen { 0.0 } else { HALF_WIDTH };
    let bend = fold.bend();
    let dim = fold.dim();

    Box(
        Modifier::empty()
            .absolute_offset(x, 0.0)
            .size_points(HALF_WIDTH, SCREEN_HEIGHT)
            .graphics_layer(move || hinge_layer(angle, origin)),
        BoxSpec::default(),
        move || {
            if screen {
                Box(
                    Modifier::empty()
                        .size_points(HALF_WIDTH, SCREEN_HEIGHT)
                        .graphics_layer(move || GraphicsLayer {
                            render_effect: Some(glass_effect(&GlassUniforms {
                                bend,
                                dim,
                                hinge_at_left: false,
                            })),
                            compositing_strategy: CompositingStrategy::Offscreen,
                            ..Default::default()
                        }),
                    BoxSpec::default(),
                    move || {
                        HalfFace(0.0, true);
                    },
                );
            } else {
                ShellFace(dim);
            }
        },
    );
}

/// One half of the device: bezel, then a window on to the screen. The picture
/// is drawn from where the window sits in the whole open screen, and what is
/// written on the screen is laid out across all of it and clipped to the
/// window, so both cross the crease unbroken.
#[composable]
fn HalfFace(offset: f32, hinge_at_right: bool) {
    let radii = outer_corners(hinge_at_right);
    let glass = Size::new(HALF_WIDTH - BEZEL * 2.0, SCREEN_HEIGHT - BEZEL * 2.0);
    let origin = offset / SCREEN_WIDTH;

    Box(
        Modifier::empty()
            .size_points(HALF_WIDTH, SCREEN_HEIGHT)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(bezel_color()), radii);
            }),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            Box(
                Modifier::empty()
                    .absolute_offset(BEZEL, BEZEL)
                    .size_points(glass.width, glass.height)
                    .clip_to_bounds(),
                BoxSpec::new().content_alignment(Alignment::TOP_START),
                move || {
                    Box(
                        Modifier::empty()
                            .size_points(glass.width, glass.height)
                            .graphics_layer(move || GraphicsLayer {
                                render_effect: Some(wallpaper_effect(origin, 0.5)),
                                compositing_strategy: CompositingStrategy::Offscreen,
                                ..Default::default()
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .absolute_offset(-offset, 0.0)
                            .required_size(Size::new(SCREEN_WIDTH - BEZEL * 2.0, glass.height)),
                        BoxSpec::new().content_alignment(Alignment::TOP_START),
                        move || {
                            ScreenWriting();
                        },
                    );
                },
            );
        },
    );
}

/// The back of the folding half once it has turned past the reader: graphite,
/// with the light running down the edge the hinge holds and a sweep of it
/// across the shell.
#[composable]
fn ShellFace(dim: f32) {
    let lit = (1.0 - dim * 0.45).clamp(0.0, 1.0);
    let base = shell_color();
    let near = Color(
        base.0 * lit * 2.1,
        base.1 * lit * 2.1,
        base.2 * lit * 2.2,
        1.0,
    );
    let far = Color(base.0 * lit, base.1 * lit, base.2 * lit, 1.0);

    Box(
        Modifier::empty()
            .size_points(HALF_WIDTH, SCREEN_HEIGHT)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect(
                    Brush::linear_gradient_range(
                        vec![near, far],
                        Point { x: 0.0, y: 0.0 },
                        Point {
                            x: size.width,
                            y: 0.0,
                        },
                    ),
                    outer_corners(false),
                );
                scope.draw_round_rect(
                    Brush::linear_gradient_range(
                        vec![
                            Color(1.0, 1.0, 1.0, 0.0),
                            Color(1.0, 1.0, 1.0, 0.07 * lit),
                            Color(1.0, 1.0, 1.0, 0.0),
                        ],
                        Point { x: 0.0, y: 0.0 },
                        Point {
                            x: size.width * 0.9,
                            y: size.height,
                        },
                    ),
                    outer_corners(false),
                );
            }),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            Box(
                Modifier::empty()
                    .size_points(HINGE_WIDTH * 0.6, SCREEN_HEIGHT)
                    .draw_behind(move |scope| {
                        let size = scope.size();
                        scope.draw_rect(Brush::linear_gradient_range(
                            vec![
                                Color(
                                    edge_color().0 * lit,
                                    edge_color().1 * lit,
                                    edge_color().2 * lit,
                                    1.0,
                                ),
                                Color(0.0, 0.0, 0.0, 0.0),
                            ],
                            Point { x: 0.0, y: 0.0 },
                            Point {
                                x: size.width,
                                y: 0.0,
                            },
                        ));
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// The seam down the middle: the shadow the folding half drops across the
/// half that stays, and the metal the two of them turn on.
#[composable]
fn Hinge(fold: Fold, tilt: f32) {
    let bend = fold.bend();
    let cast = HINGE_WIDTH + CREASE_SHADOW * bend;
    let depth = 0.5 * bend;

    HingeStrip(
        HALF_WIDTH,
        cast,
        tilt,
        TransformOrigin::new(0.0, 0.5),
        vec![Color(0.0, 0.0, 0.0, depth), Color(0.0, 0.0, 0.0, 0.0)],
    );
    HingeStrip(
        HALF_WIDTH - HINGE_WIDTH * 0.5,
        HINGE_WIDTH,
        tilt,
        TransformOrigin::new(0.5, 0.5),
        vec![
            Color(0.0, 0.0, 0.0, 0.75),
            edge_color(),
            Color(0.0, 0.0, 0.0, 0.75),
        ],
    );
}

/// A strip standing on the hinge line, painted straight across it.
#[composable]
fn HingeStrip(x: f32, width: f32, tilt: f32, origin: TransformOrigin, colours: Vec<Color>) {
    Box(
        Modifier::empty()
            .absolute_offset(x, 0.0)
            .size_points(width, SCREEN_HEIGHT)
            .graphics_layer(move || hinge_layer(tilt, origin))
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_rect(Brush::linear_gradient_range(
                    colours.clone(),
                    Point { x: 0.0, y: 0.0 },
                    Point {
                        x: size.width,
                        y: 0.0,
                    },
                ));
            }),
        BoxSpec::default(),
        || {},
    );
}

/// What is written on the lock screen, laid out across the whole open screen
/// so the crease cuts through it.
#[composable]
fn ScreenWriting() {
    let width = SCREEN_WIDTH - BEZEL * 2.0;
    let height = SCREEN_HEIGHT - BEZEL * 2.0;

    Box(
        Modifier::empty().required_size(Size::new(width, height)),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            Text(
                "Wed Apr 1",
                Modifier::empty().absolute_offset(width * 0.5 - 40.0, 26.0),
                date_style(),
            );
            Text(
                "9:41",
                Modifier::empty().absolute_offset(width * 0.5 - 92.0, 48.0),
                clock_style(),
            );
            GlassPill(24.0, height - 60.0);
            GlassPill(width - 68.0, height - 60.0);
        },
    );
}

#[composable]
fn GlassPill(x: f32, y: f32) {
    Box(
        Modifier::empty()
            .absolute_offset(x, y)
            .size_points(44.0, 44.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.22)),
                    CornerRadii::uniform(22.0),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fold_runs_from_flat_open_to_shut() {
        assert_eq!(Fold::at(0.0).angle(), 0.0);
        assert!(Fold::at(0.0).shows_screen());
        assert_eq!(Fold::at(0.0).bend(), 0.0);
        assert!((Fold::at(1.0).angle() - SHUT_ANGLE).abs() < 1e-4);
        assert!(
            !Fold::at(1.0).shows_screen(),
            "a shut device has turned its screen away"
        );
    }

    #[test]
    fn a_fold_never_leaves_the_hinge() {
        assert_eq!(Fold::at(-3.0).shut, 0.0);
        assert_eq!(Fold::at(4.5).shut, 1.0);
    }

    #[test]
    fn a_folding_panel_bends_most_at_a_right_angle() {
        let square = Fold::at(RIGHT_ANGLE / SHUT_ANGLE);
        assert!((square.bend() - 1.0).abs() < 1e-3);
        assert!(square.bend() > Fold::at(0.2).bend());
        assert!(square.bend() > Fold::at(1.0).bend());
    }

    #[test]
    fn a_folding_panel_keeps_taking_less_light_all_the_way_shut() {
        assert_eq!(Fold::at(0.0).dim(), 0.0);
        assert!(Fold::at(0.5).dim() > Fold::at(0.2).dim());
        assert!(Fold::at(1.0).dim() > Fold::at(0.5).dim());
        assert!(Fold::at(1.0).dim() <= 1.0);
    }

    #[test]
    fn the_device_turns_further_from_the_reader_as_it_shuts() {
        assert!((Fold::at(0.0).tilt() - OPEN_TILT).abs() < 1e-4);
        assert!((Fold::at(1.0).tilt() - SHUT_TILT).abs() < 1e-4);
        assert!(Fold::at(1.0).tilt() < Fold::at(0.4).tilt());
    }

    #[test]
    fn dragging_left_folds_and_stops_at_flat_and_shut() {
        assert!((fold_after_drag(0.0, -HALF_WIDTH) - 1.0).abs() < 1e-4);
        assert!((fold_after_drag(1.0, HALF_WIDTH * 0.5) - 0.5).abs() < 1e-4);
        assert_eq!(fold_after_drag(0.0, HALF_WIDTH * 3.0), 0.0);
        assert_eq!(fold_after_drag(1.0, -HALF_WIDTH * 3.0), 1.0);
    }

    #[test]
    fn a_released_panel_settles_where_it_was_headed() {
        assert_eq!(settled_fold(0.62, 0.0), 1.0);
        assert_eq!(settled_fold(0.38, 0.0), 0.0);
        assert_eq!(
            settled_fold(0.12, FLICK_FOLD),
            1.0,
            "a flick carries a barely folded panel shut"
        );
        assert_eq!(
            settled_fold(0.88, -FLICK_FOLD),
            0.0,
            "a flick back throws an almost shut panel open"
        );
    }

    #[test]
    fn the_device_reads_where_it_is_in_the_fold() {
        assert_eq!(Fold::at(0.0).reading(), "Open");
        assert_eq!(Fold::at(1.0).reading(), "Shut");
        assert_eq!(Fold::at(0.5).reading(), "50 % folded");
    }
}
