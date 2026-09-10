#![allow(non_snake_case)]

use std::sync::{Arc, OnceLock};

use cranpose_animation::{animateFloatAsState, spring, AnimationType, Spring};
use cranpose_core::{key, rememberMutableStateOf};
use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer,
    LinearArrangement, Modifier, Point, PointerEventKind, PointerInputScope, Row, RowSpec, Text,
    TextStyle, TransformOrigin, VerticalAlignment,
};
use cranpose_ui_graphics::{
    CompositingStrategy, RenderEffect, RuntimeShader, RUNTIME_SHADER_PRELUDE_WGSL,
};

const PAGE_WIDTH: f32 = 288.0;
const PAGE_HEIGHT: f32 = 392.0;
const SPREAD_WIDTH: f32 = PAGE_WIDTH * 2.0;
const PAGE_PADDING: f32 = 26.0;
const CAMERA_DISTANCE: f32 = 18.0;
const STAGE_MARGIN: f32 = 96.0;
const STRAIGHT_ANGLE: f32 = 180.0;
const RIGHT_ANGLE: f32 = 90.0;
const MAX_BLUR_PX: f32 = 9.0;
const CREASE_SHADE: f32 = 0.55;
const EDGE_SHEEN: f32 = 0.22;
const FLING_LEAVES: f32 = 0.28;

/// One printed side of a leaf.
struct Leaf {
    heading: &'static str,
    body: &'static str,
}

static LEAVES: [Leaf; 8] = [
    Leaf {
        heading: "The Turning Page",
        body: "A page is a plane that remembers it was once a solid. Push its outer edge \
               and it leaves the flat world for a moment, sweeping an arc around the spine \
               that holds it.",
    },
    Leaf {
        heading: "Perspective",
        body: "Halfway through the sweep the page is edge on, a bright line and nothing \
               more. A hair past that line the far face arrives, and the sheet you were \
               reading is now the sheet behind you.",
    },
    Leaf {
        heading: "The Crease",
        body: "Light never reaches the gutter the way it reaches the margin. The paper \
               darkens toward the spine because the spine is a room the light has to \
               travel into, and most of it does not.",
    },
    Leaf {
        heading: "Motion",
        body: "The outer edge travels the whole arc while the inner edge barely moves. \
               Blur is not decoration here: it is the difference between those two \
               speeds, drawn.",
    },
    Leaf {
        heading: "Paper",
        body: "Real paper is never quite flat. It keeps a memory of the roll it came \
               from, and it bends before it turns, which is why a page seems to lift \
               before it moves.",
    },
    Leaf {
        heading: "Weight",
        body: "The leaves already turned stack against the cover and hold the ones \
               behind them. A book knows how far through it you are without being \
               told.",
    },
    Leaf {
        heading: "Return",
        body: "Turning back is the same arc walked the other way. Nothing in the \
               geometry prefers a direction; only the reader does.",
    },
    Leaf {
        heading: "End Matter",
        body: "The last leaf falls against the board and the spread stops being a \
               spread. What is left is a cover, and the whole thing is a solid \
               again.",
    },
];

/// Which faces a turn shows, and how far the turning leaf has swung.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Fold {
    leaf: usize,
    angle: f32,
}

impl Fold {
    fn at(turn: f32) -> Self {
        let clamped = turn.clamp(0.0, last_leaf() as f32);
        let leaf = clamped.floor() as usize;
        Self {
            leaf: leaf.min(last_leaf()),
            angle: (clamped - leaf as f32).clamp(0.0, 1.0) * STRAIGHT_ANGLE,
        }
    }

    /// Before the halfway point the front of the turning leaf faces the reader;
    /// after it, the back does.
    fn shows_front(self) -> bool {
        self.angle < RIGHT_ANGLE
    }

    /// How far the leaf is from lying flat, which is what the blur and the
    /// crease shading follow.
    fn bend(self) -> f32 {
        (self.angle.to_radians().sin()).abs()
    }

    fn resting_pages(self) -> (Option<usize>, Option<usize>) {
        (self.leaf.checked_sub(1), Some(self.leaf + 2))
    }
}

