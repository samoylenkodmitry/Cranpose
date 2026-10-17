#[cfg(test)]
use cranpose_render_common::text_measure::{
    fallback_char_width, fallback_cursor_x_for_byte_offset, fallback_line_height,
    fallback_text_metrics,
};
use cranpose_render_common::{
    brush_sampling::{color_to_rgba, sample_brush_rgba},
    graph_scene::RenderDiagnostics,
    layer_shadow::ShadowRRect,
    shape_sdf,
    software_text_raster::{
        SoftwareGlyphRasterCache, TextPixels, rasterize_annotated_text_region_pixels,
    },
    text_measure::SoftwareTextResources,
};
use cranpose_ui::text::TextMotion;
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, CornerRadii, LineGeometry, Point, Rect, StrokeJoin, Trapezoid,
    expand_rect, whole,
};

use crate::{
    pipeline,
    scene::{ImageDraw, RasterScene, Scene, TextDraw},
    style::point_in_resolved_rounded_rect,
};

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
    draw_raster_window(&mut target, scene, diagnostics, text);
}

pub(crate) fn draw_raster_window(
    target: &mut Target,
    scene: &RasterScene,
    diagnostics: &RenderDiagnostics,
    text: &TextRaster,
) {
    let window = target.window();
    // The topmost item that paints the whole window opaquely hides what
    // lies under it there: drawing starts with it.
    let first = scene
        .order
        .iter()
        .rposition(|&item| scene.covers_window(item, window));
    if first.is_none() {
        clear_frame(target.pixels);
    }
    for &item in &scene.order[first.unwrap_or(0)..] {
        draw_item(target, scene, item, diagnostics, text);
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

impl RasterScene {
    /// The pixels `item` can reach before its clip, for the items drawn into
    /// a frame directly; text reaches what its rasterized image covers.
    pub(crate) fn item_reach(&self, item: RenderItem) -> Option<Rect> {
        match item {
            RenderItem::Shape(index) => {
                let placed = place_shape(&self.shapes[index])?;
                Some(placed.coverage.reach(placed.rect))
            }
            RenderItem::Image(index) => Some(image_rect(&self.images[index])),
            RenderItem::Shadow(index) => {
                let draw = &self.shadows[index];
                let snap = draw
                    .snap_anchor
                    .map(snap_delta_for_anchor)
                    .unwrap_or_default();
                Some(draw.shadow.bounds.translate(snap.x, snap.y))
            }
            RenderItem::Text(_) => None,
        }
    }

    /// Whether `item` paints every pixel of `window` with opaque bytes: an
    /// opaque solid fill whose clipped bounds hold the window and, when
    /// rounded, whose shape holds the window's corner pixels, and so every
    /// pixel between them, the shape being convex.
    fn covers_window(&self, item: RenderItem, window: ClipBounds) -> bool {
        let RenderItem::Shape(index) = item else {
            return false;
        };
        let draw = &self.shapes[index];
        if !paints_opaque(&draw.brush, draw.blend_mode) {
            return false;
        }
        let Some(PlacedShape {
            coverage: ShapeCoverage::Fill { rect, radii },
            ..
        }) = place_shape(draw)
        else {
            return false;
        };
        if clip_rect_to_window(rect, draw.clip, window) != Some(window) {
            return false;
        }
        radii.is_none_or(|radii| {
            let (left, right) = (window.min_x as f32 + 0.5, window.max_x as f32 - 0.5);
            let (top, bottom) = (window.min_y as f32 + 0.5, window.max_y as f32 - 0.5);
            // Between the corners on either axis, every pixel the rect covers
            // lies inside it: no corner needs a test.
            let between_columns = left >= rect.x + radii.top_left.max(radii.bottom_left)
                && right <= rect.x + rect.width - radii.top_right.max(radii.bottom_right);
            let between_rows = top >= rect.y + radii.top_left.max(radii.top_right)
                && bottom <= rect.y + rect.height - radii.bottom_left.max(radii.bottom_right);
            between_columns
                || between_rows
                || [(left, top), (right, top), (left, bottom), (right, bottom)]
                    .into_iter()
                    .all(|(x, y)| point_in_resolved_rounded_rect(x, y, rect, &radii))
        })
    }

    pub(crate) fn item_bounds(&self, item: RenderItem, window: ClipBounds) -> Option<ClipBounds> {
        match item {
            RenderItem::Shape(index) => {
                let draw = &self.shapes[index];
                let placed = place_shape(draw)?;
                clip_rect_to_window(placed.coverage.reach(placed.rect), draw.clip, window)
            }
            RenderItem::Image(index) => {
                let draw = &self.images[index];
                if draw.alpha <= 0.0 {
                    return None;
                }
                clip_rect_to_window(image_rect(draw), draw.clip, window)
            }
            RenderItem::Shadow(index) => {
                let (shadow, clip) = placed_shadow(&self.shadows[index]);
                clip_rect_to_window(shadow.bounds, clip, window)
            }
            RenderItem::Text(index) => text_bounds(&self.texts[index], window),
        }
    }
}

/// The pixels of `window` that the rasterized `draw` can reach.
fn text_bounds(draw: &TextDraw, window: ClipBounds) -> Option<ClipBounds> {
    let raster_rect = text_raster_rect(draw)?;
    let reach = Rect {
        width: whole::ceil(raster_rect.width).max(1.0),
        height: whole::ceil(raster_rect.height).max(1.0),
        ..raster_rect
    };
    clip_rect_to_window(reach, draw.clip, window)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderItem {
    Shape(usize),
    Image(usize),
    Text(usize),
    Shadow(usize),
}

/// Draws one pass of an elevation shadow: its color at the shadow's
/// coverage of each pixel's centre.
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
        ShapeCoverage::Arc(arc) => paint.paint(
            |y, min_x, max_x| arc_row_runs(arc, y, min_x, max_x),
            |point| shape_sdf::arc_coverage(point, arc),
        ),
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
            None => self.paint(runs, |_| 1.0),
        }
    }

    /// Paints the pixels of each row's `runs` by their `coverage`.
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

                    let mut sample =
                        sample_brush_rgba(&draw.brush, rect, center_x, center_y, dither_origin);
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
    paints_opaque(brush, blend_mode).then(|| {
        let rgba = color_to_rgba(*color);
        [
            whole::round(rgba[0] * 255.0) as u8,
            whole::round(rgba[1] * 255.0) as u8,
            whole::round(rgba[2] * 255.0) as u8,
            255,
        ]
    })
}

