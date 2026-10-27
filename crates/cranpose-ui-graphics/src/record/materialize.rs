//! How a recording hands out the primitives and brushes its tables hold.

use super::{
    BRUSH_KIND_RADIAL, BRUSH_KIND_SWEEP, BlendMode, Brush, Color, CornerRadii, DrawPrimitive,
    GradientLists, GroupMarker, Point, RECORD_KIND_ARC, RECORD_KIND_LINE, RECORD_KIND_ROUND_RECT,
    RecordLane, RecordSegment, RecordingContent, ShapeRecord, ShapeRecorder, TILE_MODES, row_rect,
};

// Every primitive of a recording's parts in recorded order, content
// markers included.
pub(super) fn parts_primitives_with_markers<'a>(
    shapes: &'a ShapeRecorder,
    content: &'a RecordingContent,
) -> impl Iterator<Item = DrawPrimitive> + use<'a> {
    shapes.tables.segments.iter().flat_map(move |segment| {
        parts_segment_primitives(shapes, content, segment, ContentMarkers::Included)
    })
}

// Whether a walk of a recording's segments hands out its content markers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ContentMarkers {
    Included,
    Skipped,
}

// The primitives of `segment` of a recording's parts.
pub(super) fn parts_segment_primitives<'a>(
    shapes: &'a ShapeRecorder,
    content: &'a RecordingContent,
    segment: &RecordSegment,
    markers: ContentMarkers,
) -> impl Iterator<Item = DrawPrimitive> + use<'a> {
    let lane = segment.lane;
    segment.range().filter_map(move |index| match lane {
        RecordLane::Shapes => Some(materialize_shape(shapes, index)),
        RecordLane::Others => Some(content.others[index].clone()),
        RecordLane::Content => {
            (markers == ContentMarkers::Included).then_some(DrawPrimitive::Content)
        }
        RecordLane::GroupStart => Some(DrawPrimitive::Group(GroupMarker::Start(
            content.groups[index],
        ))),
        RecordLane::GroupEnd => Some(DrawPrimitive::Group(GroupMarker::End)),
    })
}

// The exact [`DrawPrimitive`] record `index` of `shapes` was made from.
pub(super) fn materialize_shape(shapes: &ShapeRecorder, index: usize) -> DrawPrimitive {
    let record = shapes
        .tables
        .shapes
        .get(index)
        .expect("recorded shape index");
    let rect = record.rect_value();
    let brush = brush_of(shapes, &record);
    let stroke = record.stroke();
    let primitive = match (record.kind(), record.trapezoid()) {
        (_, Some(trapezoid)) => DrawPrimitive::Trapezoid {
            rect: row_rect(record.radii),
            brush,
            trapezoid,
        },
        (RECORD_KIND_ROUND_RECT, _) => DrawPrimitive::RoundRect {
            rect,
            brush,
            radii: CornerRadii {
                top_left: record.radii[0],
                top_right: record.radii[1],
                bottom_right: record.radii[2],
                bottom_left: record.radii[3],
            },
            stroke,
        },
        (RECORD_KIND_LINE, _) => DrawPrimitive::Line {
            rect,
            brush,
            start: Point::new(record.arc[0], record.arc[1]),
            end: Point::new(record.arc_band[2], record.arc_band[3]),
            stroke: stroke.unwrap_or_default(),
        },
        (RECORD_KIND_ARC, _) => DrawPrimitive::Arc {
            rect,
            brush,
            center: Point::new(record.arc[0], record.arc[1]),
            radius: record.arc[2],
            start_angle: record.arc_band[0],
            sweep_angle: record.arc_band[1],
            stroke,
            inner_radius: record.arc[3],
        },
        _ => DrawPrimitive::Rect {
            rect,
            brush,
            stroke,
        },
    };
    let blend_mode = record.blend_mode();
    if blend_mode == BlendMode::SrcOver {
        primitive
    } else {
        DrawPrimitive::Blend {
            primitive: Box::new(primitive),
            blend_mode,
        }
    }
}

// The brush of a record of `shapes`: its solid colour, or the gradient
// rebuilt from the tables exactly as the app gave it.
pub(super) fn brush_of(shapes: &ShapeRecorder, record: &ShapeRecord) -> Brush {
    if record.brush == 0 {
        return Brush::Solid(Color(
            record.color[0],
            record.color[1],
            record.color[2],
            record.color[3],
        ));
    }
    let index = record.brush as usize - 1;
    let brush = &shapes.tables.brushes[index];
    let GradientLists { colors, stops } = shapes.gradient_lists[index].clone();
    let [a, b, c, d] = brush.params;
    let tile_mode = TILE_MODES[brush.tile_mode as usize];
    match brush.kind {
        BRUSH_KIND_RADIAL => Brush::RadialGradient {
            colors,
            stops,
            center: Point::new(a, b),
            radius: c,
            tile_mode,
        },
        BRUSH_KIND_SWEEP => Brush::SweepGradient {
            colors,
            stops,
            center: Point::new(a, b),
        },
        _ => Brush::LinearGradient {
            colors,
            stops,
            start: Point::new(a, b),
            end: Point::new(c, d),
            tile_mode,
        },
    }
}
