use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    Box, BoxSpec, Brush, Color, GraphicsLayer, Modifier, Point, TextStyle, TransformOrigin,
};
use cranpose_ui_graphics::{Stroke, VectorPath};

pub(super) use super::super::guide_style::{reader_surface, INK};
pub(super) const MUTED: Color = Color(0.58, 0.67, 0.75, 1.0);
pub(super) const ACCENT: Color = Color(0.45, 0.89, 0.91, 1.0);
pub(super) const BORDER: Color = Color(0.21, 0.33, 0.39, 0.60);

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

#[derive(Clone, Copy, PartialEq)]
pub(super) struct WheelGeometry {
    pub width: f32,
    pub radius: f32,
    height: f32,
    center: Point,
    focus_angle: f32,
}

impl WheelGeometry {
    pub fn new(width: f32, height: f32) -> Self {
        let radius = width.max(780.0) * 0.7;
        let left = (width * 0.25).clamp(200.0, 300.0);
        let center = Point::new(radius + left - 24.0, (height * 0.08).clamp(48.0, 80.0));
        let focus_y = (height * 0.42).clamp(300.0, 360.0);
        Self {
            width,
            radius,
            height: height.max(1.0),
            center,
            focus_angle: ((focus_y - center.y) / (radius + 64.0)).asin(),
        }
    }

    pub fn height(self) -> f32 {
        self.height
    }

    pub fn reader_left(self) -> f32 {
        (self.width * 0.25).clamp(200.0, 300.0)
    }

    pub fn wheel_width(self) -> f32 {
        if self.width < super::COMPACT_BREAKPOINT {
            self.width
        } else {
            self.reader_left()
        }
    }

    pub fn reader_preview_modifier(self, scale_content: bool) -> Modifier {
        let layout_scale = if scale_content { 1.0 } else { 0.44 };
        let content_scale = if scale_content { 0.44 } else { 1.0 };
        Modifier::empty()
            .size_points(self.width * layout_scale, self.height * layout_scale)
            .offset(self.center.x - self.radius + 72.0, self.center.y + 40.0)
            .graphics_layer_value(GraphicsLayer {
                scale_x: content_scale,
                scale_y: content_scale,
                rotation_z: -20.0,
                transform_origin: TransformOrigin::new(0.0, 0.0),
                ..Default::default()
            })
            .clip_to_bounds()
    }

    pub fn section_angle(self) -> f32 {
        120.0 / (self.radius + 64.0)
    }

    pub fn point(self, angle: f32, radius: f32) -> Point {
        Point::new(
            self.center.x - radius * angle.cos(),
            self.center.y + radius * angle.sin(),
        )
    }

    pub fn entry_modifier(self, position: f32, height: f32) -> Modifier {
        self.item_modifier(position, height, 270.0, 64.0)
    }

    pub fn entry_hit_modifier(self, position: f32, height: f32) -> Option<Modifier> {
        let angle = self.focus_angle + position * self.section_angle();
        let (sin, cos) = angle.sin_cos();
        if cos <= 0.0 {
            return None;
        }
        let center = self.point(angle, self.radius + 64.0);
        let inset = sin.abs() * height * 0.5;
        let start = (-135.0_f32).max((inset - center.x) / cos);
        let end = 135.0_f32.min((self.wheel_width() - inset - center.x) / cos);
        if end <= start {
            return None;
        }
        let shift = (start + end) * 0.5;
        let center = Point::new(center.x + shift * cos, center.y - shift * sin);
        Some(Self::placed_item(center, end - start, height, angle))
    }

    pub fn visible_entries(self, position: f32, count: usize) -> std::ops::Range<usize> {
        let radius = self.radius + 64.0;
        let margin = 150.0;
        let first_angle = ((-margin - self.center.y) / radius).clamp(-1.0, 1.0).asin();
        let last_angle = ((self.height + margin - self.center.y) / radius)
            .clamp(-1.0, 1.0)
            .asin();
        let first = (position + (first_angle - self.focus_angle) / self.section_angle()).ceil();
        let last =
            (position + (last_angle - self.focus_angle) / self.section_angle()).floor() + 1.0;
        first.max(0.0) as usize..(last.max(0.0) as usize).min(count)
    }

