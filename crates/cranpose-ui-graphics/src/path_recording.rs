// How a draw scope records a path: a fill as the slices the GPU draws, a
// stroke as lines, and the cached coverage mask a path that cannot be sliced
// falls back to.

use crate::{
    BlendMode, Brush, CommandRecorder, DrawPrimitive, ImageSampling, PathFillRule, Point, Rect,
    Size, VectorPath,
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
    brush: Brush,
    style: DrawStyle,
    blend_mode: BlendMode,
) {
    let (stroke, dash) = match style {
        DrawStyle::Fill => {
            record_path_fill(recording, size, path, &brush, blend_mode);
            return;
        }
        DrawStyle::Stroke(stroke) => (stroke, None),
        DrawStyle::DashedStroke(stroke, dash) => (stroke, Some(dash)),
    };
    if !stroke.is_visible() {
        return;
    }
    let mut run = recording.line_run(&brush, stroke, blend_mode);
    let mut push = |line: LineGeometry| run.push(line.start, line.end, line.cap);
    for contour in path.contours() {
        let Some(dash) = dash else {
            crate::for_each_stroke_line(&contour.points, contour.closed, stroke, &mut push);
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
            crate::for_each_stroke_line(piece, false, stroke, &mut push);
        });
    }
}

/// Records the fill of `path` as the slices the GPU draws, its brush
/// resolved against the scope's rect; a path that cannot be sliced is
/// rasterized into a mask instead.
fn record_path_fill(
    recording: &mut CommandRecorder<'_>,
    size: Size,
    path: &Path,
    brush: &Brush,
    blend_mode: BlendMode,
) {
    let brush_rect = Rect::from_size(size);
    let sliced = crate::thread_state::with(|state| {
        let mut slicer = state.path_slicer.borrow_mut();
        let contours = path
            .contours()
            .iter()
            .map(|contour| contour.points.as_slice());
        slicer
            .slice(contours, PathFillRule::NonZero)
            .map(|slices| recording.push_trapezoids(brush_rect, slices, brush, blend_mode))
            .is_some()
    });
    if !sliced {
        record_vector_path(
            recording,
            &path.to_vector_path(PathFillRule::NonZero),
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
