//! A bitmap that tiles, charts and text are drawn on once, on the CPU, for the
//! layers that never change. Vello draws no more than a few thousand glyphs a
//! frame and Vger no more than 65,536 shapes, so the wall's tens of
//! thousands of tiles do not fit either.

use floem::{
    kurbo::Point,
    peniko::{Color, Image},
    text::{CacheKey, SwashContent, TextLayout},
};
use floem_renderer::{
    swash::SwashScaler,
    tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform},
};

use crate::{picture::bitmap, surface::Surface};

pub struct Raster {
    pixels: Pixmap,
    scale: f32,
    glyphs: SwashScaler,
    // Where the point `(0, 0)` of what is drawn lies, in points.
    origin: (f32, f32),
}

fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    paint
}

impl Raster {
    /// A bitmap of `width` by `height` points at `scale` pixels to a point,
    /// filled with `background`.
    pub fn new((width, height): (f32, f32), scale: f32, background: Color) -> Self {
        let mut pixels = Pixmap::new(
            (width * scale).ceil() as u32,
            (height * scale).ceil() as u32,
        )
        .unwrap_or_else(|| Pixmap::new(1, 1).expect("a bitmap of one pixel"));
        pixels.fill(floem_renderer::tiny_skia::Color::from_rgba8(
            background.r,
            background.g,
            background.b,
            background.a,
        ));
        Self {
            pixels,
            scale,
            glyphs: SwashScaler::new(0.0),
            origin: (0.0, 0.0),
        }
    }

    /// Moves the point `(0, 0)` of what is drawn from now on to `(x, y)`.
    pub fn set_origin(&mut self, x: f32, y: f32) {
        self.origin = (x, y);
    }

    fn transform(&self) -> Transform {
        Transform::from_row(
            self.scale,
            0.0,
            0.0,
            self.scale,
            self.origin.0 * self.scale,
            self.origin.1 * self.scale,
        )
    }

    /// Makes the rectangle `(left, top, width, height)` of the bitmap
    /// `color` again, whatever it showed.
    pub fn clear_rect(&mut self, (left, top, width, height): (f32, f32, f32, f32), color: Color) {
        let (left, top) = (left + self.origin.0, top + self.origin.1);
        let Some(rect) = floem_renderer::tiny_skia::Rect::from_xywh(
            left * self.scale,
            top * self.scale,
            width * self.scale,
            height * self.scale,
        ) else {
            return;
        };
        let mut source = paint(color);
        source.blend_mode = floem_renderer::tiny_skia::BlendMode::Source;
        source.anti_alias = false;
        self.pixels
            .fill_rect(rect, &source, Transform::identity(), None);
    }

    /// A line of `points` that is `width` points wide.
    pub fn polyline(&mut self, points: impl Iterator<Item = Point>, width: f32, color: Color) {
        let mut path = PathBuilder::new();
        for (index, point) in points.enumerate() {
            if index == 0 {
                path.move_to(point.x as f32, point.y as f32);
            } else {
                path.line_to(point.x as f32, point.y as f32);
            }
        }
        let Some(path) = path.finish() else {
            return;
        };
        let stroke = Stroke {
            width,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            ..Stroke::default()
        };
        self.pixels
            .stroke_path(&path, &paint(color), &stroke, self.transform(), None);
    }

