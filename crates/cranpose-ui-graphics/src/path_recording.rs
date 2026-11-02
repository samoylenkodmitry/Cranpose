// How a draw scope records a path: a fill as the slices the GPU draws, a
// stroke as lines, or as the fill of its outline where its lines' overlaps
// would show, and the cached coverage mask a path that cannot be sliced
// falls back to.

use crate::{
    BlendMode, Brush, CommandRecorder, DashPathEffect, DrawPrimitive, ImageSampling, PathContour,
    PathFillRule, Point, Rect, Size, Stroke, TileMode, VectorPath,
    path::{DrawStyle, Path},
    stroke::LineGeometry,
    vector_path_mask::{
        PathSide, VECTOR_PATH_SUPERSAMPLE, rasterize_vector_path, vector_path_mask_cache_get,
        vector_path_mask_cache_put, vector_path_mask_key,
    },
};

pub(crate) fn record_path(
    recording: &mut CommandRecorder<'_>,
    size: Size,
    path: &Path,
    brush: &Brush,
    style: DrawStyle,
    blend_mode: BlendMode,
) {
    let (stroke, dash) = match style {
        DrawStyle::Fill => {
            record_path_fill(recording, size, path.contours(), brush, blend_mode);
            return;
        }
        DrawStyle::Stroke(stroke) => (stroke, None),
        DrawStyle::DashedStroke(stroke, dash) => (stroke, Some(dash)),
    };
    if !stroke.is_visible() {
        return;
    }
    if overlaps_show(brush, blend_mode) && !is_one_segment(path) {
        record_stroke_outline(recording, size, path, (stroke, dash), brush, blend_mode);
        return;
    }
    let mut run = recording.line_run(brush, stroke, blend_mode);
    for_each_line(path, stroke, dash, |line| {
        run.push(line.start, line.end, line.cap);
    });
}

// Whether the lines of a stroke show where they overlap: a brush that is not
// opaque paints an overlap twice as deep, and a blend other than plain
// source-over reads what the line beneath drew.
fn overlaps_show(brush: &Brush, blend_mode: BlendMode) -> bool {
    blend_mode != BlendMode::SrcOver
        || match brush {
            Brush::Solid(color) => color.3 < 1.0,
            Brush::LinearGradient {
                tile_mode: TileMode::Decal,
                ..
            }
            | Brush::RadialGradient {
                tile_mode: TileMode::Decal,
                ..
            } => true,
            gradient => gradient.colors().iter().any(|color| color.3 < 1.0),
        }
}

// Whether `path` is one open segment, whose line, or dashes, cannot overlap.
fn is_one_segment(path: &Path) -> bool {
    matches!(path.contours(), [contour] if !contour.closed && contour.points.len() <= 2)
}

// Records a stroke as one fill of what its lines cover, so it paints each
// point once.
fn record_stroke_outline(
    recording: &mut CommandRecorder<'_>,
    size: Size,
    path: &Path,
    (stroke, dash): (Stroke, Option<DashPathEffect>),
    brush: &Brush,
    blend_mode: BlendMode,
) {
    let mut outline = crate::thread_state::with(|state| state.stroke_outline.take());
    outline.clear();
    for_each_line(path, stroke, dash, |line| outline.add_line(line));
    record_path_fill(recording, size, outline.contours(), brush, blend_mode);
    crate::thread_state::with(|state| state.stroke_outline.replace(outline));
}

// Calls `line` with each line a stroke of `path` draws, dashed by `dash`.
fn for_each_line(
    path: &Path,
    stroke: Stroke,
    dash: Option<DashPathEffect>,
    mut line: impl FnMut(LineGeometry),
) {
    for contour in path.contours() {
        let Some(dash) = dash else {
            crate::for_each_stroke_line(&contour.points, contour.closed, stroke, &mut line);
            continue;
        };
        let closing = contour
            .closed
            .then(|| contour.points.first().copied())
            .flatten();
        let points: std::borrow::Cow<'_, [Point]> = match closing {
            Some(first) => {
                std::borrow::Cow::Owned(contour.points.iter().copied().chain([first]).collect())
            }
            None => std::borrow::Cow::Borrowed(&contour.points),
        };
        dash.for_each_dash(&points, |piece| {
            crate::for_each_stroke_line(piece, false, stroke, &mut line);
        });
    }
}

