use super::count_primitive;
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawScope, Point, Rect, Stroke, StrokeCap, TileMode,
};

const DOT_SPACING: f32 = 0.45;
const FALLOFF_BANDS: usize = 8;
const FALLOFF_FLOOR: f32 = 0.0008;
const FLAT_ARC_RADII: f32 = 64.0;

pub struct Painter<'a> {
    scope: &'a mut dyn DrawScope,
    pub center_x: f32,
    pub center_y: f32,
    pub radius: f32,
}

impl<'a> Painter<'a> {
    pub fn new(scope: &'a mut dyn DrawScope, center_x: f32, center_y: f32, radius: f32) -> Self {
        Self {
            scope,
            center_x,
            center_y,
            radius,
        }
    }

    #[inline]
    pub(crate) fn emit(&mut self) -> &mut dyn DrawScope {
        count_primitive();
        self.scope
    }

    #[inline]
    pub(crate) fn scope(&self) -> &dyn DrawScope {
        self.scope
    }

    #[inline]
    pub fn px(&self, unit_x: f32) -> f32 {
        self.center_x + unit_x * self.radius
    }

    #[inline]
    pub fn py(&self, unit_y: f32) -> f32 {
        self.center_y + unit_y * self.radius
    }

    #[inline]
    pub fn scale(&self, unit: f32) -> f32 {
        unit * self.radius
    }

    pub fn fill_screen(&mut self, color: Color) {
        self.emit().draw_rect(Brush::solid(color));
    }

    pub fn dot(&mut self, unit_x: f32, unit_y: f32, unit_radius: f32, color: Color) {
        if color.3 <= 0.002 || unit_radius <= 0.0 {
            return;
        }
        let center = Point::new(self.px(unit_x), self.py(unit_y));
        let pixels = self.scale(unit_radius);
        self.emit().draw_circle(Brush::solid(color), center, pixels);
    }

