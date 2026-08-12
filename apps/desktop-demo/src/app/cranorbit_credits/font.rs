use super::draw::Painter;
use cranpose_ui_graphics::{Brush, Color, FontWeight, Point, TextMeasurement, TextStyle};
use std::f32::consts::TAU;
use std::sync::atomic::{AtomicU32, Ordering};

pub const FONT_FAMILY: &str = "sans-serif";

/// Roboto's vertical metrics, as Android reports them.
///
/// These are the `hhea` ascender and descender over `unitsPerEm`, read from the
/// `Roboto-Regular.ttf` a Wear device ships. Skia only prefers the `OS/2` typo
/// metrics when a font sets `USE_TYPO_METRICS` in `fsSelection`, and Roboto
/// does not (`fsSelection = 0x0040`), so `hhea` is what `Paint.getFontMetrics`
/// hands `StaticLayout` and what every Compose line height is built from.
/// Taking `usWinDescent` (555) instead put every line one device pixel too
/// tall at 19sp, which moved the wordmark's second line and everything under
/// it down by one.
pub const ROBOTO_ASCENT: f32 = 1900.0 / 2048.0;
pub const ROBOTO_DESCENT: f32 = 500.0 / 2048.0;

pub const BODY_WEIGHT: FontWeight = FontWeight(450);
pub const TITLE_WEIGHT: FontWeight = FontWeight(550);

pub const SHADOW_RINGS: [(f32, f32); 3] = [(0.34, 0.24), (0.67, 0.14), (1.0, 0.07)];
pub const SHADOW_TAPS: usize = 8;
pub const SHADOW_BLUR_PX: f32 = 14.0;

const MIN_VISIBLE_ALPHA: f32 = 0.004;
const DEFAULT_SCREEN_MILLI: u32 = 2000;

static SCREEN_MILLI: AtomicU32 = AtomicU32::new(DEFAULT_SCREEN_MILLI);

/// The system font-size setting, x1000. Wear guideline WO-V1 asks that text
/// follow it; without this every size here would be a fixed dp and the setting
/// would do nothing.
static FONT_SCALE_MILLI: AtomicU32 = AtomicU32::new(1000);

pub fn set_screen_density(density: f32) {
    if density.is_finite() && density > 0.0 {
        SCREEN_MILLI.store((density * 1000.0).round() as u32, Ordering::Relaxed);
    }
}

pub fn screen_density() -> f32 {
    SCREEN_MILLI.load(Ordering::Relaxed) as f32 / 1000.0
}

/// Records the platform's font scale. Values the platform cannot mean are
/// ignored, leaving the last good one in place.
pub fn set_font_scale(scale: f32) {
    if scale.is_finite() && scale > 0.0 {
        FONT_SCALE_MILLI.store((scale.clamp(0.5, 3.0) * 1000.0).round() as u32, Ordering::Relaxed);
    }
}

pub fn font_scale() -> f32 {
    FONT_SCALE_MILLI.load(Ordering::Relaxed) as f32 / 1000.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub size: f32,
    pub line_height: f32,
    pub weight: FontWeight,
    pub tracking: f32,
    pub screen_density: f32,
}

impl Face {
    pub fn ascent(self) -> f32 {
        (ROBOTO_ASCENT * self.size * self.screen_density).round() / self.screen_density
    }

    pub fn descent(self) -> f32 {
        (ROBOTO_DESCENT * self.size * self.screen_density).round() / self.screen_density
    }

    pub fn natural_height(self) -> f32 {
        self.ascent() + self.descent()
    }

    pub fn asked_height(self) -> f32 {
        (self.line_height * self.screen_density).ceil() / self.screen_density
    }

    pub fn block_height(self) -> f32 {
        self.asked_height().max(self.natural_height())
    }