/// Whether [`opaque_solid_bytes`] has bytes for `brush` under `blend_mode`,
/// without working them out.
fn paints_opaque(brush: &Brush, blend_mode: BlendMode) -> bool {
    blend_mode == BlendMode::SrcOver && matches!(brush, Brush::Solid(color) if color.3 >= 1.0)
}

/// How far from a band's edge a pixel centre still takes ink: the
/// antialiasing reaches half a pixel, and the rest is room for the rounding
/// in the distance functions.
const INK_REACH: f32 = 0.5 + 1.0 / 64.0;

/// The pixels of one row worth testing, as two runs `start..end`: the band on
/// either side of a hole. A run with `start >= end` is empty.
type RowRuns = [(i32, i32); 2];

const NO_RUN: (i32, i32) = (0, 0);

/// The pixels whose centres lie within `half` of `centre`, cut to
/// `min..max`.
fn run_within(centre: f32, half: f32, min: i32, max: i32) -> (i32, i32) {
    let start = (whole::ceil(centre - half - 0.5) as i32).max(min);
    let end = (whole::floor(centre + half - 0.5) as i32 + 1).min(max);
    (start, end)
}

/// `run` without the pixels whose centres lie in `from..=to`.
fn runs_around(run: (i32, i32), from: f32, to: f32) -> RowRuns {
    let first = whole::ceil(from - 0.5) as i32;
    let last = whole::floor(to - 0.5) as i32;
    if first > last {
        return [run, NO_RUN];
    }
    [(run.0, run.1.min(first)), (run.0.max(last + 1), run.1)]
}