/// What the spread announces: which leaf is open, and whether one is moving.
fn spread_reading(fold: Fold) -> String {
    if fold.angle == 0.0 {
        format!("Leaf {} of {}, at rest", fold.leaf + 1, last_leaf() + 1)
    } else {
        format!("Leaf {} of {}, turning", fold.leaf + 1, last_leaf() + 1)
    }
}

fn last_leaf() -> usize {
    LEAVES.len().saturating_sub(2)
}

fn leaf_at(index: usize) -> Option<&'static Leaf> {
    LEAVES.get(index)
}

/// A drag of one page width turns one leaf; dragging left turns forward.
fn turn_after_drag(turn: f32, delta_x: f32) -> f32 {
    (turn - delta_x / PAGE_WIDTH).clamp(0.0, last_leaf() as f32)
}

/// Where a released page settles: past the halfway point, or carried over it by
/// a fling.
fn settled_turn(turn: f32, leaves_per_second: f32) -> f32 {
    let leaf = turn.floor();
    let progress = turn - leaf;
    let carried = if leaves_per_second <= -FLING_LEAVES {
        0.0
    } else if leaves_per_second >= FLING_LEAVES || progress >= 0.5 {
        1.0
    } else {
        0.0
    };
    (leaf + carried).clamp(0.0, last_leaf() as f32)
}

fn paper_color(dark: bool) -> Color {
    if dark {
        Color(0.128, 0.126, 0.122, 1.0)
    } else {
        Color(0.968, 0.960, 0.941, 1.0)
    }
}

fn page_back_color(dark: bool) -> Color {
    if dark {
        Color(0.104, 0.102, 0.100, 1.0)
    } else {
        Color(0.930, 0.920, 0.898, 1.0)
    }
}

fn desk_color(dark: bool) -> Color {
    if dark {
        Color(0.047, 0.047, 0.052, 1.0)
    } else {
        Color(0.836, 0.828, 0.812, 1.0)
    }
}

fn ink_color(dark: bool) -> Color {
    if dark {
        Color(0.88, 0.87, 0.85, 1.0)
    } else {
        Color(0.13, 0.12, 0.11, 1.0)
    }
}

fn muted_ink(dark: bool) -> Color {
    if dark {
        Color(0.58, 0.57, 0.55, 1.0)
    } else {
        Color(0.42, 0.41, 0.39, 1.0)
    }
}

fn heading_style(dark: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(ink_color(dark)),
            font_size: TextUnit::Sp(21.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn body_style(dark: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(ink_color(dark)),
            font_size: TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn folio_style(dark: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(muted_ink(dark)),
            font_size: TextUnit::Sp(12.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn caption_style(dark: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(muted_ink(dark)),
            font_size: TextUnit::Sp(13.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn title_style(dark: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(ink_color(dark)),
            font_size: TextUnit::Sp(28.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn fold_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    SOURCE
        .get_or_init(|| {
            Arc::<str>::from(format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}{}",
                include_str!("page_fold.wgsl")
            ))
        })
        .clone()
}

struct FoldUniforms {
    spine_at_left: bool,
    bend: f32,
    blur: f32,
    shade: f32,
    sheen: f32,
}

fn fold_effect(uniforms: &FoldUniforms) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(fold_wgsl());
    shader.set_float(0, if uniforms.spine_at_left { 1.0 } else { 0.0 });
    shader.set_float(1, uniforms.bend);
    shader.set_float(2, uniforms.blur);
    shader.set_float(3, uniforms.shade);
    shader.set_float(4, uniforms.sheen);
    RenderEffect::runtime_shader(shader)
}

#[composable]
pub(crate) fn PageFoldTab() {
    let dark = rememberMutableStateOf(|| false);
    let turn = rememberMutableStateOf(|| 0.0f32);
    let dragging = rememberMutableStateOf(|| false);
    let is_dark = dark.get();

    let animated =
        animateFloatAsState(turn.get(), turn_animation(dragging.get()), "page_fold_turn");
    let fold = Fold::at(animated.value());

    Column(
        Modifier::empty()
            .fill_max_width()
            .background(desk_color(is_dark))
            .padding_symmetric(2.0, 6.0),
        ColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(14.0))
            .horizontal_alignment(cranpose_ui::HorizontalAlignment::CenterHorizontally),
        move || {
            FoldHeader(dark, is_dark, fold);
            Spread(fold, is_dark, turn, dragging);
            Text(
                "Drag across the spread to turn a leaf. Let go past halfway, or flick, and it \
                 falls the rest of the way.",
                Modifier::empty().padding_symmetric(24.0, 0.0),
                caption_style(is_dark),
            );
        },
    );
}

fn turn_animation(dragging: bool) -> AnimationType {
    if dragging {
        spring(Spring::DampingRatioNoBouncy, Spring::StiffnessHigh)
    } else {
        spring(Spring::DampingRatioLowBouncy, Spring::StiffnessMediumLow)
    }
}

#[composable]
fn FoldHeader(dark: cranpose_core::MutableState<bool>, is_dark: bool, fold: Fold) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_symmetric(8.0, 4.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text("Page Fold", Modifier::empty(), title_style(is_dark));
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(14.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        format!("Leaf {} of {}", fold.leaf + 1, last_leaf() + 1),
                        Modifier::empty(),
                        caption_style(is_dark),
                    );
                    ThemeSwitch(dark, is_dark);
                },
            );
        },
    );
}

