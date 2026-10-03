use cranpose::liquid::{Glass, LiquidModifierExt, LiquidShape};
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    Box, BoxSpec, Brush, Color, GraphicsLayer, Modifier, Point, TextStyle,
};
use cranpose_ui_graphics::{Stroke, TransformOrigin, VectorPath};

pub(super) const INK: Color = Color(0.91, 0.95, 0.98, 1.0);
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
        Modifier::empty()
            .size_points(width, height)
            .offset(center.x - width * 0.5, center.y - height * 0.5)
            .graphics_layer_value(GraphicsLayer {
                rotation_z: -angle.to_degrees(),
                ..Default::default()
            })
    }

    pub fn rotation_layer(self, position: f32) -> GraphicsLayer {
        GraphicsLayer {
            rotation_z: (position * self.section_angle()).to_degrees(),
            transform_origin: TransformOrigin::new(
                self.center.x / self.width,
                self.center.y / self.height,
            ),
            ..Default::default()
        }
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
pub(super) fn WheelSurface(geometry: WheelGeometry) {
    Box(
        Modifier::empty()
            .size_points(geometry.width, geometry.height())
            .draw_with_cache(move |cache| {
                let start = geometry.focus_angle - 0.8;
                let sweep = 3.4;
                let ticks: Vec<_> = (0..136)
                    .map(|index| {
                        let angle = start + sweep * index as f32 / 136.0;
                        (
                            geometry.point(angle, geometry.radius - 26.0),
                            geometry.point(
                                angle,
                                geometry.radius - if index % 5 == 0 { 65.0 } else { 38.0 },
                            ),
                        )
                    })
                    .collect();
                let spokes: Vec<_> = (-2..super::chapters().len() as i32)
                    .map(|index| {
                        let angle =
                            geometry.focus_angle + (index as f32 + 0.5) * geometry.section_angle();
                        (
                            geometry.point(angle, geometry.radius + 280.0),
                            geometry.point(angle, geometry.radius - 210.0),
                        )
                    })
                    .collect();
                cache.on_draw_behind(move |scope| {
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
                            std::f32::consts::PI - start,
                            -sweep,
                            Stroke::new(if inset == 0.0 { 2.0 } else { 1.0 }),
                        );
                    }
                    for &(from, to) in &ticks {
                        scope.draw_line(
                            Brush::solid(Color(0.50, 0.82, 0.89, 0.45)),
                            from,
                            to,
                            Stroke::new(1.0),
                        );
                    }
                    for &(from, to) in &spokes {
                        scope.draw_line(
                            Brush::solid(Color(0.31, 0.67, 0.76, 0.25)),
                            from,
                            to,
                            Stroke::new(1.0),
                        );
                    }
                });
            }),
        BoxSpec::default(),
        || {},
    );
}

pub(super) fn reader_surface(modifier: Modifier) -> Modifier {
    let mut glass = Glass::clear();
    glass.shape = LiquidShape::RoundedRect(0.0);
    glass.tint = Some(Color(0.045, 0.085, 0.12, 0.24));
    glass.blur_radius = Some(0.8);
    glass.dispersion = 1.0;
    glass.lift = Some(0.0);
    glass.contrast = Some(1.0);
    glass.saturation = Some(1.0);
    glass.refraction_depth_dp = Some(12.0);
    glass.transmission_refraction = 0.2;
    glass.foreground = Some(INK);
    glass.adaptive_frost = 0.0;
    glass.highlight = 0.25;
    glass.rim_reflection = 0.2;
    glass.face_lighting = false;
    glass.shadow = false;
    modifier.glass_effect(glass)
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
