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

const HALF_WIDTH: f32 = 196.0;
const SCREEN_WIDTH: f32 = HALF_WIDTH * 2.0;
const SCREEN_HEIGHT: f32 = 272.0;
const BEZEL: f32 = 6.0;
/// The bezel either half keeps against the crease, which is far thinner than
/// the one around the outside.
const INNER_BEZEL: f32 = 2.0;
const CORNER: f32 = 18.0;
const HINGE_WIDTH: f32 = 4.0;
/// How far the folding half throws its shadow across the half that stays.
const CREASE_SHADOW: f32 = 22.0;
/// The two screens fold on to each other, so the folding half swings toward
/// the reader and, past a right angle, presents the back of the device.
const SHUT_ANGLE: f32 = 155.0;
const RIGHT_ANGLE: f32 = 90.0;
const STRAIGHT_ANGLE: f32 = 180.0;
const CAMERA_DISTANCE: f32 = 15.0;
/// Room above and below a painted half for the part of the turned half that
/// comes toward the reader and so stands taller than the flat one.
const PAINT_PAD: f32 = 52.0;
const STAGE_WIDTH: f32 = 880.0;
const STAGE_HEIGHT: f32 = 430.0;
const MAX_BLUR_PX: f32 = 30.0;
const GLASS_SHEEN: f32 = 0.22;
/// How far into shadow the crease goes once the device is shut.
const CREASE_DARK: f32 = 0.92;
/// What a unit of `camera_distance` is worth in pixels, which the painted half
/// needs to put its picture through the same projection the turned one takes.
const CAMERA_DISTANCE_SCALE: f32 = 72.0;

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

    /// How far the folding half has swung toward the reader, in degrees. At a
    /// right angle it is edge on; past that its back is what faces the reader.
    fn angle(self) -> f32 {
        self.shut * SHUT_ANGLE
    }

    /// Before a right angle the folding half still shows its screen; past it,
    /// the back of the device does.
    fn shows_screen(self) -> bool {
        self.angle() < RIGHT_ANGLE
    }

    /// How much of the width it covers when flat the folding half has given
    /// up. A half turned by `a` covers `cos a` of it, and once it is edge on
    /// it has given up all of it and covers no less from then on.
    fn squeeze(self) -> f32 {
        (1.0 - self.angle().min(RIGHT_ANGLE).to_radians().cos()).clamp(0.0, 1.0)
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

/// A hand holds a device it is folding in the same place, so what the reader
/// sees of it stays where it was. The half that folds gives up its width on
/// one side only, so without this the whole device would appear to slide
/// toward the half that stays.
fn held_still(fold: Fold) -> f32 {
    -HALF_WIDTH * fold.squeeze() * 0.5
}

/// The turn the folding half takes, and the turn the painted half's picture
/// is put through. Past a right angle the back is drawn on the other side of
/// the hinge, so the turn is measured from there.
fn panel_turn(fold: Fold) -> f32 {
    if fold.shows_screen() {
        fold.angle()
    } else {
        fold.angle() - STRAIGHT_ANGLE
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
    /// How far the device is shut, from flat open to edge on.
    fold: f32,
    hinge_at_left: bool,
}

fn glass_effect(uniforms: &GlassUniforms) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(foldable_wgsl());
    shader.set_float(0, 1.0);
    shader.set_float(1, uniforms.fold);
    shader.set_float(2, MAX_BLUR_PX);
    shader.set_float(3, CREASE_DARK);
    shader.set_float(4, GLASS_SHEEN);
    shader.set_float(5, if uniforms.hinge_at_left { 1.0 } else { 0.0 });
    RenderEffect::runtime_shader(shader)
}

struct PaintUniforms {
    fold: f32,
    /// The turn the picture is put through, in degrees.
    turn: f32,
    hinge_at_left: bool,
}

fn paint_effect(uniforms: &PaintUniforms) -> RenderEffect {
    let turn = uniforms.turn.to_radians();
    let mut shader = RuntimeShader::from_shared_source(foldable_wgsl());
    shader.set_float(0, 2.0);
    shader.set_float(1, uniforms.fold);
    shader.set_float(2, MAX_BLUR_PX);
    shader.set_float(3, CREASE_DARK);
    shader.set_float(4, GLASS_SHEEN);
    shader.set_float(5, turn.cos());
    shader.set_float(6, turn.sin());
    shader.set_float(7, CAMERA_DISTANCE * CAMERA_DISTANCE_SCALE);
    shader.set_float(8, HALF_WIDTH);
    shader.set_float(9, SCREEN_HEIGHT);
    shader.set_float(10, if uniforms.hinge_at_left { 1.0 } else { 0.0 });
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
                "Drag across either screen to fold both. On the left a layer transform really \
                 turns the half. On the right nothing moves at all: the picture itself is put \
                 through that turn, so it is skewed, blurred along the way and shadowed into \
                 the crease.",
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
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(28.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    key("turned", || Bench(fold, Built::Turned, "Turned in 3D"));
                    key("painted", || Bench(fold, Built::Painted, "Painted flat"));
                },
            );
        },
    );
}