#[composable]
fn ThemeSwitch(dark: cranpose_core::MutableState<bool>, is_dark: bool) {
    let label = if is_dark { "Daylight" } else { "Lamplight" };
    let background = if is_dark {
        Color(1.0, 1.0, 1.0, 0.10)
    } else {
        Color(0.0, 0.0, 0.0, 0.06)
    };

    Box(
        Modifier::empty()
            .rounded_corners(15.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(background), CornerRadii::uniform(15.0));
            })
            .toggleable(
                is_dark,
                Some(format!("Lamplight, {}", if is_dark { "on" } else { "off" })),
                Some(cranpose_foundation::SemanticsWidgetRole::Switch),
                move |next| dark.set(next),
            )
            .padding_symmetric(14.0, 8.0),
        BoxSpec::default(),
        move || {
            Text(label, Modifier::empty(), caption_style(is_dark));
        },
    );
}

#[composable]
fn Spread(
    fold: Fold,
    dark: bool,
    turn: cranpose_core::MutableState<f32>,
    dragging: cranpose_core::MutableState<bool>,
) {
    let (left, right) = fold.resting_pages();

    let reading = spread_reading(fold);

    Box(
        Modifier::empty()
            .size_points(
                SPREAD_WIDTH + STAGE_MARGIN * 2.0,
                PAGE_HEIGHT + STAGE_MARGIN,
            )
            .clip_to_bounds()
            .semantics(move |config: &mut SemanticsConfiguration| {
                config.content_description = Some("Book spread".to_string());
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
                                    started = turn.get();
                                    travelled = 0.0;
                                    dragging.set(true);
                                    event.consume();
                                }
                                PointerEventKind::Move => {
                                    if dragging.get() {
                                        travelled += event.position.x - last.x;
                                        last = event.position;
                                        turn.set(turn_after_drag(started, travelled));
                                        event.consume();
                                    }
                                }
                                PointerEventKind::Up | PointerEventKind::Cancel => {
                                    if dragging.get() {
                                        dragging.set(false);
                                        let flick = -(event.position.x - last.x) / PAGE_WIDTH;
                                        turn.set(settled_turn(turn.get(), flick));
                                    }
                                }
                                PointerEventKind::Exit => {
                                    if dragging.get() {
                                        dragging.set(false);
                                        turn.set(settled_turn(turn.get(), 0.0));
                                    }
                                }
                                _ => {}
                            }
                        }
                    })
                    .await;
            }),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            key("resting", || {
                RestingPage(left, STAGE_MARGIN, dark, false);
                RestingPage(right, STAGE_MARGIN + PAGE_WIDTH, dark, true);
            });
            GutterShadow(fold, dark);
            key("turning", || TurningLeaf(fold, dark));
        },
    );
}