    pub fn brand_modifier(self, position: f32) -> Modifier {
        self.item_modifier(position, 112.0, 246.0, 24.0)
    }

    fn item_modifier(self, position: f32, height: f32, width: f32, radial_offset: f32) -> Modifier {
        let angle = self.focus_angle + position * self.section_angle();
        let center = self.point(angle, self.radius + radial_offset);
        Self::placed_item(center, width, height, angle)
    }

    fn placed_item(center: Point, width: f32, height: f32, angle: f32) -> Modifier {
        Modifier::empty()
            .size_points(width, height)
            .offset(center.x - width * 0.5, center.y - height * 0.5)
            .graphics_layer_value(GraphicsLayer {
                rotation_z: -angle.to_degrees(),
                ..Default::default()
            })
    }
}

#[composable]
pub(super) fn Backdrop(geometry: WheelGeometry) {
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.025, 0.042, 0.061, 1.0))
            .draw_with_cache(move |cache| {
                let glow = Brush::radial_gradient(
                    vec![Color(0.10, 0.48, 0.52, 0.38), Color(0.035, 0.10, 0.16, 0.0)],
                    Point::new(200.0, 420.0),
                    geometry.radius,
                );
                cache.on_draw_behind(move |scope| scope.draw_rect(glow.clone()));
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
pub(super) fn WheelSurface(geometry: WheelGeometry, position: impl Fn() -> f32 + 'static) {
    Box(
        Modifier::empty()
            .size_points(geometry.width, geometry.height())
            .draw_behind(move |scope| {
                let rotation = position() * geometry.section_angle();
                for inset in [0.0, -22.0, -112.0] {
                    let color = if inset == 0.0 {
                        Color(0.35, 0.86, 0.89, 0.7)
                    } else {
                        Color(0.31, 0.66, 0.77, 0.28)
                    };
                    scope.draw_arc(
                        Brush::solid(color),
                        geometry.center,
                        geometry.radius + inset,
                        0.0,
                        std::f32::consts::TAU,
                        Stroke::new(if inset == 0.0 { 2.0 } else { 1.0 }),
                    );
                }
                let tick_step = geometry.section_angle() / 8.0;
                let first = ((rotation - std::f32::consts::FRAC_PI_2) / tick_step).floor() as i32;
                let count = (std::f32::consts::PI / tick_step).ceil() as i32;
                for index in first..=first + count {
                    let angle = index as f32 * tick_step - rotation;
                    scope.draw_line(
                        Brush::solid(Color(0.50, 0.82, 0.89, 0.45)),
                        geometry.point(angle, geometry.radius - 26.0),
                        geometry.point(
                            angle,
                            geometry.radius - if index % 5 == 0 { 65.0 } else { 38.0 },
                        ),
                        Stroke::new(1.0),
                    );
                }
                let first = ((rotation - geometry.focus_angle - std::f32::consts::FRAC_PI_2)
                    / geometry.section_angle())
                .floor() as i32;
                let count = (std::f32::consts::PI / geometry.section_angle()).ceil() as i32;
                for index in first..=first + count {
                    let angle = geometry.focus_angle
                        + (index as f32 + 0.5) * geometry.section_angle()
                        - rotation;
                    scope.draw_line(
                        Brush::solid(Color(0.31, 0.67, 0.76, 0.25)),
                        geometry.point(angle, geometry.radius + 280.0),
                        geometry.point(angle, geometry.radius - 210.0),
                        Stroke::new(1.0),
                    );
                }
            }),
        BoxSpec::default(),
        || {},
    );
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