/// The row of an arc that can take ink. Every point of the band, caps
/// included, lies between the inner and the outer radius, so the row stops at
/// the outer circle and skips the inner one.
fn arc_row_runs(arc: &ArcGeometry, y: f32, min_x: i32, max_x: i32) -> RowRuns {
    let dy = y - arc.center.y;
    let outer = arc.outer_radius + INK_REACH;
    let reach = outer * outer - dy * dy;
    if reach.is_nan() || reach <= 0.0 {
        return [NO_RUN, NO_RUN];
    }
    let run = run_within(arc.center.x, reach.sqrt(), min_x, max_x);
    let inner = arc.inner_radius - INK_REACH;
    let hole = inner * inner - dy * dy;
    if inner <= 0.0 || hole.is_nan() || hole <= 0.0 {
        return [run, NO_RUN];
    }
    let hole = hole.sqrt();
    runs_around(run, arc.center.x - hole, arc.center.x + hole)
}

/// The row of a stroked rounded rect that can take ink: inside the outer edge
/// grown by `INK_REACH`, and outside the part of the hole that lies at least
/// `INK_REACH` inside the inner edge. Both edges are the rounded boxes
/// `shape_sdf::sdf_stroked_rounded_rect` measures, taken on each side of the
/// centre with that side's corner radius.
fn stroke_row_runs(
    rect: Rect,
    radii: Option<CornerRadii>,
    half_width: f32,
    join: StrokeJoin,
    y: f32,
    min_x: i32,
    max_x: i32,
) -> RowRuns {
    let half_width = half_width.max(0.0);
    let half_size = (rect.width * 0.5, rect.height * 0.5);
    let geometry = (
        (half_size.0 - half_width).max(0.0),
        (half_size.1 - half_width).max(0.0),
    );
    let outer = (geometry.0 + half_width, geometry.1 + half_width);
    let inner = (
        (geometry.0 - half_width).max(0.0),
        (geometry.1 - half_width).max(0.0),
    );
    let centre_x = rect.x + half_size.0;
    let local_y = (y - (rect.y + half_size.1)).abs();
    if !(local_y.is_finite() && centre_x.is_finite()) {
        return [(min_x, max_x), NO_RUN];
    }
    let radii = radii.unwrap_or_default();
    let (left, right) = if y - (rect.y + half_size.1) > 0.0 {
        (radii.bottom_left, radii.bottom_right)
    } else {
        (radii.top_left, radii.top_right)
    };
    // How far from the centre the grown outer edge reaches on one side;
    // `None` when the row passes outside it.
    let ink_reach = |radius: f32| {
        let radius = if join != StrokeJoin::Round && radius < 0.0001 {
            0.0
        } else {
            radius + half_width
        };
        let qy = local_y - outer.1 + radius;
        let grown = radius + INK_REACH;
        if qy.is_nan() || qy >= grown {
            return None;
        }
        Some(if qy <= 0.0 {
            outer.0 + INK_REACH
        } else {
            outer.0 - radius + (grown * grown - qy * qy).max(0.0).sqrt()
        })
    };
    // How far from the centre the hole is empty on one side, if it is.
    let empty_reach = |radius: f32| {
        let radius = (radius - half_width).max(0.0);
        let qy = local_y - inner.1 + radius;
        if qy.is_nan() || qy > radius - INK_REACH {
            return None;
        }
        let reach = if qy <= 0.0 {
            inner.0 - INK_REACH
        } else {
            let span = (radius - INK_REACH) * (radius - INK_REACH) - qy * qy;
            inner.0 - radius + span.max(0.0).sqrt()
        };
        (reach >= 0.0).then_some(reach)
    };
    // A side the row misses still keeps the centre's own column, which
    // belongs to the left side.
    let run = match (ink_reach(left), ink_reach(right)) {
        (None, None) => return [NO_RUN, NO_RUN],
        (left, right) => {
            let from = centre_x - left.unwrap_or(0.0);
            let to = centre_x + right.unwrap_or(0.0);
            let start = (whole::ceil(from - 0.5) as i32).max(min_x);
            let end = (whole::floor(to - 0.5) as i32 + 1).min(max_x);
            (start, end)
        }
    };
    match (empty_reach(left), empty_reach(right)) {
        (Some(left), Some(right)) => runs_around(run, centre_x - left, centre_x + right),
        (Some(left), None) => runs_around(run, centre_x - left, centre_x),
        // The right side starts past the centre: the centre's own column
        // belongs to the left one.
        (None, Some(right)) => runs_around(run, centre_x + 1.0 / 64.0, centre_x + right),
        (None, None) => [run, NO_RUN],
    }
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

/// A rounded rect to fill, with what every row of it shares worked out once.
struct RoundedFill {
    rect: Rect,
    /// A pixel whose centre lies in `flat_left..=flat_right`, between the
    /// corners' widest reach on either side, is inside on every row of the
    /// rect: an end there needs no test.
    flat_left: f32,
    flat_right: f32,
    top_left: Corner,
    top_right: Corner,
    bottom_right: Corner,
    bottom_left: Corner,
}

/// A rounded corner: a point in its square, the one between the circle's
/// centre and the rect's corner, lies outside when it lies outside the
/// circle. A corner without a radius has its centre at infinity on the side
/// away from its square, so no point lies in the square.
#[derive(Clone, Copy)]
struct Corner {
    centre_x: f32,
    centre_y: f32,
    /// The square of the radius.
    reach: f32,
}

impl Corner {
    /// `square_left` when the corner's square lies left of its centre.
    fn new(radius: f32, centre_x: f32, centre_y: f32, square_left: bool) -> Self {
        if radius > 0.0 {
            return Self {
                centre_x,
                centre_y,
                reach: radius.powi(2),
            };
        }
        Self {
            centre_x: if square_left {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            },
            centre_y,
            reach: f32::INFINITY,
        }
    }

    fn excludes(self, x: f32, y: f32) -> bool {
        (x - self.centre_x).powi(2) + (y - self.centre_y).powi(2) > self.reach
    }
}

impl RoundedFill {
    fn new(rect: Rect, radii: &CornerRadii) -> Self {
        let left = rect.x;
        let right = rect.x + rect.width;
        let top = rect.y;
        let bottom = rect.y + rect.height;
        Self {
            rect,
            flat_left: rect.x + radii.top_left.max(radii.bottom_left),
            flat_right: rect.x + rect.width - radii.top_right.max(radii.bottom_right),
            top_left: Corner::new(
                radii.top_left,
                left + radii.top_left,
                top + radii.top_left,
                true,
            ),
            top_right: Corner::new(
                radii.top_right,
                right - radii.top_right,
                top + radii.top_right,
                false,
            ),
            bottom_right: Corner::new(
                radii.bottom_right,
                right - radii.bottom_right,
                bottom - radii.bottom_right,
                false,
            ),
            bottom_left: Corner::new(
                radii.bottom_left,
                left + radii.bottom_left,
                bottom - radii.bottom_left,
                true,
            ),
        }
    }

    /// [`point_in_resolved_rounded_rect`] for a point of a row within the
    /// rect's height, from the corners worked out once: each test computes
    /// what that function computes, in the same order. Out of line, so the
    /// row loop around a fill stays small enough to keep inline.
    #[inline(never)]
    fn contains(&self, x: f32, y: f32) -> bool {
        let rect = self.rect;
        let (top_left, top_right) = (self.top_left, self.top_right);
        let (bottom_right, bottom_left) = (self.bottom_right, self.bottom_left);
        x >= rect.x
            && x <= rect.x + rect.width
            && !(x < top_left.centre_x && y < top_left.centre_y && top_left.excludes(x, y))
            && !(x > top_right.centre_x && y < top_right.centre_y && top_right.excludes(x, y))
            && !(x > bottom_right.centre_x
                && y > bottom_right.centre_y
                && bottom_right.excludes(x, y))
            && !(x < bottom_left.centre_x && y > bottom_left.centre_y && bottom_left.excludes(x, y))
    }

    /// The pixels of the row centred at `y` whose centres lie in the rounded
    /// rect, within `min_x..max_x`. The shape is convex, so they form one
    /// run; its ends lie at most a corner radius in from the rect's sides.
    fn run(&self, y: f32, min_x: i32, max_x: i32) -> (i32, i32) {
        let rect = self.rect;
        if !(y >= rect.y && y <= rect.y + rect.height) {
            return NO_RUN;
        }
        let inside = |x: i32| self.contains(x as f32 + 0.5, y);
        let flat = |x: i32| (self.flat_left..=self.flat_right).contains(&(x as f32 + 0.5));
        let mut start = min_x;
        if !(start < max_x && flat(start)) {
            while start < max_x && !inside(start) {
                start += 1;
            }
        }
        let mut end = max_x;
        if !(end > start && flat(end - 1)) {
            while end > start && !inside(end - 1) {
                end -= 1;
            }
        }
        (start, end)
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

/// The device pixels an image covers: its rect where it lands, or the box
/// of its turned rect.
fn image_rect(draw: &ImageDraw) -> Rect {
    if let Some(transform) = draw.transform {
        return transform.bounds_for_rect(draw.rect);
    }
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    draw.rect.translate(snap_delta.x, snap_delta.y)
}

fn draw_image(target: &mut Target, draw: &ImageDraw, diagnostics: &RenderDiagnostics) {
    let rect = image_rect(draw);
    if draw.alpha <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(clip_bounds) = clip_rect_to_bounds(rect, draw.clip, target) else {
        return;
    };
    let Some(sampler) = ImageSampler::of(draw) else {
        return;
    };
    // A turned image is read back through its transform: each pixel's
    // centre lands somewhere in its rect, or outside it.
    let to_image = match draw.transform {
        Some(transform) => match transform.inverse() {
            Some(inverse) => Some(inverse),
            None => return,
        },
        None => None,
    };
    let image = draw.rect;
    let local = |x: f32, y: f32| match to_image {
        Some(inverse) => {
            let point = inverse.map_point_in_front(Point::new(x, y))?;
            image.contains(point.x, point.y).then(|| {
                (
                    (point.x - image.x) / image.width,
                    (point.y - image.y) / image.height,
                )
            })
        }
        None => Some((
            ((x - rect.x) / rect.width).clamp(0.0, 1.0),
            ((y - rect.y) / rect.height).clamp(0.0, 1.0),
        )),
    };
    for py in clip_bounds.min_y..clip_bounds.max_y {
        let sample_y = py as f32 + 0.5;
        let row = target.row(py, clip_bounds.min_x, clip_bounds.max_x);
        for (px, dst) in (clip_bounds.min_x..).zip(row) {
            let Some((u, v)) = local(px as f32 + 0.5, sample_y) else {
                continue;
            };
            let sample = sampler.sample(u, v);
            if sample[3] > 0.0 {
                blend_pixel(dst, sample, draw.blend_mode, diagnostics);
            }
        }
    }
}

/// Reads an image's pixels at a place within its rect, with its sampling,
/// source rect, colour filter and alpha applied.
struct ImageSampler<'a> {
    draw: &'a ImageDraw,
    source: Rect,
}

impl<'a> ImageSampler<'a> {
    /// `None` for an image with no pixels of its own to read.
    fn of(draw: &'a ImageDraw) -> Option<Self> {
        let (width, height) = (draw.image.width(), draw.image.height());
        if width == 0
            || height == 0
            || draw.image.format() == cranpose_ui_graphics::ImagePixelFormat::External
        {
            return None;
        }
        let source = draw.src_rect.unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        });
        Some(Self { draw, source })
    }

    /// The colour at `u`, `v`, the fractions of the rect's width and height.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let Self { draw, source } = self;
        let image = &draw.image;
        let x = source.x + u * source.width;
        let y = source.y + v * source.height;
        let mut sample = match draw.sampling {
            cranpose_ui_graphics::ImageSampling::Nearest => {
                let src_x = (whole::floor(x) as i32).clamp(0, image.width() as i32 - 1);
                let src_y = (whole::floor(y) as i32).clamp(0, image.height() as i32 - 1);
                sample_image_nearest(image, src_x as u32, src_y as u32)
            }
            cranpose_ui_graphics::ImageSampling::Linear => {
                sample_image_linear(image, x - 0.5, y - 0.5)
            }
        };
        if let Some(filter) = &draw.color_filter {
            sample = filter.apply_rgba(sample);
        }
        sample[3] *= draw.alpha.clamp(0.0, 1.0);
        sample
    }
}

