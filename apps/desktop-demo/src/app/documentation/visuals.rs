use cranpose::liquid::{Glass, LiquidModifierExt, LiquidShape};
use cranpose_animation::{Animatable, AnimationType, SpringSpec};
use cranpose_core::{remember, rememberMutableStateOf, LaunchedEffect};
use cranpose_foundation::lazy::LazyListState;
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    Box, BoxSpec, Brush, Color, Modifier, Point, ScrollState, TextStyle,
};
use cranpose_ui_graphics::{DrawScope, Stroke, VectorPath};

pub(super) const INK: Color = Color(0.91, 0.95, 0.98, 1.0);
pub(super) const MUTED: Color = Color(0.58, 0.67, 0.75, 1.0);
pub(super) const ACCENT: Color = Color(0.45, 0.89, 0.91, 1.0);
pub(super) const PAPER: Color = Color(0.040, 0.061, 0.089, 0.97);
pub(super) const BORDER: Color = Color(0.21, 0.33, 0.39, 0.60);
pub(super) const SELECTED: Color = Color(0.10, 0.28, 0.33, 0.92);
pub(super) const CARD: Color = Color(0.07, 0.13, 0.18, 0.34);

const GITHUB_MARK: &str = "M10.226 17.284c-2.965-.36-5.054-2.493-5.054-5.256 0-1.123.404-2.336 1.078-3.144-.292-.741-.247-2.314.09-2.965.898-.112 2.111.36 2.83 1.01.853-.269 1.752-.404 2.853-.404 1.1 0 1.999.135 2.807.382.696-.629 1.932-1.1 2.83-.988.315.606.36 2.179.067 2.942.72.854 1.101 2 1.101 3.167 0 2.763-2.089 4.852-5.098 5.234.763.494 1.28 1.572 1.28 2.807v2.336c0 .674.561 1.056 1.235.786 4.066-1.55 7.255-5.615 7.255-10.646C23.5 6.188 18.334 1 11.978 1 5.62 1 .5 6.188.5 12.545c0 4.986 3.167 9.12 7.435 10.669.606.225 1.19-.18 1.19-.786V20.63a2.9 2.9 0 0 1-1.078.224c-1.483 0-2.359-.808-2.987-2.313-.247-.607-.517-.966-1.034-1.033-.27-.023-.359-.135-.359-.27 0-.27.45-.471.898-.471.652 0 1.213.404 1.797 1.235.45.651.921.943 1.483.943.561 0 .92-.202 1.437-.719.382-.381.674-.718.944-.943";

pub(super) fn text_style(size: f32, color: Color, weight: FontWeight) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            font_weight: Some(weight),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(super) fn caption_style(color: Color) -> TextStyle {
    let mut style = text_style(10.0, color, FontWeight::BOLD);
    style.span_style.letter_spacing = TextUnit::Sp(1.6);
    style
}

#[composable]
pub(super) fn Backdrop() {
    let modifier = Modifier::empty()
        .fill_max_size()
        .background(Color(0.026, 0.042, 0.067, 1.0))
        .draw_with_cache(|cache| {
            let cyan = Brush::radial_gradient(
                vec![Color(0.06, 0.42, 0.47, 0.62), Color(0.02, 0.10, 0.17, 0.0)],
                Point::new(105.0, 190.0),
                370.0,
            );
            let violet = Brush::radial_gradient(
                vec![Color(0.37, 0.16, 0.45, 0.40), Color(0.10, 0.03, 0.17, 0.0)],
                Point::new(230.0, 660.0),
                360.0,
            );
            cache.on_draw_behind(move |scope| {
                scope.draw_rect(cyan.clone());
                scope.draw_rect(violet.clone());
                let size = scope.size();
                for column in 0..((size.width / 64.0).ceil() as usize).min(40) {
                    for row in 0..((size.height / 64.0).ceil() as usize).min(30) {
                        scope.draw_circle(
                            Brush::solid(Color(0.55, 0.75, 0.80, 0.09)),
                            Point::new(column as f32 * 64.0 + 24.0, row as f32 * 64.0 + 24.0),
                            0.85,
                        );
                    }
                }
            });
        });
    Box(modifier, BoxSpec::default(), || {});
}