#[composable]
fn RestingPage(index: Option<usize>, x: f32, dark: bool, front: bool) {
    let colour = if front {
        paper_color(dark)
    } else {
        page_back_color(dark)
    };
    Box(
        Modifier::empty()
            .absolute_offset(x, STAGE_MARGIN * 0.5)
            .size_points(PAGE_WIDTH, PAGE_HEIGHT)
            .draw_behind(move |scope| {
                scope.draw_rect(Brush::solid(colour));
            }),
        BoxSpec::default(),
        move || {
            PageContent(index, dark);
        },
    );
}

/// The dark seam the turning leaf casts into the gutter as it lifts.
#[composable]
fn GutterShadow(fold: Fold, dark: bool) {
    let bend = fold.bend();
    let width = 26.0 + 96.0 * bend;
    let depth = (0.34 + 0.30 * bend) * if dark { 0.75 } else { 1.0 };
    let leaving = fold.shows_front();

    Box(
        Modifier::empty()
            .absolute_offset(
                STAGE_MARGIN
                    + if leaving {
                        PAGE_WIDTH
                    } else {
                        PAGE_WIDTH - width
                    },
                STAGE_MARGIN * 0.5,
            )
            .size_points(width, PAGE_HEIGHT)
            .draw_behind(move |scope| {
                let colours = if leaving {
                    vec![Color(0.0, 0.0, 0.0, depth), Color(0.0, 0.0, 0.0, 0.0)]
                } else {
                    vec![Color(0.0, 0.0, 0.0, 0.0), Color(0.0, 0.0, 0.0, depth)]
                };
                let size = scope.size();
                scope.draw_rect(Brush::linear_gradient_range(
                    colours,
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

#[composable]
fn TurningLeaf(fold: Fold, dark: bool) {
    let front = fold.shows_front();
    let index = if front { fold.leaf } else { fold.leaf + 1 };
    let angle = if front {
        -fold.angle
    } else {
        STRAIGHT_ANGLE - fold.angle
    };
    let origin = if front {
        TransformOrigin::new(0.0, 0.5)
    } else {
        TransformOrigin::new(1.0, 0.5)
    };
    let x = STAGE_MARGIN + if front { PAGE_WIDTH } else { 0.0 };
    let bend = fold.bend();
    let colour = if front {
        paper_color(dark)
    } else {
        page_back_color(dark)
    };

    Box(
        Modifier::empty()
            .absolute_offset(x, STAGE_MARGIN * 0.5)
            .size_points(PAGE_WIDTH, PAGE_HEIGHT)
            .graphics_layer(move || GraphicsLayer {
                rotation_y: angle,
                camera_distance: CAMERA_DISTANCE,
                transform_origin: origin,
                ..Default::default()
            }),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size_points(PAGE_WIDTH, PAGE_HEIGHT)
                    .draw_behind(move |scope| {
                        scope.draw_rect(Brush::solid(colour));
                    })
                    .graphics_layer(move || GraphicsLayer {
                        render_effect: Some(fold_effect(&FoldUniforms {
                            spine_at_left: front,
                            bend,
                            blur: MAX_BLUR_PX,
                            shade: CREASE_SHADE,
                            sheen: EDGE_SHEEN,
                        })),
                        compositing_strategy: CompositingStrategy::Offscreen,
                        ..Default::default()
                    }),
                BoxSpec::default(),
                move || {
                    PageContent(leaf_index(index), dark);
                },
            );
        },
    );
}

fn leaf_index(index: usize) -> Option<usize> {
    if index < LEAVES.len() {
        Some(index)
    } else {
        None
    }
}

#[composable]
fn PageContent(index: Option<usize>, dark: bool) {
    let Some(leaf) = index.and_then(leaf_at) else {
        return;
    };
    let folio = index.unwrap_or(0) + 1;

    Column(
        Modifier::empty()
            .size_points(PAGE_WIDTH, PAGE_HEIGHT)
            .padding(PAGE_PADDING),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(14.0)),
        move || {
            Text(leaf.heading, Modifier::empty(), heading_style(dark));
            Text(leaf.body, Modifier::empty(), body_style(dark));
            Text(format!("— {folio} —"), Modifier::empty(), folio_style(dark));
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fold_reads_its_leaf_and_angle_from_the_turn() {
        let flat = Fold::at(0.0);
        assert_eq!(flat.leaf, 0);
        assert_eq!(flat.angle, 0.0);
        assert!(flat.shows_front());
        assert_eq!(flat.bend(), 0.0);

        let quarter = Fold::at(0.25);
        assert!((quarter.angle - 45.0).abs() < 1e-4);
        assert!(quarter.shows_front());

        let past_half = Fold::at(0.75);
        assert!((past_half.angle - 135.0).abs() < 1e-4);
        assert!(
            !past_half.shows_front(),
            "the back face arrives past 90 degrees"
        );
    }

    #[test]
    fn a_leaf_bends_most_when_it_stands_on_edge() {
        assert!((Fold::at(0.5).bend() - 1.0).abs() < 1e-4);
        assert!(Fold::at(0.5).bend() > Fold::at(0.2).bend());
        assert!(Fold::at(0.5).bend() > Fold::at(0.9).bend());
        assert!(Fold::at(1.0).bend() < 1e-4, "a leaf lying flat has no bend");
    }

    #[test]
    fn a_fold_never_leaves_the_book() {
        assert_eq!(Fold::at(-4.0).leaf, 0);
        assert_eq!(Fold::at(-4.0).angle, 0.0);
        let end = Fold::at(last_leaf() as f32 + 3.0);
        assert_eq!(end.leaf, last_leaf());
        assert_eq!(end.angle, 0.0);
    }

    #[test]
    fn the_spread_announces_its_leaf_and_whether_one_is_moving() {
        assert_eq!(spread_reading(Fold::at(0.0)), "Leaf 1 of 7, at rest");
        assert_eq!(spread_reading(Fold::at(0.4)), "Leaf 1 of 7, turning");
        assert_eq!(spread_reading(Fold::at(2.0)), "Leaf 3 of 7, at rest");
    }

    #[test]
    fn the_resting_pages_sit_either_side_of_the_turning_leaf() {
        assert_eq!(Fold::at(0.3).resting_pages(), (None, Some(2)));
        assert_eq!(Fold::at(2.4).resting_pages(), (Some(1), Some(4)));
    }

    #[test]
    fn dragging_left_turns_forward_and_stops_at_the_covers() {
        assert!((turn_after_drag(0.0, -PAGE_WIDTH) - 1.0).abs() < 1e-4);
        assert!((turn_after_drag(1.0, PAGE_WIDTH * 0.5) - 0.5).abs() < 1e-4);
        assert_eq!(turn_after_drag(0.0, PAGE_WIDTH * 3.0), 0.0);
        assert_eq!(
            turn_after_drag(last_leaf() as f32, -PAGE_WIDTH * 3.0),
            last_leaf() as f32
        );
    }

    #[test]
    fn a_released_leaf_settles_where_it_was_headed() {
        assert_eq!(settled_turn(1.2, 0.0), 1.0);
        assert_eq!(settled_turn(1.6, 0.0), 2.0);
        assert_eq!(
            settled_turn(1.2, 1.0),
            2.0,
            "a flick carries a leaf it had barely lifted"
        );
        assert_eq!(
            settled_turn(1.8, -1.0),
            1.0,
            "a flick back drops a leaf that had almost landed"
        );
        assert_eq!(
            settled_turn(0.1, -1.0),
            0.0,
            "the first leaf cannot go back"
        );
    }

    #[test]
    fn every_leaf_of_the_book_is_printed() {
        for leaf in 0..LEAVES.len() {
            assert!(leaf_at(leaf).is_some());
            assert!(!LEAVES[leaf].heading.is_empty());
            assert!(LEAVES[leaf].body.len() > 40);
        }
        assert!(leaf_at(LEAVES.len()).is_none());
        assert_eq!(leaf_index(LEAVES.len()), None);
        assert_eq!(leaf_index(0), Some(0));
    }
}