fn sample_image_nearest(
    image: &cranpose_ui_graphics::ImageBitmap,
    src_x: u32,
    src_y: u32,
) -> [f32; 4] {
    image
        .rgba8_pixel(src_y as usize * image.width() as usize + src_x as usize)
        .map(|channel| UNIT[usize::from(channel)])
}

fn sample_image_linear(image: &cranpose_ui_graphics::ImageBitmap, x: f32, y: f32) -> [f32; 4] {
    let img_width = image.width();
    let img_height = image.height();
    let x = x.clamp(0.0, img_width.saturating_sub(1) as f32);
    let y = y.clamp(0.0, img_height.saturating_sub(1) as f32);
    let x0 = whole::floor(x);
    let y0 = whole::floor(y);
    let tx = x - x0;
    let ty = y - y0;
    let x0 = (x0 as i32).clamp(0, img_width as i32 - 1) as u32;
    let y0 = (y0 as i32).clamp(0, img_height as i32 - 1) as u32;
    let x1 = (x0 + 1).min(img_width - 1);
    let y1 = (y0 + 1).min(img_height - 1);
    let top_left = sample_image_nearest(image, x0, y0);
    let top_right = sample_image_nearest(image, x1, y0);
    let bottom_left = sample_image_nearest(image, x0, y1);
    let bottom_right = sample_image_nearest(image, x1, y1);

    let mut out = [0.0; 4];
    for channel in 0..4 {
        let top = top_left[channel] + (top_right[channel] - top_left[channel]) * tx;
        let bottom = bottom_left[channel] + (bottom_right[channel] - bottom_left[channel]) * tx;
        out[channel] = top + (bottom - top) * ty;
    }
    out
}

