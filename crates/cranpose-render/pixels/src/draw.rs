#[cfg(test)]
use cranpose_render_common::text_measure::{
    fallback_char_width, fallback_cursor_x_for_byte_offset, fallback_line_height,
    fallback_text_metrics,
};
use cranpose_render_common::{
    brush_sampling::{DitherAxis, DitheredLine, color_to_rgba, sample_brush_rgba},
    graph_scene::RenderDiagnostics,
    layer_shadow::ShadowRRect,
    shape_sdf,
    software_text_raster::SoftwareGlyphRasterCache,
    text_measure::SoftwareTextResources,
};
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, BrushSampler, CornerRadii, LineGeometry, Point, Rect,
    StrokeJoin, Trapezoid, expand_rect, whole,
};

use crate::{
    pipeline,
    scene::{RasterScene, Scene},
};

mod image;
mod reach;
mod row_runs;
mod text;

use image::{blit_image_between_pixels, draw_image, image_rect};
use reach::text_bounds;
use row_runs::{INK_REACH, NO_RUN, RoundedFill, RowRuns, arc_row_runs, stroke_row_runs};
pub(crate) use text::rasterize_text;
use text::{draw_text, is_static_text, rasterize_text_part, text_raster_rect};

/// Glyph masks a renderer keeps between frames.
const GLYPH_MASKS: usize = 256;

/// What drawing text needs: the fonts, and the masks of the glyphs drawn
/// before, so a text that changes rasterizes only glyphs it did not show yet.
pub(crate) struct TextRaster {
    pub(crate) resources: SoftwareTextResources,
    glyphs: std::cell::RefCell<SoftwareGlyphRasterCache>,
}

impl TextRaster {
    pub(crate) fn new(resources: SoftwareTextResources) -> Self {
        Self {
            resources,
            glyphs: std::cell::RefCell::new(SoftwareGlyphRasterCache::with_capacity_at_least_one(
                GLYPH_MASKS,
            )),
        }
    }
}

fn is_blend_mode_supported(mode: BlendMode) -> bool {
    matches!(mode, BlendMode::SrcOver | BlendMode::DstOut)
}

fn snapped_anchor_origin(anchor: Point) -> Point {
    Point::new(whole::round(anchor.x), whole::round(anchor.y))
}

pub(crate) fn snap_delta_for_anchor(anchor: Point) -> Point {
    let snapped = snapped_anchor_origin(anchor);
    Point::new(snapped.x - anchor.x, snapped.y - anchor.y)
}

/// A window of the device frame: the RGBA8 pixels `left..left + width` by
/// `top..top + height`, row by row. The whole frame is the window at the
/// origin; a sprite's texture is the window around the one item it holds.
pub(crate) struct Target<'a> {
    pub pixels: &'a mut [u8],
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl Target<'_> {
    pub(crate) fn window(&self) -> ClipBounds {
        ClipBounds {
            min_x: self.left,
            min_y: self.top,
            max_x: self.left + self.width as i32,
            max_y: self.top + self.height as i32,
        }
    }

    /// The pixels `start..end` of row `y`, which lie inside the window
    /// unless the run is empty (`end <= start`).
    fn row(&mut self, y: i32, start: i32, end: i32) -> &mut [[u8; 4]] {
        if end <= start {
            return &mut [];
        }
        let row = (y - self.top) as usize * self.width as usize;
        let from = row + (start - self.left) as usize;
        let to = row + (end - self.left) as usize;
        &mut self.pixels.as_chunks_mut::<4>().0[from..to]
    }
}