/// One of the two devices, with what it is under it.
#[composable]
fn Bench(fold: Fold, built: Built, label: &'static str) {
    Column(
        Modifier::empty(),
        ColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(10.0))
            .horizontal_alignment(cranpose_ui::HorizontalAlignment::CenterHorizontally),
        move || {
            Box(
                Modifier::empty().size_points(SCREEN_WIDTH, SCREEN_HEIGHT + PAINT_PAD * 2.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                move || {
                    Device(fold, built);
                },
            );
            Text(label, Modifier::empty(), caption_style());
        },
    );
}

/// Which of the two devices on the stage this is: the one that really turns
/// its half, or the one whose surface never moves at all.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Built {
    /// A layer transform turns the half, and the reader sees the turn.
    Turned,
    /// Nothing moves. A shader puts the picture through the same turn, so the
    /// image itself is skewed, blurred and shadowed into the same shape.
    Painted,
}

#[composable]
fn Device(fold: Fold, built: Built) {
    Box(
        Modifier::empty()
            .offset(held_still(fold), 0.0)
            .size_points(SCREEN_WIDTH, SCREEN_HEIGHT),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            key("still", StillHalf);
            Hinge(fold);
            key("folding", || FoldingHalf(fold, built));
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
fn StillHalf() {
    Box(
        Modifier::empty()
            .absolute_offset(HALF_WIDTH, 0.0)
            .size_points(HALF_WIDTH, SCREEN_HEIGHT),
        BoxSpec::default(),
        move || {
            HalfFace(HALF_WIDTH, false);
        },
    );
}

/// The half that swings on the hinge, toward the reader. Its picture goes with
/// it and is foreshortened by it; what the fold takes away is the reader's
/// ability to focus on it, not the layout, which never reflows. Past a right
/// angle its screen faces away and the back of the device faces the reader.
#[composable]
fn FoldingHalf(fold: Fold, built: Built) {
    let screen = fold.shows_screen();
    let angle = panel_turn(fold);
    let origin = if screen {
        TransformOrigin::new(1.0, 0.5)
    } else {
        TransformOrigin::new(0.0, 0.5)
    };
    let x = if screen { 0.0 } else { HALF_WIDTH };
    let shut = fold.shut;

    if built == Built::Painted {
        PaintedHalf(fold, screen, x);
        return;
    }

    Box(
        Modifier::empty()
            .absolute_offset(x, 0.0)
            .size_points(HALF_WIDTH, SCREEN_HEIGHT)
            .graphics_layer(move || hinge_layer(angle, origin)),
        BoxSpec::default(),
        move || {
            if screen {
                FoldingScreen(shut);
            } else {
                ShellFace(shut);
            }
        },
    );
}

/// The same half, on a surface that never moves. The shader is given the turn
/// the other device's half really takes and works back, for every fragment,
/// to the point of the flat half that would land there -- so the picture is
/// skewed into the same trapezium -- then blurs and shadows it the same way.
#[composable]
fn PaintedHalf(fold: Fold, screen: bool, x: f32) {
    let shut = fold.shut;
    let turn = panel_turn(fold);
    let height = SCREEN_HEIGHT + PAINT_PAD * 2.0;

    Box(
        Modifier::empty()
            .absolute_offset(x, -PAINT_PAD)
            .required_size(Size::new(HALF_WIDTH, height))
            // The picture grows past the flat half where the turned one comes
            // toward the reader, so the surface being painted has to reach
            // that far. Nothing is drawn here; this only says how far it goes.
            .draw_behind(|scope| {
                scope.draw_rect(Brush::solid(Color(0.0, 0.0, 0.0, 0.0)));
            })
            .graphics_layer(move || {
                if shut <= 0.0 {
                    return GraphicsLayer::default();
                }
                GraphicsLayer {
                    render_effect: Some(paint_effect(&PaintUniforms {
                        fold: shut,
                        turn,
                        hinge_at_left: !screen,
                    })),
                    compositing_strategy: CompositingStrategy::Offscreen,
                    ..Default::default()
                }
            }),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            Box(
                Modifier::empty()
                    .absolute_offset(0.0, PAINT_PAD)
                    .size_points(HALF_WIDTH, SCREEN_HEIGHT),
                BoxSpec::new().content_alignment(Alignment::TOP_START),
                move || {
                    if screen {
                        HalfFace(0.0, true);
                    } else {
                        ShellFace(shut);
                    }
                },
            );
        },
    );
}

/// The screen of the folding half, with the reader's focus taken off it.
#[composable]
fn FoldingScreen(shut: f32) {
    Box(
        Modifier::empty()
            .size_points(HALF_WIDTH, SCREEN_HEIGHT)
            // A half lying flat is square to the reader and carries no
            // gradient, so it goes straight to the screen: no layer of its
            // own, and none of the blur's reads per pixel.
            .graphics_layer(move || {
                if shut <= 0.0 {
                    return GraphicsLayer::default();
                }
                GraphicsLayer {
                    render_effect: Some(glass_effect(&GlassUniforms {
                        fold: shut,
                        hinge_at_left: false,
                    })),
                    compositing_strategy: CompositingStrategy::Offscreen,
                    ..Default::default()
                }
            }),
        BoxSpec::default(),
        move || {
            HalfFace(0.0, true);
        },
    );
}

/// The back of the folding half once its screen has turned past the reader:
/// graphite, with the light running down the edge the hinge holds.
#[composable]
fn ShellFace(shut: f32) {
    let lit = (1.0 - shut * 0.35).clamp(0.0, 1.0);
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
            }),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            Box(
                Modifier::empty()
                    .size_points(HINGE_WIDTH * 0.7, SCREEN_HEIGHT)
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

/// One half of the device: bezel, then a window on to the screen. The picture
/// is drawn from where the window sits in the whole open screen, and what is
/// written on the screen is laid out across all of it and clipped to the
/// window, so both cross the crease unbroken.
#[composable]
fn HalfFace(offset: f32, hinge_at_right: bool) {
    let radii = outer_corners(hinge_at_right);
    let glass = Size::new(
        HALF_WIDTH - BEZEL - INNER_BEZEL,
        SCREEN_HEIGHT - BEZEL * 2.0,
    );
    let glass_x = if hinge_at_right { BEZEL } else { INNER_BEZEL };
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
                    .absolute_offset(glass_x, BEZEL)
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

/// The seam down the middle: the shadow the folding half drops across the
/// half that stays, and the metal the two of them turn on.
#[composable]
fn Hinge(fold: Fold) {
    let squeeze = fold.squeeze();
    let cast = HINGE_WIDTH + CREASE_SHADOW * squeeze;
    let depth = 0.38 * squeeze;

    HingeStrip(
        HALF_WIDTH,
        cast,
        vec![Color(0.0, 0.0, 0.0, depth), Color(0.0, 0.0, 0.0, 0.0)],
    );
    HingeStrip(
        HALF_WIDTH - HINGE_WIDTH * 0.5,
        HINGE_WIDTH,
        vec![
            Color(0.0, 0.0, 0.0, 0.75),
            edge_color(),
            Color(0.0, 0.0, 0.0, 0.75),
        ],
    );
}

/// A strip standing on the hinge line, painted straight across it.
#[composable]
fn HingeStrip(x: f32, width: f32, colours: Vec<Color>) {
    Box(
        Modifier::empty()
            .absolute_offset(x, 0.0)
            .size_points(width, SCREEN_HEIGHT)
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
    let width = SCREEN_WIDTH - BEZEL - INNER_BEZEL;
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
        assert_eq!(Fold::at(0.0).squeeze(), 0.0);
        assert!(Fold::at(0.0).shows_screen());
        assert!((Fold::at(1.0).angle() - SHUT_ANGLE).abs() < 1e-4);
        assert!(
            (Fold::at(1.0).squeeze() - 1.0).abs() < 1e-3,
            "a shut half covers none of the width it covered when flat"
        );
    }

    #[test]
    fn a_fold_never_leaves_the_hinge() {
        assert_eq!(Fold::at(-3.0).shut, 0.0);
        assert_eq!(Fold::at(4.5).shut, 1.0);
    }

    #[test]
    fn the_picture_is_pressed_harder_the_further_the_half_turns() {
        assert!(Fold::at(0.5).squeeze() > Fold::at(0.2).squeeze());
        assert!(Fold::at(1.0).squeeze() > Fold::at(0.5).squeeze());
    }

    #[test]
    fn the_device_stays_where_the_hand_holds_it() {
        assert_eq!(held_still(Fold::at(0.0)), 0.0);
        assert!(held_still(Fold::at(0.5)) < 0.0);
        assert!(
            (held_still(Fold::at(1.0)) + HALF_WIDTH * 0.5).abs() < 1e-3,
            "a shut device has given up a whole half, so it moves back by half of that"
        );
    }

    #[test]
    fn the_turn_the_half_takes_is_the_turn_the_painted_one_is_given() {
        assert_eq!(panel_turn(Fold::at(0.0)), 0.0);
        assert!(
            panel_turn(Fold::at(0.4)) > panel_turn(Fold::at(0.2)),
            "the half swings toward the reader"
        );
        let shut = Fold::at(1.0);
        assert!(!shut.shows_screen());
        assert!(
            (panel_turn(shut) - (SHUT_ANGLE - STRAIGHT_ANGLE)).abs() < 1e-4,
            "past a right angle the back is drawn on the other side of the hinge"
        );
    }

    #[test]
    fn the_back_arrives_at_a_right_angle() {
        assert!(Fold::at(0.5).shows_screen());
        assert!(!Fold::at(0.7).shows_screen());
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