    /// The bitmap as an image. A bitmap holds its colors premultiplied by their
    /// alpha and an image does not.
    pub fn snapshot(&self) -> Image {
        let (width, height) = (self.pixels.width(), self.pixels.height());
        let mut data = self.pixels.data().to_vec();
        for pixel in data.as_chunks_mut::<4>().0.iter_mut() {
            let alpha = u32::from(pixel[3]);
            if alpha != 0 && alpha != 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        bitmap(width, height, data)
    }

    /// The bitmap as an image, which draws nothing more.
    pub fn into_image(self) -> Image {
        self.snapshot()
    }

    // Adds the glyph's coverage of the pixel at `(x, y)` in `color`.
    fn blend(&mut self, x: i32, y: i32, coverage: u8, color: Color) {
        let (width, height) = (self.pixels.width() as i32, self.pixels.height() as i32);
        if x < 0 || y < 0 || x >= width || y >= height {
            return;
        }
        let alpha = u32::from(coverage) * u32::from(color.a) / 255;
        let at = (y * width + x) as usize * 4;
        let pixel = &mut self.pixels.data_mut()[at..at + 4];
        for (channel, value) in pixel.iter_mut().zip([color.r, color.g, color.b]) {
            let over = u32::from(value) * alpha / 255;
            *channel = (over + u32::from(*channel) * (255 - alpha) / 255) as u8;
        }
        pixel[3] = (alpha + u32::from(pixel[3]) * (255 - alpha) / 255) as u8;
    }
}

impl Surface for Raster {
    fn fill_rounded(
        &mut self,
        (left, top, right, bottom): (f64, f64, f64, f64),
        radius: f64,
        color: Color,
    ) {
        let (left, top, right, bottom, radius) = (
            left as f32,
            top as f32,
            right as f32,
            bottom as f32,
            radius as f32,
        );
        let mut path = PathBuilder::new();
        // Each corner is a cubic curve that approximates a quarter circle.
        let bend = radius * 0.552_284_8;
        path.move_to(left + radius, top);
        path.line_to(right - radius, top);
        path.cubic_to(
            right - radius + bend,
            top,
            right,
            top + radius - bend,
            right,
            top + radius,
        );
        path.line_to(right, bottom - radius);
        path.cubic_to(
            right,
            bottom - radius + bend,
            right - radius + bend,
            bottom,
            right - radius,
            bottom,
        );
        path.line_to(left + radius, bottom);
        path.cubic_to(
            left + radius - bend,
            bottom,
            left,
            bottom - radius + bend,
            left,
            bottom - radius,
        );
        path.line_to(left, top + radius);
        path.cubic_to(
            left,
            top + radius - bend,
            left + radius - bend,
            top,
            left + radius,
            top,
        );
        path.close();
        if let Some(path) = path.finish() {
            self.pixels.fill_path(
                &path,
                &paint(color),
                FillRule::Winding,
                self.transform(),
                None,
            );
        }
    }

    fn fill_circle(&mut self, center: Point, radius: f64, color: Color) {
        let mut path = PathBuilder::new();
        path.push_circle(center.x as f32, center.y as f32, radius as f32);
        if let Some(path) = path.finish() {
            self.pixels.fill_path(
                &path,
                &paint(color),
                FillRule::Winding,
                self.transform(),
                None,
            );
        }
    }

    fn text(&mut self, layout: &TextLayout, at: Point) {
        for run in layout.layout_runs() {
            for glyph in run.glyphs {
                let physical = glyph.physical(
                    (
                        (at.x as f32 + self.origin.0) * self.scale,
                        (at.y as f32 + run.line_y + self.origin.1) * self.scale,
                    ),
                    self.scale,
                );
                let key: CacheKey = physical.cache_key;
                let Some(image) = self.glyphs.get_image(key) else {
                    continue;
                };
                if image.content != SwashContent::Mask {
                    continue;
                }
                let color = glyph
                    .color_opt
                    .map_or(Color::BLACK, |c| Color::rgba8(c.r(), c.g(), c.b(), c.a()));
                let (width, height) = (image.placement.width as i32, image.placement.height as i32);
                for row in 0..height {
                    for column in 0..width {
                        let coverage = image.data[(row * width + column) as usize];
                        if coverage > 0 {
                            self.blend(
                                physical.x + image.placement.left + column,
                                physical.y - image.placement.top + row,
                                coverage,
                                color,
                            );
                        }
                    }
                }
            }
        }
    }
}