/// Records the fill of `path` as the slices the GPU draws, its brush
/// resolved against the scope's rect; a path that cannot be sliced is
/// rasterized into a mask instead.
// A path's fill and a stroke's outline both come here, so the slicer keeps
// one caller.
fn record_path_fill(
    recording: &mut CommandRecorder<'_>,
    size: Size,
    path: &[PathContour],
    brush: &Brush,
    blend_mode: BlendMode,
) {
    let brush_rect = Rect::from_size(size);
    let sliced = crate::thread_state::with(|state| {
        let mut slicer = state.path_slicer.borrow_mut();
        let contours = path.iter().map(|contour| contour.points.as_slice());
        slicer
            .slice(contours, PathFillRule::NonZero)
            .map(|slices| recording.push_trapezoids(brush_rect, slices, brush, blend_mode))
            .is_some()
    });
    if !sliced {
        record_vector_path(
            recording,
            &crate::path::contours_vector_path(path, PathFillRule::NonZero),
            brush,
            brush_rect,
            blend_mode,
            PathSide::Inside,
        );
    }
}

/// Rasterizes the `side` of `path`'s edge into a cached image and
/// records it, blended by `blend_mode`. A gradient `brush` resolves
/// against `brush_rect`.
pub(crate) fn record_vector_path(
    recording: &mut CommandRecorder<'_>,
    path: &VectorPath,
    brush: &Brush,
    brush_rect: Rect,
    blend_mode: BlendMode,
    side: PathSide,
) {
    const MAX_MASK_PIXELS: f32 = 4096.0;

    if path.is_empty() || !brush.paints() {
        return;
    }
    let bounds = path.bounds();
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }

    let rect = vector_mask_rect(bounds);
    let origin = Point::new(rect.x, rect.y);
    let mask_width = (rect.width * VECTOR_PATH_SUPERSAMPLE)
        .ceil()
        .clamp(1.0, MAX_MASK_PIXELS) as usize;
    let mask_height = (rect.height * VECTOR_PATH_SUPERSAMPLE)
        .ceil()
        .clamp(1.0, MAX_MASK_PIXELS) as usize;
    let mask_size = (mask_width, mask_height);

    let key = vector_path_mask_key(path, origin, mask_size, brush, brush_rect, side);
    let image = match vector_path_mask_cache_get(key) {
        Some(image) => image,
        None => {
            let Some(image) =
                rasterize_vector_path(path, brush, brush_rect, origin, mask_size, side)
            else {
                return;
            };
            vector_path_mask_cache_put(key, image.clone());
            image
        }
    };

    let image = DrawPrimitive::Image {
        rect,
        image,
        alpha: 1.0,
        color_filter: None,
        sampling: ImageSampling::Linear,
        src_rect: None,
    };
    recording.push_other(if blend_mode == BlendMode::SrcOver {
        image
    } else {
        DrawPrimitive::Blend {
            primitive: Box::new(image),
            blend_mode,
        }
    });
}

/// The rect a path's coverage mask covers: its bounds out to whole pixels and
/// one pixel more each way, so the anti-aliased edge fits.
pub(crate) fn vector_mask_rect(bounds: Rect) -> Rect {
    let x = bounds.x.floor() - 1.0;
    let y = bounds.y.floor() - 1.0;
    Rect {
        x,
        y,
        width: (bounds.x + bounds.width).ceil() - x + 1.0,
        height: (bounds.y + bounds.height).ceil() - y + 1.0,
    }
}