    pub fn disc(&mut self, unit_x: f32, unit_y: f32, unit_radius: f32, brush: Brush) {
        if unit_radius <= 0.0 {
            return;
        }
        let center = Point::new(self.px(unit_x), self.py(unit_y));
        let pixels = self.scale(unit_radius);
        self.emit().draw_circle(brush, center, pixels);
    }

    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        if color.3 <= 0.002 || width <= 0.0 || height <= 0.0 {
            return;
        }
        self.emit().draw_rect_at(
            Rect {
                x,
                y,
                width,
                height,
            },
            Brush::solid(color),
        );
    }

    pub fn round_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        corner: f32,
        color: Color,
    ) {
        if color.3 <= 0.002 || width <= 0.0 || height <= 0.0 {
            return;
        }
        self.emit().draw_round_rect_at(
            Rect {
                x,
                y,
                width,
                height,
            },
            Brush::solid(color),
            CornerRadii::uniform(corner.min(width.min(height) * 0.5)),
        );
    }

    pub fn unit_rect(&mut self, ux: f32, uy: f32, uw: f32, uh: f32, color: Color) {
        let x = self.px(ux);
        let y = self.py(uy);
        self.rect(x, y, self.scale(uw), self.scale(uh), color);
    }

    /// A glow that fades outwards, as the stack of circles the Compose
    /// original stacks.
    ///
    /// `ArenaRenderer.drawFalloff` paints eight translucent filled circles of
    /// shrinking radius, and the stack is not interchangeable with its own
    /// analytic profile: each circle is composited separately against an
    /// eight-bit destination, so the rounding happens eight times over. A
    /// single radial gradient reproduces the profile and not the rounding,
    /// which is an off-by-one across every glow in the frame -- and this game
    /// is mostly glows. The picture has to be built from the same primitives.
    pub fn falloff(&mut self, unit_x: f32, unit_y: f32, unit_radius: f32, color: Color, peak: f32) {
        if unit_radius <= 0.0 || peak <= FALLOFF_FLOOR {
            return;
        }
        for index in 0..FALLOFF_BANDS {
            let step = (index as f32 + 0.5) / FALLOFF_BANDS as f32;
            let alpha = peak * step * step;
            if alpha <= FALLOFF_FLOOR {
                continue;
            }
            // Straight to the scope rather than through `dot`, whose alpha
            // guard is a hair above Compose's `FALLOFF_FLOOR` and would drop
            // the faintest band. Invisible over black, but it still multiplies
            // a bright destination down by a level.
            let center = Point::new(self.px(unit_x), self.py(unit_y));
            let pixels = self.scale(unit_radius * (1.0 - step));
            self.emit()
                .draw_circle(Brush::solid(color.with_alpha(alpha)), center, pixels);
        }
    }

    /// The same glow turned inside out -- brightest at the rim, fading towards
    /// the middle -- which Compose paints as eight stroked rings rather than
    /// eight discs.
    pub fn inverse_falloff(
        &mut self,
        unit_x: f32,
        unit_y: f32,
        unit_radius: f32,
        color: Color,
        peak: f32,
    ) {
        if unit_radius <= 0.0 || peak <= FALLOFF_FLOOR {
            return;
        }
        let band = unit_radius / FALLOFF_BANDS as f32;
        for index in 0..FALLOFF_BANDS {
            let outer = 1.0 - index as f32 / FALLOFF_BANDS as f32;
            let alpha = peak * outer * outer;
            if alpha <= FALLOFF_FLOOR {
                continue;
            }
            let center = Point::new(self.px(unit_x), self.py(unit_y));
            let radius = self.scale(unit_radius * outer - band * 0.5);
            let stroke = Stroke::new(self.scale(band));
            self.emit().draw_circle_stroked(
                Brush::solid(color.with_alpha(alpha)),
                center,
                radius,
                stroke,
            );
        }
    }

    pub fn radial_disc(
        &mut self,
        unit_x: f32,
        unit_y: f32,
        unit_radius: f32,
        inner: Color,
        outer: Color,
    ) {
        if unit_radius <= 0.0 {
            return;
        }
        let center = Point::new(self.px(unit_x), self.py(unit_y));
        let pixels = self.scale(unit_radius);
        self.emit().draw_circle(
            Brush::radial_gradient_tiled(
                vec![inner, outer],
                Point::new(pixels, pixels),
                pixels,
                TileMode::Clamp,
            ),
            center,
            pixels,
        );
    }

    pub fn ring(&mut self, unit_radius: f32, unit_thickness: f32, color: Color) {
        self.ring_at(0.0, 0.0, unit_radius, unit_thickness, color);
    }

    pub fn ring_at(
        &mut self,
        unit_x: f32,
        unit_y: f32,
        unit_radius: f32,
        unit_thickness: f32,
        color: Color,
    ) {
        if unit_radius <= 0.0 || unit_thickness <= 0.0 || color.3 <= 0.002 {
            return;
        }
        let center = Point::new(self.px(unit_x), self.py(unit_y));
        let radius = self.scale(unit_radius);
        let stroke = Stroke::new(self.scale(unit_thickness));
        self.emit()
            .draw_circle_stroked(Brush::solid(color), center, radius, stroke);
    }

    pub fn arc(
        &mut self,
        unit_radius: f32,
        start_angle: f32,
        sweep: f32,
        unit_thickness: f32,
        cap: StrokeCap,
        color: Color,
    ) {
        self.arc_at(
            0.0,
            0.0,
            unit_radius,
            start_angle,
            sweep,
            unit_thickness,
            cap,
            color,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn arc_at(
        &mut self,
        unit_x: f32,
        unit_y: f32,
        unit_radius: f32,
        start_angle: f32,
        sweep: f32,
        unit_thickness: f32,
        cap: StrokeCap,
        color: Color,
    ) {
        if unit_radius <= 0.0 || unit_thickness <= 0.0 || color.3 <= 0.002 {
            return;
        }
        if sweep.abs() <= 1e-5 {
            return;
        }
        let center = Point::new(self.px(unit_x), self.py(unit_y));
        let radius = self.scale(unit_radius);
        let stroke = Stroke::new(self.scale(unit_thickness)).with_cap(cap);
        self.emit().draw_arc(
            Brush::solid(color),
            center,
            radius,
            start_angle,
            sweep,
            stroke,
        );
    }

    pub fn annular_sector(
        &mut self,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep: f32,
        color: Color,
    ) {
        if outer_radius <= inner_radius || color.3 <= 0.002 {
            return;
        }
        if sweep.abs() <= 1e-5 {
            return;
        }
        let center = Point::new(self.center_x, self.center_y);
        let inner = self.scale(inner_radius.max(0.0));
        let outer = self.scale(outer_radius);
        self.emit().draw_annular_sector(
            Brush::solid(color),
            center,
            inner,
            outer,
            start_angle,
            sweep,
        );
    }

    pub fn sector_outline(
        &mut self,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep: f32,
        unit_thickness: f32,
        color: Color,
    ) {
        if outer_radius <= inner_radius || unit_thickness <= 0.0 || color.3 <= 0.002 {
            return;
        }
        if sweep.abs() <= 1e-5 {
            return;
        }
        let inner = inner_radius.max(1e-4);
        let outer = outer_radius;
        let half = unit_thickness * 0.5;
        let (start, span) = if sweep < 0.0 {
            (start_angle + sweep, -sweep)
        } else {
            (start_angle, sweep)
        };
        let inner_pad = half / inner;
        let outer_pad = half / outer;
        self.arc(
            inner,
            start - inner_pad,
            span + inner_pad * 2.0,
            unit_thickness,
            StrokeCap::Butt,
            color,
        );
        self.arc(
            outer,
            start - outer_pad,
            span + outer_pad * 2.0,
            unit_thickness,
            StrokeCap::Butt,
            color,
        );
        let band_inner = inner + half;
        let band_outer = outer - half;
        if band_outer <= band_inner {
            return;
        }
        let edge_sweep = unit_thickness / ((band_inner + band_outer) * 0.5);
        for edge in [start, start + span] {
            self.annular_sector(
                band_inner,
                band_outer,
                edge - edge_sweep * 0.5,
                edge_sweep,
                color,
            );
        }
    }

    pub fn dot_px(&mut self, x: f32, y: f32, radius: f32, color: Color) {
        if color.3 <= 0.002 || radius <= 0.0 {
            return;
        }
        self.emit()
            .draw_circle(Brush::solid(color), Point::new(x, y), radius);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn round_rect_stroked(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        corner: f32,
        thickness: f32,
        color: Color,
    ) {
        if color.3 <= 0.002 || width <= 0.0 || height <= 0.0 || thickness <= 0.0 {
            return;
        }
        self.emit().draw_round_rect_at_stroked(
            Rect {
                x,
                y,
                width,
                height,
            },
            Brush::solid(color),
            CornerRadii::uniform(corner.min(width.min(height) * 0.5)),
            Stroke::new(thickness),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn line_px(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness: f32,
        cap: StrokeCap,
        color: Color,
    ) {
        if thickness <= 0.0 || color.3 <= 0.002 {
            return;
        }
        let dx = x1 - x0;
        let dy = y1 - y0;
        let length = dx.hypot(dy);
        if length <= f32::EPSILON {
            self.dot_px(x0, y0, thickness * 0.5, color);
            return;
        }
        let radius = length * FLAT_ARC_RADII;
        let centre_x = (x0 + x1) * 0.5 - dy / length * radius;
        let centre_y = (y0 + y1) * 0.5 + dx / length * radius;
        let start = (y0 - centre_y).atan2(x0 - centre_x);
        let end = (y1 - centre_y).atan2(x1 - centre_x);
        let mut sweep = end - start;
        while sweep > std::f32::consts::PI {
            sweep -= std::f32::consts::TAU;
        }
        while sweep < -std::f32::consts::PI {
            sweep += std::f32::consts::TAU;
        }
        self.emit().draw_arc(
            Brush::solid(color),
            Point::new(centre_x, centre_y),
            (radius * radius + length * length * 0.25).sqrt(),
            start,
            sweep,
            Stroke::new(thickness).with_cap(cap),
        );
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, unit_thickness: f32, color: Color) {
        if unit_thickness <= 0.0 || color.3 <= 0.002 {
            return;
        }
        let dx = x1 - x0;
        let dy = y1 - y0;
        let length = (dx * dx + dy * dy).sqrt();
        let dot_radius = unit_thickness * 0.5;
        if length <= dot_radius {
            self.dot(x0, y0, dot_radius, color);
            return;
        }
        let spacing = (dot_radius * DOT_SPACING).max(1e-4);
        let count = ((length / spacing).ceil() as usize + 1).clamp(2, 288);
        let step = 1.0 / (count - 1) as f32;
        for index in 0..count {
            let t = step * index as f32;
            self.dot(x0 + dx * t, y0 + dy * t, dot_radius, color);
        }
    }

    pub fn radial_line(
        &mut self,
        angle: f32,
        inner_radius: f32,
        outer_radius: f32,
        unit_thickness: f32,
        color: Color,
    ) {
        let inner = inner_radius.min(outer_radius).max(0.0);
        let outer = inner_radius.max(outer_radius);
        let mid = (inner + outer) * 0.5;
        if outer <= inner || mid <= 0.0 || unit_thickness <= 0.0 {
            return;
        }
        let sweep = unit_thickness / mid;
        self.annular_sector(inner, outer, angle - sweep * 0.5, sweep, color);
    }
}


fn band_alpha(peak: f32, fraction: f32) -> f32 {
    let alpha = peak * fraction * fraction;
    if alpha <= FALLOFF_FLOOR {
        0.0
    } else {
        alpha
    }
}

pub fn falloff_alpha(peak: f32, fraction: f32) -> f32 {
    if peak <= FALLOFF_FLOOR {
        return 0.0;
    }
    let mut transmittance = 1.0;
    for index in 0..FALLOFF_BANDS {
        let step = (index as f32 + 0.5) / FALLOFF_BANDS as f32;
        if 1.0 - step < fraction {
            break;
        }
        transmittance *= 1.0 - band_alpha(peak, step);
    }
    1.0 - transmittance
}

pub fn inverse_falloff_alpha(peak: f32, fraction: f32) -> f32 {
    if peak <= FALLOFF_FLOOR {
        return 0.0;
    }
    for index in 0..FALLOFF_BANDS {
        let step = 1.0 - index as f32 / FALLOFF_BANDS as f32;
        if fraction >= step - 1.0 / FALLOFF_BANDS as f32 {
            return band_alpha(peak, step);
        }
    }
    0.0
}
