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
    software_text_raster::{SoftwareGlyphRasterCache, rasterize_annotated_text_region},
    text_measure::SoftwareTextResources,
};
use cranpose_ui::text::TextMotion;
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, ColorFilter, CornerRadii, LineGeometry, Point, Rect, StrokeJoin,
    Trapezoid, expand_rect,
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
    Point::new(anchor.x.round(), anchor.y.round())
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

#[derive(Clone, Copy)]
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

    let min_x = (min_x.floor() as i32).clamp(window.min_x, window.max_x);
    let min_y = (min_y.floor() as i32).clamp(window.min_y, window.max_y);
    let max_x = (max_x.ceil() as i32).clamp(window.min_x, window.max_x);
    let max_y = (max_y.ceil() as i32).clamp(window.min_y, window.max_y);

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
        let raster_scene = pipeline::build_raster_scene(graph, scene.diagnostics(), scale);
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
    frame.as_chunks_mut::<4>().0.fill([18, 18, 24, 255]);
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
    clear_frame(target.pixels);
    let window = target.window();
    for &item in &scene.order {
        if scene.item_bounds(item, window).is_some() {
            draw_item(target, scene, item, diagnostics, text);
        }
    }
}

/// Draws one item of `scene` into `target`.
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
            draw_text(target, &scene.texts[index], diagnostics, text);
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
            RenderItem::Image(index) => {
                let draw = &self.images[index];
                let snap = draw
                    .snap_anchor
                    .map(snap_delta_for_anchor)
                    .unwrap_or_default();
                Some(draw.rect.translate(snap.x, snap.y))
            }
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
            RenderItem::Text(index) => {
                let draw = &self.texts[index];
                let raster_rect = text_raster_rect(draw)?;
                let reach = Rect {
                    width: raster_rect.width.ceil().max(1.0),
                    height: raster_rect.height.ceil().max(1.0),
                    ..raster_rect
                };
                clip_rect_to_window(reach, draw.clip, window)
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
        x: rect.x.round(),
        y: rect.y.round(),
        width: if rect.width > 0.0 {
            rect.width.ceil().max(1.0)
        } else {
            rect.width
        },
        height: if rect.height > 0.0 {
            rect.height.ceil().max(1.0)
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

    let opaque = opaque_solid_bytes(&draw.brush, draw.blend_mode);
    let fills_runs = opaque.filter(|_| coverage.covers_runs_whole());
    for py in clip_bounds.min_y..clip_bounds.max_y {
        let center_y = py as f32 + 0.5;
        for (start, end) in coverage.row_runs(center_y, clip_bounds.min_x, clip_bounds.max_x) {
            let row = target.row(py, start, end);
            if let Some(bytes) = fills_runs {
                row.fill(bytes);
                continue;
            }
            for (px, dst) in (start..).zip(row) {
                let center_x = px as f32 + 0.5;
                let coverage = coverage.at(Point::new(center_x, center_y));
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
                let alpha = sample[3];
                if alpha <= 0.0 {
                    continue;
                }
                blend_pixel(dst, sample, draw.blend_mode, diagnostics);
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
    let rgba = color_to_rgba(*color);
    (blend_mode == BlendMode::SrcOver && rgba[3] >= 1.0).then(|| {
        [
            (rgba[0] * 255.0).round() as u8,
            (rgba[1] * 255.0).round() as u8,
            (rgba[2] * 255.0).round() as u8,
            255,
        ]
    })
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
    let start = ((centre - half - 0.5).ceil() as i32).max(min);
    let end = ((centre + half - 0.5).floor() as i32 + 1).min(max);
    (start, end)
}

/// `run` without the pixels whose centres lie in `from..=to`.
fn runs_around(run: (i32, i32), from: f32, to: f32) -> RowRuns {
    let first = (from - 0.5).ceil() as i32;
    let last = (to - 0.5).floor() as i32;
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
            let start = ((from - 0.5).ceil() as i32).max(min_x);
            let end = ((to - 0.5).floor() as i32 + 1).min(max_x);
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

    /// The pixels of the row centred at `y` that can take ink, within
    /// `min_x..max_x`.
    fn row_runs(&self, y: f32, min_x: i32, max_x: i32) -> RowRuns {
        match self {
            Self::Arc(arc) => arc_row_runs(arc, y, min_x, max_x),
            Self::Stroke {
                rect,
                radii,
                half_width,
                join,
            } => stroke_row_runs(*rect, *radii, *half_width, *join, y, min_x, max_x),
            Self::Fill {
                rect,
                radii: Some(radii),
            } => [rounded_fill_run(*rect, radii, y, min_x, max_x), NO_RUN],
            _ => [(min_x, max_x), NO_RUN],
        }
    }

    /// Whether every pixel of its row runs is covered whole: a fill, whose
    /// runs hold exactly the pixels it covers.
    fn covers_runs_whole(&self) -> bool {
        matches!(self, Self::Fill { .. })
    }

    /// The coverage of the pixel centred at `point`, which lies in one of its
    /// row's runs.
    fn at(&self, point: Point) -> f32 {
        match self {
            Self::Line(line) => line.coverage(point),
            Self::Trapezoid(trapezoid) => trapezoid.coverage(point),
            Self::Arc(arc) => shape_sdf::arc_coverage(point, arc),
            Self::Stroke {
                rect,
                radii,
                half_width,
                join,
            } => shape_sdf::stroked_rect_coverage(point, *rect, *radii, *half_width, *join),
            Self::Fill { .. } => 1.0,
        }
    }
}

/// The pixels of the row centred at `y` whose centres lie in the rounded
/// rect, within `min_x..max_x`. The shape is convex, so they form one run;
/// its ends lie at most a corner radius in from the rect's sides.
fn rounded_fill_run(rect: Rect, radii: &CornerRadii, y: f32, min_x: i32, max_x: i32) -> (i32, i32) {
    if !(y >= rect.y && y <= rect.y + rect.height) {
        return NO_RUN;
    }
    let inside = |x: i32| point_in_resolved_rounded_rect(x as f32 + 0.5, y, rect, radii);
    let mut start = min_x;
    while start < max_x && !inside(start) {
        start += 1;
    }
    let mut end = max_x;
    while end > start && !inside(end - 1) {
        end -= 1;
    }
    (start, end)
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

fn image_rect(draw: &ImageDraw) -> Rect {
    let snap_delta = draw
        .snap_anchor
        .map(snap_delta_for_anchor)
        .unwrap_or_default();
    draw.rect.translate(snap_delta.x, snap_delta.y)
}

fn draw_image(target: &mut Target, draw: &ImageDraw, diagnostics: &RenderDiagnostics) {
    let rect = image_rect(draw);
    let clip = draw.clip;

    if draw.alpha <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }

    let Some(clip_bounds) = clip_rect_to_bounds(rect, clip, target) else {
        return;
    };

    let img_width = draw.image.width();
    let img_height = draw.image.height();
    if img_width == 0
        || img_height == 0
        || draw.image.format() == cranpose_ui_graphics::ImagePixelFormat::External
    {
        return;
    }

    let (sr_x, sr_y, sr_w, sr_h) = if let Some(sr) = draw.src_rect {
        (sr.x, sr.y, sr.width, sr.height)
    } else {
        (0.0, 0.0, img_width as f32, img_height as f32)
    };

    for py in clip_bounds.min_y..clip_bounds.max_y {
        let sample_y = py as f32 + 0.5;
        let v = ((sample_y - rect.y) / rect.height).clamp(0.0, 1.0);
        let row = target.row(py, clip_bounds.min_x, clip_bounds.max_x);
        for (px, dst) in (clip_bounds.min_x..).zip(row) {
            let sample_x = px as f32 + 0.5;
            let u = ((sample_x - rect.x) / rect.width).clamp(0.0, 1.0);

            let mut sample = match draw.sampling {
                cranpose_ui_graphics::ImageSampling::Nearest => {
                    let src_x = ((sr_x + u * sr_w).floor() as i32).clamp(0, img_width as i32 - 1);
                    let src_y = ((sr_y + v * sr_h).floor() as i32).clamp(0, img_height as i32 - 1);
                    sample_image_nearest(&draw.image, src_x as u32, src_y as u32)
                }
                cranpose_ui_graphics::ImageSampling::Linear => {
                    sample_image_linear(&draw.image, sr_x + u * sr_w - 0.5, sr_y + v * sr_h - 0.5)
                }
            };

            if let Some(filter) = draw.color_filter {
                sample = apply_color_filter(sample, filter);
            }

            sample[3] *= draw.alpha.clamp(0.0, 1.0);
            if sample[3] <= 0.0 {
                continue;
            }

            blend_pixel(dst, sample, draw.blend_mode, diagnostics);
        }
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
    let x0 = x.floor();
    let y0 = y.floor();
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
    if let Some((image, rect)) = rasterize_text(draw, text) {
        blit_rasterized_text_image(target, rect, draw.clip, &image, diagnostics);
    }
}

/// `draw` rasterized into an image of its own, straight alpha, and the
/// rect that image covers.
pub(crate) fn rasterize_text(
    draw: &TextDraw,
    text: &TextRaster,
) -> Option<(cranpose_ui_graphics::ImageBitmap, Rect)> {
    let raster_rect = text_raster_rect(draw)?;
    let mut glyphs = text.glyphs.try_borrow_mut().ok();
    let image = rasterize_annotated_text_region(
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
        width: image.width() as f32,
        height: image.height() as f32,
    };
    Some((image, rect))
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

fn blit_rasterized_text_image(
    target: &mut Target,
    rect: Rect,
    clip: Option<Rect>,
    image: &cranpose_ui_graphics::ImageBitmap,
    diagnostics: &RenderDiagnostics,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(clip_bounds) = clip_rect_to_bounds(rect, clip, target) else {
        return;
    };

    let img_width = image.width();
    let img_height = image.height();
    if img_width == 0 || img_height == 0 {
        return;
    }

    // The image is the rect's size. On whole pixels each pixel takes the
    // image pixel under it; between pixels it takes the image moved by the
    // fraction, sampled linearly.
    let whole = rect.x.fract() == 0.0 && rect.y.fract() == 0.0;
    for py in clip_bounds.min_y..clip_bounds.max_y {
        let image_y = py as f32 - rect.y;
        let row = target.row(py, clip_bounds.min_x, clip_bounds.max_x);
        for (px, dst) in (clip_bounds.min_x..).zip(row) {
            let image_x = px as f32 - rect.x;
            let src = if whole {
                let index = image_y as usize * img_width as usize + image_x as usize;
                image
                    .rgba8_pixel(index)
                    .map(|channel| UNIT[usize::from(channel)])
            } else {
                sample_image_linear(image, image_x, image_y)
            };
            if src[3] <= 0.0 {
                continue;
            }
            blend_pixel(dst, src, BlendMode::SrcOver, diagnostics);
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
        dst[0] = (src[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        dst[1] = (src[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        dst[2] = (src[2].clamp(0.0, 1.0) * 255.0).round() as u8;
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

    dst[0] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn apply_color_filter(sample: [f32; 4], filter: ColorFilter) -> [f32; 4] {
    filter.apply_rgba(sample)
}

#[cfg(test)]
#[path = "tests/draw_tests.rs"]
mod tests;