fn draw_text(
    target: &mut Target,
    draw: &TextDraw,
    diagnostics: &RenderDiagnostics,
    text: &TextRaster,
) {
    if let Some((pixels, rect)) = rasterize_text(draw, text) {
        blit_rasterized_text(target, rect, draw.clip, pixels, diagnostics);
    }
}

/// `draw` rasterized into an image of its own, straight alpha, and the
/// rect that image covers.
pub(crate) fn rasterize_text(draw: &TextDraw, text: &TextRaster) -> Option<(TextPixels, Rect)> {
    let raster_rect = text_raster_rect(draw)?;
    let mut glyphs = text.glyphs.try_borrow_mut().ok();
    let pixels = rasterize_annotated_text_region_pixels(
        draw.text.as_ref(),
        raster_rect,
        Point::new(raster_rect.x, raster_rect.y),
        &draw.text_style,
        draw.color,
        draw.font_size,
        draw.scale,
        text.resources.fonts(),
        glyphs.as_deref_mut(),
    )?;
    let rect = Rect {
        x: raster_rect.x,
        y: raster_rect.y,
        width: pixels.width as f32,
        height: pixels.height as f32,
    };
    Some((pixels, rect))
}

fn text_raster_rect(draw: &TextDraw) -> Option<Rect> {
    if draw.scale.max(0.0) == 0.0 {
        return None;
    }
    let static_text_motion = draw
        .text_style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static;
    if !static_text_motion {
        return Some(draw.rect);
    }
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    Some(on_pixel_grid(
        draw.rect.translate(snap_delta.x, snap_delta.y),
    ))
}