#[composable]
pub(super) fn GlassPane(modifier: Modifier) {
    let mut glass = Glass::regular();
    glass.shape = LiquidShape::RoundedRect(38.0);
    glass.tint = Some(Color(0.08, 0.17, 0.23, 0.42));
    glass.blur_radius = Some(18.0);
    glass.refraction_depth_dp = Some(4.0);
    glass.transmission_refraction = 0.18;
    glass.foreground = Some(INK);
    glass.highlight = 0.45;
    glass.rim_reflection = 0.45;
    glass.face_lighting = false;
    glass.shadow = false;
    Box(modifier.glass_effect(glass), BoxSpec::default(), || {});
}

#[composable]
pub(super) fn RepositoryIcon(modifier: Modifier) {
    Box(
        modifier.draw_with_cache(|cache| {
            let path = VectorPath::parse(GITHUB_MARK)
                .expect("bundled GitHub mark is valid vector data")
                .scaled(21.0 / 24.0);
            cache.on_draw_behind(move |scope| {
                scope.draw_vector_path(&path, Brush::solid(INK));
            });
        }),
        BoxSpec::default(),
        || {},
    );
}

fn edge_point(y: f32, height: f32) -> Point {
    let distance = (y / height.max(1.0) * 2.0 - 1.0).clamp(-1.0, 1.0);
    Point::new(10.0 + 11.0 * distance * distance, y)
}

fn draw_edge(scope: &mut dyn DrawScope, energy: f32, phase: f32) {
    let height = scope.size().height;
    let top = 34.0_f32.min(height * 0.2);
    let extent = (height - top * 2.0).max(0.0);
    for segment in 0..32 {
        let first = edge_point(top + extent * segment as f32 / 32.0, height);
        let second = edge_point(top + extent * (segment + 1) as f32 / 32.0, height);
        scope.draw_line(
            Brush::solid(Color(0.54, 0.88, 0.91, 0.20)),
            first,
            second,
            Stroke::new(1.0),
        );
    }
    if energy <= 0.01 {
        return;
    }
    for bolt in 0..4 {
        let position = (phase * 0.037 + bolt as f32 * 0.239).rem_euclid(1.0);
        let start = edge_point(top + extent * (0.1 + position * 0.78), height);
        let points = [
            start,
            Point::new(start.x - 4.0, start.y + 7.0),
            Point::new(start.x + 3.0, start.y + 13.0),
            Point::new(start.x - 5.0, start.y + 20.0),
            Point::new(start.x + 1.0, start.y + 28.0),
        ];
        for pair in points.windows(2) {
            scope.draw_line(
                Brush::solid(Color(0.75, 0.93, 0.48, 0.07 * energy)),
                pair[0],
                pair[1],
                Stroke::new(7.0),
            );
            scope.draw_line(
                Brush::solid(Color(0.93, 0.99, 0.68, 0.82 * energy)),
                pair[0],
                pair[1],
                Stroke::new(1.35),
            );
        }
    }
}

#[composable]
pub(super) fn ElectricEdge(document: LazyListState, wheel: ScrollState, modifier: Modifier) {
    let position = document.first_visible_item_index() as f32 * 100.0
        + document.first_visible_item_scroll_offset()
        + wheel.value();
    let reduce_motion = cranpose_services::local_accessibility_options()
        .current()
        .reduce_motion;
    let mounted = rememberMutableStateOf(|| false);
    let pulse = remember(|| {
        let runtime = cranpose_core::runtime::current_runtime_handle()
            .expect("documentation is composed inside a runtime");
        Animatable::new(0.0_f32, runtime)
    });
    let energy = pulse.with(Animatable::state);
    LaunchedEffect((position.to_bits(), reduce_motion), move |_| {
        if !mounted.get() {
            mounted.set(true);
            return;
        }
        pulse.update(|value| {
            value.snapTo(if reduce_motion { 0.0 } else { 1.0 });
            if !reduce_motion {
                value.animateTo(0.0, AnimationType::Spring(SpringSpec::new(1.0, 220.0)));
            }
        });
    });
    Box(
        modifier.draw_behind(move |scope| draw_edge(scope, energy.value(), position)),
        BoxSpec::default(),
        || {},
    );
}