/// `UNIT[c]` is `c as f32 / 255.0`, worked out at build time, so a byte
/// channel becomes a float without a division per pixel.
static UNIT: [f32; 256] = {
    let mut table = [0.0; 256];
    let mut channel = 0;
    while channel < 256 {
        table[channel] = channel as f32 / 255.0;
        channel += 1;
    }
    table
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipBounds {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl ClipBounds {
    fn overlaps(self, other: Self) -> bool {
        self.min_x < other.max_x
            && other.min_x < self.max_x
            && self.min_y < other.max_y
            && other.min_y < self.max_y
    }

    fn holds(self, other: Self) -> bool {
        self.min_x <= other.min_x
            && self.min_y <= other.min_y
            && self.max_x >= other.max_x
            && self.max_y >= other.max_y
    }
}

/// The whole pixels of `rect` cut to `clip` and to the target's window.
fn clip_rect_to_bounds(rect: Rect, clip: Option<Rect>, target: &Target) -> Option<ClipBounds> {
    clip_rect_to_window(rect, clip, target.window())
}

/// The pixel at or below `edge`, a finite value within the `i32` range:
/// the conversion truncates toward zero, so a negative edge with a fraction
/// steps one down. A float floor and a second conversion cost more where
/// no instruction rounds a float.
fn floor_pixel(edge: f32) -> i32 {
    let whole = edge as i32;
    whole - i32::from((whole as f32) > edge)
}

/// The pixel at or above `edge`, a finite value within the `i32` range.
fn ceil_pixel(edge: f32) -> i32 {
    let whole = edge as i32;
    whole + i32::from((whole as f32) < edge)
}

/// The whole pixels of `rect` cut to `clip` and to `window`.
pub(crate) fn clip_rect_to_window(
    rect: Rect,
    clip: Option<Rect>,
    window: ClipBounds,
) -> Option<ClipBounds> {
    let mut min_x = rect.x;
    let mut min_y = rect.y;
    let mut max_x = rect.x + rect.width;
    let mut max_y = rect.y + rect.height;

    if let Some(clip_rect) = clip {
        min_x = min_x.max(clip_rect.x);
        min_y = min_y.max(clip_rect.y);
        max_x = max_x.min(clip_rect.x + clip_rect.width);
        max_y = max_y.min(clip_rect.y + clip_rect.height);
    }

    min_x = min_x.max(window.min_x as f32);
    min_y = min_y.max(window.min_y as f32);
    max_x = max_x.min(window.max_x as f32);
    max_y = max_y.min(window.max_y as f32);

    if max_x <= min_x || max_y <= min_y {
        return None;
    }

    // Each edge now lies within the window, whose bounds are whole, so it
    // is finite (`max` and `min` pass over a NaN) and its whole pixel too.
    let min_x = floor_pixel(min_x);
    let min_y = floor_pixel(min_y);
    let max_x = ceil_pixel(max_x);
    let max_y = ceil_pixel(max_y);

    if min_x >= max_x || min_y >= max_y {
        return None;
    }

    Some(ClipBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

/// Draws `scene` into `frame`, an RGBA8 buffer of `width` by `height` pixels, at one device pixel
/// per logical pixel and with the default font resources.
pub fn draw_scene(frame: &mut [u8], width: u32, height: u32, scene: &Scene) {
    let text = TextRaster::new(SoftwareTextResources::default());
    draw_scene_scaled(frame, width, height, scene, &text, 1.0);
}

pub(crate) fn draw_scene_scaled(
    frame: &mut [u8],
    width: u32,
    height: u32,
    scene: &Scene,
    text: &TextRaster,
    scale: f32,
) {
    if let Some(graph) = scene.graph.as_ref() {
        let raster_scene = pipeline::build_raster_scene(graph, scene.diagnostics(), scale, text);
        draw_raster_scene(
            frame,
            width,
            height,
            &raster_scene,
            scene.diagnostics(),
            text,
        );
    } else {
        clear_frame(frame);
    }
}

fn clear_frame(frame: &mut [u8]) {
    fill_pixels(frame.as_chunks_mut::<4>().0, [18, 18, 24, 255]);
}

/// Paints every pixel of `pixels` with `bytes`: as whole 32-bit words where
/// the pixels are aligned for them, a store a pixel instead of four.
fn fill_pixels(pixels: &mut [[u8; 4]], bytes: [u8; 4]) {
    match bytemuck::try_cast_slice_mut::<[u8; 4], u32>(pixels) {
        Ok(words) => fill_words(words, u32::from_ne_bytes(bytes)),
        Err(_) => pixels.fill(bytes),
    }
}

/// `words.fill(word)` four words a step: a step of the plain loop stores one
/// word, and its counter and branch cost more than the store.
fn fill_words(words: &mut [u32], word: u32) {
    let (quads, rest) = words.as_chunks_mut::<4>();
    for quad in quads {
        *quad = [word; 4];
    }
    rest.fill(word);
}

fn draw_raster_scene(
    frame: &mut [u8],
    width: u32,
    height: u32,
    scene: &RasterScene,
    diagnostics: &RenderDiagnostics,
    text: &TextRaster,
) {
    let mut target = Target {
        pixels: frame,
        left: 0,
        top: 0,
        width,
        height,
    };
    let mut reach = Vec::new();
    scene.reach_into(target.window(), &mut reach);
    draw_raster_window(&mut target, scene, &reach, diagnostics, text);
}

/// Draws the items of `scene` into `target`'s window. `reach` holds where
/// each item of the scene's order can draw, as [`RasterScene::reach_into`]
/// puts it: an item that cannot draw in the window is passed over.
pub(crate) fn draw_raster_window(
    target: &mut Target,
    scene: &RasterScene,
    reach: &[Option<ClipBounds>],
    diagnostics: &RenderDiagnostics,
    text: &TextRaster,
) {
    let window = target.window();
    // The topmost item that paints the whole window opaquely hides what
    // lies under it there: drawing starts with it.
    let first = scene.order.iter().zip(reach).rposition(|(&item, bounds)| {
        bounds.is_some_and(|bounds| bounds.holds(window)) && scene.covers_window(item, window)
    });
    if first.is_none() {
        clear_frame(target.pixels);
    }
    let first = first.unwrap_or(0);
    for (&item, bounds) in scene.order[first..].iter().zip(&reach[first..]) {
        if bounds.is_some_and(|bounds| bounds.overlaps(window)) {
            draw_item(target, scene, item, diagnostics, text);
        }
    }
}

/// Rasterizes into the glyph cache the glyphs of the text of `scene` that
/// reaches `window`, so that drawing the window takes them from the cache.
pub(crate) fn prepare_raster_window(scene: &RasterScene, window: ClipBounds, text: &TextRaster) {
    for draw in &scene.texts {
        if is_static_text(draw)
            && let Some(raster_rect) = text_raster_rect(draw)
            && text_bounds(draw, window).is_some()
        {
            // A part of one pixel visits every glyph, and the cache keeps
            // each glyph it misses, whether the part shows it or not.
            let part = Rect {
                width: 1.0,
                height: 1.0,
                ..raster_rect
            };
            rasterize_text_part(draw, raster_rect, part, text);
        }
    }
}

/// Draws one item of `scene` into `target`, where it reaches the target's
/// window. A shape, an image or a shadow cuts itself to the window as it
/// places itself; a text is checked first, as rasterizing it costs more.
pub(crate) fn draw_item(
    target: &mut Target,
    scene: &RasterScene,
    item: RenderItem,
    diagnostics: &RenderDiagnostics,
    text: &TextRaster,
) {
    match item {
        RenderItem::Shape(index) => draw_shape(target, &scene.shapes[index], diagnostics),
        RenderItem::Image(index) => draw_image(target, &scene.images[index], diagnostics),
        RenderItem::Shadow(index) => draw_shadow(target, &scene.shadows[index], diagnostics),
        RenderItem::Text(index) => {
            let draw = &scene.texts[index];
            if text_bounds(draw, target.window()).is_some() {
                draw_text(target, draw, diagnostics, text);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderItem {
    Shape(usize),
    Image(usize),
    Text(usize),
    Shadow(usize),
}

/// Draws one pass of an elevation shadow: its color at the shadow's
/// coverage of each pixel's centre. Out of line, as are images and the
/// shapes a fill does not paint: the item loop keeps the solid fills and
/// text that most frames draw, and the rest takes code only where it draws.
#[inline(never)]
fn draw_shadow(
    target: &mut Target,
    draw: &crate::scene::ShadowDraw,
    diagnostics: &RenderDiagnostics,
) {
    let (shadow, clip) = placed_shadow(draw);
    let Some(bounds) = clip_rect_to_bounds(shadow.bounds, clip, target) else {
        return;
    };
    let color = [
        draw.color.r(),
        draw.color.g(),
        draw.color.b(),
        draw.color.a(),
    ];
    for py in bounds.min_y..bounds.max_y {
        let center_y = py as f32 + 0.5;
        let row = target.row(py, bounds.min_x, bounds.max_x);
        for (px, dst) in (bounds.min_x..).zip(row) {
            let coverage = shadow.coverage(px as f32 + 0.5, center_y);
            if coverage <= 0.0 {
                continue;
            }
            blend_pixel(
                dst,
                [color[0], color[1], color[2], color[3] * coverage],
                BlendMode::SrcOver,
                diagnostics,
            );
        }
    }
}

/// A shape where its snap puts it: its rect, the coverage of each pixel and
/// the origin its gradient's dither is measured from.
pub(crate) struct PlacedShape {
    pub rect: Rect,
    pub coverage: ShapeCoverage,
    pub dither_origin: Point,
}

/// `rect` moved to whole pixels and grown to cover at least one.
fn on_pixel_grid(rect: Rect) -> Rect {
    Rect {
        x: whole::round(rect.x),
        y: whole::round(rect.y),
        width: if rect.width > 0.0 {
            whole::ceil(rect.width).max(1.0)
        } else {
            rect.width
        },
        height: if rect.height > 0.0 {
            whole::ceil(rect.height).max(1.0)
        } else {
            rect.height
        },
    }
}

/// `draw` where its snap puts it; `None` when it covers nothing.
pub(crate) fn place_shape(draw: &crate::scene::DrawShape) -> Option<PlacedShape> {
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    let rect = draw.rect.translate(snap_delta.x, snap_delta.y);
    let dither_origin = draw
        .snap_anchor
        .map(snapped_anchor_origin)
        .unwrap_or_default();
    let rect = if draw.snap_to_pixel_grid {
        on_pixel_grid(rect)
    } else {
        rect
    };
    let coverage = ShapeCoverage::of(draw, rect, snap_delta)?;
    Some(PlacedShape {
        rect,
        coverage,
        dither_origin,
    })
}

fn draw_shape(
    target: &mut Target,
    draw: &crate::scene::DrawShape,
    diagnostics: &RenderDiagnostics,
) {
    let Some(PlacedShape {
        rect,
        coverage,
        dither_origin,
    }) = place_shape(draw)
    else {
        return;
    };
    let Some(clip_bounds) = clip_rect_to_bounds(coverage.reach(rect), draw.clip, target) else {
        return;
    };

    let paint = ShapeRows {
        target,
        draw,
        rect,
        clip_bounds,
        dither_origin,
        opaque: opaque_solid_bytes(&draw.brush, draw.blend_mode),
        diagnostics,
    };
    let whole_rows = |_: f32, min_x: i32, max_x: i32| [(min_x, max_x), NO_RUN];
    match &coverage {
        ShapeCoverage::Line(line) => paint.paint(whole_rows, |point| line.coverage(point)),
        ShapeCoverage::Trapezoid(trapezoid) => {
            paint.paint(whole_rows, |point| trapezoid.coverage(point));
        }
        ShapeCoverage::Arc(arc) => {
            let band = shape_sdf::ArcBand::new(arc);
            paint.paint(
                |y, min_x, max_x| arc_row_runs(arc, y, min_x, max_x),
                |point| band.coverage(point),
            );
        }
        &ShapeCoverage::Stroke {
            rect,
            radii,
            half_width,
            join,
        } => paint.paint(
            |y, min_x, max_x| stroke_row_runs(rect, radii, half_width, join, y, min_x, max_x),
            |point| shape_sdf::stroked_rect_coverage(point, rect, radii, half_width, join),
        ),
        ShapeCoverage::Fill {
            rect,
            radii: Some(radii),
        } => {
            let fill = RoundedFill::new(*rect, radii);
            paint.fill(|y, min_x, max_x| [fill.run(y, min_x, max_x), NO_RUN]);
        }
        ShapeCoverage::Fill { radii: None, .. } => paint.fill(whole_rows),
    }
}

/// A placed shape's rows to paint. Each kind of coverage paints them through
/// its own copy of the loop: the work a kind does once per shape, such as an
/// arc's sines or a line's direction, then stays out of the other kinds'
/// rows, where the compiler would otherwise hoist it for every shape.
struct ShapeRows<'a, 'p> {
    target: &'a mut Target<'p>,
    draw: &'a crate::scene::DrawShape,
    rect: Rect,
    clip_bounds: ClipBounds,
    dither_origin: Point,
    /// The bytes the brush leaves on a pixel it covers whole, when it is an
    /// opaque solid colour.
    opaque: Option<[u8; 4]>,
    diagnostics: &'a RenderDiagnostics,
}

impl ShapeRows<'_, '_> {
    /// Paints a fill, which covers every pixel of its runs whole.
    fn fill(self, runs: impl Fn(f32, i32, i32) -> RowRuns) {
        match self.opaque {
            Some(bytes) => {
                let bounds = self.clip_bounds;
                for py in bounds.min_y..bounds.max_y {
                    for (start, end) in runs(py as f32 + 0.5, bounds.min_x, bounds.max_x) {
                        fill_pixels(self.target.row(py, start, end), bytes);
                    }
                }
            }
            None => {
                let brush = BrushSampler::new(&self.draw.brush, self.rect);
                if brush.has_uniform_rows() {
                    self.fill_uniform_rows(runs);
                } else if brush.has_uniform_columns()
                    && brush.is_opaque()
                    && self.draw.blend_mode == BlendMode::SrcOver
                {
                    self.fill_uniform_columns(runs);
                } else {
                    self.paint(runs, |_| 1.0);
                }
            }
        }
    }

    // Paints a fill whose brush gives every pixel of a row one color, such
    // as a gradient that runs straight down: the pixels [`Self::paint`]
    // paints, with each row's colors worked out once for the four columns
    // of the dither pattern instead of once a pixel.
    #[inline(never)]
    fn fill_uniform_rows(self, runs: impl Fn(f32, i32, i32) -> RowRuns) {
        let Self {
            target,
            draw,
            rect,
            clip_bounds,
            dither_origin,
            diagnostics,
            ..
        } = self;
        let brush = BrushSampler::new(&draw.brush, rect);
        let columns = DitherAxis::new(dither_origin.x);
        for py in clip_bounds.min_y..clip_bounds.max_y {
            let center_y = py as f32 + 0.5;
            let row_runs = runs(center_y, clip_bounds.min_x, clip_bounds.max_x);
            let Some(&(first, _)) = row_runs.iter().find(|(start, end)| start < end) else {
                continue;
            };
            let row = DitheredLine::row(&brush, first as f32 + 0.5, center_y, dither_origin);
            let colors = row.colors();
            let bytes = colors.map(|color| {
                (draw.blend_mode == BlendMode::SrcOver && color[3] >= 1.0)
                    .then(|| opaque_bytes(color))
            });
            for (start, end) in row_runs {
                for (px, dst) in (start..).zip(target.row(py, start, end)) {
                    let cell = columns.cell(px);
                    match bytes[cell] {
                        Some(bytes) => *dst = bytes,
                        None if colors[cell][3] > 0.0 => {
                            blend_pixel(dst, colors[cell], draw.blend_mode, diagnostics);
                        }
                        None => {}
                    }
                }
            }
        }
    }

    // Paints a fill whose opaque brush gives every pixel of a column one
    // color, such as a gradient that runs straight across: the pixels
    // [`Self::paint`] paints, with each column's bytes worked out once for
    // the four rows of the dither pattern instead of once a pixel.
    #[inline(never)]
    fn fill_uniform_columns(self, runs: impl Fn(f32, i32, i32) -> RowRuns) {
        let Self {
            target,
            draw,
            rect,
            clip_bounds,
            dither_origin,
            ..
        } = self;
        let brush = BrushSampler::new(&draw.brush, rect);
        let rows = DitherAxis::new(dither_origin.y);
        let top = clip_bounds.min_y as f32 + 0.5;
        let mut band = [[[0u8; 4]; 4]; COLUMN_BAND];
        let mut left = clip_bounds.min_x;
        while left < clip_bounds.max_x {
            let right = left
                .saturating_add(COLUMN_BAND as i32)
                .min(clip_bounds.max_x);
            for (bytes, px) in band.iter_mut().zip(left..right) {
                let column = DitheredLine::column(&brush, px as f32 + 0.5, top, dither_origin);
                *bytes = column.colors().map(opaque_bytes);
            }
            for py in clip_bounds.min_y..clip_bounds.max_y {
                let cell = rows.cell(py);
                for (start, end) in runs(py as f32 + 0.5, left, right) {
                    if start >= end {
                        continue;
                    }
                    let first = (start - left) as usize;
                    for (dst, bytes) in target.row(py, start, end).iter_mut().zip(&band[first..]) {
                        *dst = bytes[cell];
                    }
                }
            }
            left = right;
        }
    }

    /// Paints the pixels of each row's `runs` by their `coverage`. Out of
    /// line, as [`draw_shadow`] is.
    #[inline(never)]
    fn paint(self, runs: impl Fn(f32, i32, i32) -> RowRuns, coverage: impl Fn(Point) -> f32) {
        let Self {
            target,
            draw,
            rect,
            clip_bounds,
            dither_origin,
            opaque,
            diagnostics,
        } = self;
        let brush = BrushSampler::new(&draw.brush, rect);
        for py in clip_bounds.min_y..clip_bounds.max_y {
            let center_y = py as f32 + 0.5;
            for (start, end) in runs(center_y, clip_bounds.min_x, clip_bounds.max_x) {
                for (px, dst) in (start..).zip(target.row(py, start, end)) {
                    let center_x = px as f32 + 0.5;
                    let coverage = coverage(Point::new(center_x, center_y));
                    if coverage <= 0.0 {
                        continue;
                    }
                    if let Some(bytes) = opaque
                        && coverage >= 1.0
                    {
                        *dst = bytes;
                        continue;
                    }

                    let mut sample = sample_brush_rgba(&brush, center_x, center_y, dither_origin);
                    sample[3] *= coverage;
                    if sample[3] <= 0.0 {
                        continue;
                    }
                    blend_pixel(dst, sample, draw.blend_mode, diagnostics);
                }
            }
        }
    }
}

/// The bytes an opaque solid colour leaves on a pixel it covers fully,
/// whatever was there: what `blend_pixel` works out for it.
fn opaque_solid_bytes(brush: &Brush, blend_mode: BlendMode) -> Option<[u8; 4]> {
    let Brush::Solid(color) = brush else {
        return None;
    };
    paints_opaque(brush, blend_mode).then(|| opaque_bytes(color_to_rgba(*color)))
}

/// Whether [`opaque_solid_bytes`] has bytes for `brush` under `blend_mode`,
/// without working them out.
fn paints_opaque(brush: &Brush, blend_mode: BlendMode) -> bool {
    blend_mode == BlendMode::SrcOver && matches!(brush, Brush::Solid(color) if color.3 >= 1.0)
}

/// How much of a pixel a shape covers, decided once per shape: a line, an
/// arc band, a stroked outline or a fill, placed where its snap moved it.
pub(crate) enum ShapeCoverage {
    Line(LineGeometry),
    Trapezoid(Trapezoid),
    Arc(ArcGeometry),
    Stroke {
        rect: Rect,
        radii: Option<CornerRadii>,
        half_width: f32,
        join: StrokeJoin,
    },
    Fill {
        rect: Rect,
        radii: Option<CornerRadii>,
    },
}

impl ShapeCoverage {
    /// The coverage of `draw` laid at `rect`, its line or arc moved by
    /// `snap_delta` as the rect was; `None` when it covers nothing.
    fn of(draw: &crate::scene::DrawShape, rect: Rect, snap_delta: Point) -> Option<Self> {
        let shift = |point: Point| Point::new(point.x + snap_delta.x, point.y + snap_delta.y);
        if let Some(line) = draw.line {
            return (!line.is_degenerate())
                .then(|| Self::Line(line.placed(shift(line.start), shift(line.end), 1.0)));
        }
        if let Some(trapezoid) = draw.trapezoid {
            return Some(Self::Trapezoid(
                trapezoid.translate(snap_delta.x, snap_delta.y),
            ));
        }
        if let Some(arc) = draw.arc {
            return (!arc.is_degenerate()).then(|| {
                Self::Arc(ArcGeometry {
                    center: shift(arc.center),
                    ..arc
                })
            });
        }
        let outset = draw.stroke.map_or(0.0, |stroke| stroke.half_width());
        let radii = draw.shape.map(|shape| {
            shape.resolve(
                (rect.width - outset * 2.0).max(0.0),
                (rect.height - outset * 2.0).max(0.0),
            )
        });
        match draw.stroke {
            Some(stroke) if !stroke.is_visible() => None,
            Some(stroke) => Some(Self::Stroke {
                rect,
                radii,
                half_width: stroke.half_width(),
                join: stroke.join,
            }),
            None => Some(Self::Fill { rect, radii }),
        }
    }

    /// The pixels it can cover: a slice's own box, which its edges reach
    /// half a pixel past, an arc's band, otherwise the shape's `rect`. An
    /// arc's `rect` is the disc around its whole circle.
    pub(crate) fn reach(&self, rect: Rect) -> Rect {
        match self {
            Self::Trapezoid(trapezoid) => expand_rect(trapezoid.bounds(), 0.5),
            Self::Arc(arc) => expand_rect(arc.bounds(), INK_REACH),
            _ => rect,
        }
    }
}

fn placed_shadow(draw: &crate::scene::ShadowDraw) -> (ShadowRRect, Option<Rect>) {
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    (
        draw.shadow.translated(snap_delta.x, snap_delta.y),
        draw.clip
            .map(|clip| clip.translate(snap_delta.x, snap_delta.y)),
    )
}

// The columns [`ShapeRows::fill_uniform_columns`] works out at a time: their
// bytes, a kilobyte, sit on the stack.
const COLUMN_BAND: usize = 64;

// `unit` clamped to 0..=1 as the nearest of 256 levels, halves rounded up:
// `whole::round(unit.clamp(0.0, 1.0) * 255.0) as u8`. Below 2^23 the
// truncation and the fraction left are exact, and the level is never
// negative, so a fraction from one half up is the whole step `round` takes.
fn unit_byte(unit: f32) -> u8 {
    let level = unit.clamp(0.0, 1.0) * 255.0;
    let truncated = level as u32;
    (truncated + u32::from(level - truncated as f32 >= 0.5)) as u8
}

// The bytes an opaque `src` leaves on any pixel under `SrcOver`.
fn opaque_bytes(src: [f32; 4]) -> [u8; 4] {
    [unit_byte(src[0]), unit_byte(src[1]), unit_byte(src[2]), 255]
}

fn blend_pixel(
    dst: &mut [u8; 4],
    src: [f32; 4],
    blend_mode: BlendMode,
    diagnostics: &RenderDiagnostics,
) {
    let resolved_blend_mode = if is_blend_mode_supported(blend_mode) {
        blend_mode
    } else {
        if diagnostics.claim_warning_once("pixels.unsupported-blend-mode") {
            log::warn!(
                "Pixels renderer currently supports BlendMode::SrcOver and BlendMode::DstOut; falling back to SrcOver for unsupported modes"
            );
        }
        BlendMode::SrcOver
    };

    let src_alpha = src[3].clamp(0.0, 1.0);
    if src_alpha <= 0.0 {
        return;
    }
    // An opaque source over anything is the source: the same bytes the
    // blend below works out, without reading the destination.
    if src_alpha >= 1.0 && resolved_blend_mode != BlendMode::DstOut {
        *dst = opaque_bytes(src);
        return;
    }
    let [dst_r, dst_g, dst_b, dst_a] = dst.map(|channel| UNIT[usize::from(channel)]);

    let (out_r, out_g, out_b, out_a) = match resolved_blend_mode {
        BlendMode::DstOut => {
            let keep = 1.0 - src_alpha;
            (dst_r * keep, dst_g * keep, dst_b * keep, dst_a * keep)
        }
        BlendMode::SrcOver => (
            src[0].clamp(0.0, 1.0) * src_alpha + dst_r * (1.0 - src_alpha),
            src[1].clamp(0.0, 1.0) * src_alpha + dst_g * (1.0 - src_alpha),
            src[2].clamp(0.0, 1.0) * src_alpha + dst_b * (1.0 - src_alpha),
            src_alpha + dst_a * (1.0 - src_alpha),
        ),
        _ => (
            src[0].clamp(0.0, 1.0) * src_alpha + dst_r * (1.0 - src_alpha),
            src[1].clamp(0.0, 1.0) * src_alpha + dst_g * (1.0 - src_alpha),
            src[2].clamp(0.0, 1.0) * src_alpha + dst_b * (1.0 - src_alpha),
            src_alpha + dst_a * (1.0 - src_alpha),
        ),
    };

    *dst = [
        unit_byte(out_r),
        unit_byte(out_g),
        unit_byte(out_b),
        unit_byte(out_a),
    ];
}

#[cfg(test)]
#[path = "tests/draw_tests.rs"]
mod tests;