fn blit_rasterized_text(
    target: &mut Target,
    rect: Rect,
    clip: Option<Rect>,
    text: TextPixels,
    diagnostics: &RenderDiagnostics,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 || text.width == 0 || text.height == 0 {
        return;
    }
    let Some(clip_bounds) = clip_rect_to_bounds(rect, clip, target) else {
        return;
    };

    // The pixels are the rect's size. On whole pixels each pixel takes the
    // text pixel under it; between pixels it takes the text moved by the
    // fraction, sampled linearly from it as an image.
    if rect.x.fract() != 0.0 || rect.y.fract() != 0.0 {
        if let Some(image) = text.into_image() {
            blit_image_between_pixels(target, rect, clip_bounds, &image, diagnostics);
        }
        return;
    }
    let source_pixels = text.pixels.as_chunks::<4>().0;
    for py in clip_bounds.min_y..clip_bounds.max_y {
        let row = target.row(py, clip_bounds.min_x, clip_bounds.max_x);
        let first = (py as f32 - rect.y) as usize * text.width as usize
            + (clip_bounds.min_x as f32 - rect.x) as usize;
        let source = source_pixels
            .get(first..first + row.len())
            .unwrap_or_default();
        for (dst, src) in row.iter_mut().zip(source) {
            blend_byte_pixel(dst, *src);
        }
    }
}