    pub fn baseline(self) -> f32 {
        let asked = self.asked_height();
        let natural = self.natural_height();
        if asked <= natural {
            return self.ascent();
        }
        let grew = (asked - natural) * self.screen_density;
        let below = self.descent() + (grew * 0.5).ceil() / self.screen_density;
        asked - below
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_tracking(mut self, tracking: f32) -> Self {
        self.tracking = tracking;
        self
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn scaled(mut self, scale: f32) -> Self {
        self.size *= scale;
        self.line_height *= scale;
        self.tracking *= scale;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Typography {
    screen_density: f32,
    font_scale: f32,
}

impl Typography {
    pub const BODY_LINE_HEIGHT_SP: f32 = 18.0;
    pub const BODY_TRACKING_SP: f32 = 0.4;

    pub fn new(screen_density: f32) -> Self {
        Self {
            screen_density,
            font_scale: 1.0,
        }
    }

    pub fn platform() -> Self {
        Self::new(screen_density()).with_font_scale(font_scale())
    }

    pub fn with_font_scale(mut self, font_scale: f32) -> Self {
        if font_scale.is_finite() && font_scale > 0.0 {
            self.font_scale = font_scale;
        }
        self
    }

    pub fn font_scale(self) -> f32 {
        self.font_scale
    }

    /// Device pixels per dp, for the places that have to land on a whole one.
    pub fn density(self) -> f32 {
        self.screen_density
    }

    pub fn dp(self, value: f32) -> f32 {
        value
    }

    /// A dp length rounded to the whole device pixel Compose would give it.
    ///
    /// Compose's layout is integral: `Dp.roundToPx()` turns every padding,
    /// minimum size and spacing into an `Int` before anything is measured, and
    /// a child is placed at an `IntOffset`. A length that skips this lands a
    /// row edge mid-pixel, where the rasterizer antialiases a boundary the
    /// Compose build draws crisp -- which is a whole scanline of near-miss
    /// pixels per edge, not a rounding curiosity.
    pub fn dp_px(self, value: f32) -> f32 {
        if self.screen_density > 0.0 {
            (value * self.screen_density).round() / self.screen_density
        } else {
            value
        }
    }

    /// A size in scale-independent pixels: dp multiplied by the user's font
    /// setting. Every piece of text in the app is sized through here, so the
    /// setting reaches all of it; anything that must not move with it is
    /// measured in `dp` instead.
    pub fn sp(self, value: f32) -> f32 {
        value * self.font_scale
    }

    pub fn px(self, value: f32) -> f32 {
        value / self.screen_density
    }

    pub fn face(self, size_sp: f32, weight: FontWeight, tracking_sp: f32) -> Face {
        self.styled(size_sp, Self::BODY_LINE_HEIGHT_SP, weight, tracking_sp)
    }

    pub fn styled(
        self,
        size_sp: f32,
        line_height_sp: f32,
        weight: FontWeight,
        tracking_sp: f32,
    ) -> Face {
        Face {
            size: self.sp(size_sp),
            line_height: self.sp(line_height_sp),
            weight,
            tracking: self.sp(tracking_sp),
            screen_density: self.screen_density,
        }
    }

    pub fn line_height(self) -> f32 {
        self.sp(Self::BODY_LINE_HEIGHT_SP)
    }

    pub fn screen_title(self) -> Face {
        self.face(15.0, FontWeight::MEDIUM, Self::BODY_TRACKING_SP)
    }

    pub fn screen_caption(self) -> Face {
        self.face(12.0, BODY_WEIGHT, Self::BODY_TRACKING_SP)
    }

    pub fn play_caption(self, emphasis: bool) -> Face {
        if emphasis {
            self.face(14.0, FontWeight::MEDIUM, Self::BODY_TRACKING_SP)
        } else {
            self.face(12.0, FontWeight::NORMAL, Self::BODY_TRACKING_SP)
        }
    }

    pub fn word_mark_orbit(self) -> Face {
        self.face(19.0, FontWeight::LIGHT, 5.0)
    }

    pub fn word_mark_breaker(self) -> Face {
        self.face(19.0, FontWeight::MEDIUM, 3.0)
    }

    pub fn choice_label(self) -> Face {
        self.face(14.0, FontWeight::MEDIUM, 1.0)
    }

    pub fn choice_hint(self) -> Face {
        self.face(12.0, BODY_WEIGHT, 1.0)
    }

    pub fn list_header(self) -> Face {
        self.styled(16.0, 18.0, TITLE_WEIGHT, Self::BODY_TRACKING_SP)
    }

    pub fn button_label(self) -> Face {
        self.styled(15.0, 18.0, FontWeight::MEDIUM, Self::BODY_TRACKING_SP)
    }

    pub fn button_secondary(self) -> Face {
        self.styled(13.0, 16.0, FontWeight::MEDIUM, Self::BODY_TRACKING_SP)
    }

    /// A plain line inside a list — a credit, a billing notice, a label row.
    ///
    /// Kotlin's `CreditLine` and the Settings notice both set
    /// `lineHeight = 16.sp` explicitly rather than inheriting the theme's, so
    /// a wrapped credit advances 16sp per line and not the 18sp everything
    /// else in the app inherits. Measured against the Kotlin build, a wrapped
    /// credit paragraph steps 31 device pixels per line; the port was stepping
    /// 36 and every line after the first drifted further down the screen.
    pub fn list_body(self) -> Face {
        self.styled(12.0, 16.0, BODY_WEIGHT, Self::BODY_TRACKING_SP)
    }
}

pub fn text_style(face: Face) -> TextStyle {
    TextStyle::new(face.size)
        .with_font_family(FONT_FAMILY)
        .with_weight(face.weight)
        .with_letter_spacing(face.tracking)
        .with_line_height(face.line_height)
}

pub fn text_width(painter: &Painter, text: &str, face: Face) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    painter
        .scope()
        .measure_text(text, &text_style(face))
        .size
        .width
}

pub fn draw_text(
    painter: &mut Painter,
    text: &str,
    x: f32,
    y: f32,
    face: Face,
    color: Color,
    align: Align,
) {
    draw_layer(painter, text, x, y, face, color, align);
}

#[allow(clippy::too_many_arguments)]
pub fn draw_text_shadowed(
    painter: &mut Painter,
    text: &str,
    x: f32,
    y: f32,
    face: Face,
    color: Color,
    align: Align,
    shadow: Color,
    blur: f32,
) {
    if text.is_empty() || face.size <= 0.0 {
        return;
    }
    let style = text_style(face);
    let measurement = painter.scope().measure_text(text, &style);
    for (spread, alpha) in SHADOW_RINGS {
        let radius = blur * spread;
        let halo = shadow.with_alpha(shadow.3 * alpha);
        for tap in 0..SHADOW_TAPS {
            let angle = TAU * tap as f32 / SHADOW_TAPS as f32;
            emit_layer(
                painter,
                text,
                x + angle.cos() * radius,
                y + angle.sin() * radius,
                face,
                halo,
                align,
                &style,
                &measurement,
            );
        }
    }
    emit_layer(
        painter,
        text,
        x,
        y,
        face,
        color,
        align,
        &style,
        &measurement,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_layer(
    painter: &mut Painter,
    text: &str,
    x: f32,
    y: f32,
    face: Face,
    color: Color,
    align: Align,
    style: &TextStyle,
    measurement: &TextMeasurement,
) {
    if color.3 <= MIN_VISIBLE_ALPHA {
        return;
    }
    let left = match align {
        Align::Start => x,
        Align::Center => x - measurement.size.width * 0.5,
        Align::End => x - measurement.size.width,
    };
    let top = y + face.baseline() - measurement.first_baseline;
    painter
        .emit()
        .draw_text_from(Point::new(left, top), Brush::solid(color), text, style);
}

fn draw_layer(
    painter: &mut Painter,
    text: &str,
    x: f32,
    y: f32,
    face: Face,
    color: Color,
    align: Align,
) {
    if text.is_empty() || color.3 <= MIN_VISIBLE_ALPHA || face.size <= 0.0 {
        return;
    }
    let style = text_style(face);
    let measurement = painter.scope().measure_text(text, &style);
    emit_layer(
        painter,
        text,
        x,
        y,
        face,
        color,
        align,
        &style,
        &measurement,
    );
}