/// Draws `image`, the size of `rect`, at a rect between whole pixels: each
/// pixel samples the image linearly.
fn blit_image_between_pixels(
    target: &mut Target,
    rect: Rect,
    clip_bounds: ClipBounds,
    image: &cranpose_ui_graphics::ImageBitmap,
    diagnostics: &RenderDiagnostics,
) {
    for py in clip_bounds.min_y..clip_bounds.max_y {
        let image_y = py as f32 - rect.y;
        let row = target.row(py, clip_bounds.min_x, clip_bounds.max_x);
        for (px, dst) in (clip_bounds.min_x..).zip(row) {
            let src = sample_image_linear(image, px as f32 - rect.x, image_y);
            if src[3] <= 0.0 {
                continue;
            }
            blend_pixel(dst, src, BlendMode::SrcOver, diagnostics);
        }
    }
}

/// Draws a straight-alpha byte pixel over `dst`, source over, with the bytes
/// `blend_pixel` works out in floats. Each channel is the rational
/// `(s * a + d * (255 - a)) / 255` rounded: its fraction is a multiple of
/// 1/255, never one half, and the float error stays far below that gap, so
/// whole numbers round it the same way.
fn blend_byte_pixel(dst: &mut [u8; 4], src: [u8; 4]) {
    let alpha = u32::from(src[3]);
    match alpha {
        0 => {}
        255 => *dst = src,
        _ => {
            let keep = 255 - alpha;
            let mix = |source: u8, under: u8| {
                ((u32::from(source) * alpha + u32::from(under) * keep + 127) / 255) as u8
            };
            *dst = [
                mix(src[0], dst[0]),
                mix(src[1], dst[1]),
                mix(src[2], dst[2]),
                mix(255, dst[3]),
            ];
        }
    }
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
        dst[0] = whole::round(src[0].clamp(0.0, 1.0) * 255.0) as u8;
        dst[1] = whole::round(src[1].clamp(0.0, 1.0) * 255.0) as u8;
        dst[2] = whole::round(src[2].clamp(0.0, 1.0) * 255.0) as u8;
        dst[3] = 255;
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

    dst[0] = whole::round(out_r.clamp(0.0, 1.0) * 255.0) as u8;
    dst[1] = whole::round(out_g.clamp(0.0, 1.0) * 255.0) as u8;
    dst[2] = whole::round(out_b.clamp(0.0, 1.0) * 255.0) as u8;
    dst[3] = whole::round(out_a.clamp(0.0, 1.0) * 255.0) as u8;
}

#[cfg(test)]
#[path = "tests/draw_tests.rs"]
mod tests;
