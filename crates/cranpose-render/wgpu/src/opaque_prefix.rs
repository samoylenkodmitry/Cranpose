use std::hash::{Hash, Hasher};

use cranpose_render_common::{
    graph::DrawCommandId,
    raster_cache::{LayerRasterCacheKey, ScaleBucket},
};
use cranpose_ui_graphics::{
    BRUSH_KIND_LINEAR, BRUSH_KIND_RADIAL, BRUSH_KIND_SWEEP, BlendMode, BrushRecord,
    GradientStopRecord, Point, RECORD_KIND_RECT, RecordLane, RecordTables, Rect, ShapeRecord,
    TileMode,
};

use crate::{
    capture_hash::capture_hasher,
    geometry::{canonicalized_scaled_rect, snap_delta_for_anchor},
    render::hash_f32_for_cache,
    scene::{CompositorScene, DrawOp, DrawOpKind, RunDraw},
};

pub(crate) struct OpaquePrefix {
    pub(crate) key: LayerRasterCacheKey,
    pub(crate) command: DrawCommandId,
    pub(crate) z_index: usize,
    pub(crate) device_rect: (f32, f32, f32, f32),
}

pub(crate) struct PrefixContext<'a> {
    pub(crate) scene: &'a CompositorScene,
    pub(crate) base: wgpu::LoadOp<wgpu::Color>,
    pub(crate) page_offset: [f32; 2],
    pub(crate) page_size: (u32, u32),
    pub(crate) scale: f32,
    pub(crate) format: wgpu::TextureFormat,
}

struct Candidate<'a> {
    run: &'a RunDraw,
    command: DrawCommandId,
    record: ShapeRecord,
    brush: Option<&'a BrushRecord>,
    stops: &'a [GradientStopRecord],
    explicit: &'a [f32],
}

struct Edges {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Edges {
    fn is_area(&self) -> bool {
        self.right > self.left && self.bottom > self.top
    }
}

fn canonical(value: f32) -> f32 {
    value.signum() * (value.abs() * 16.0 + 0.5).floor() / 16.0
}

fn whole(value: f32) -> Option<f32> {
    (value.is_finite() && value.fract() == 0.0).then_some(value)
}

fn hash_rect<H: Hasher>(rect: Rect, state: &mut H) {
    for value in [rect.x, rect.y, rect.width, rect.height] {
        hash_f32_for_cache(value, state);
    }
}

fn plain_rect(record: &ShapeRecord) -> bool {
    record.blend_mode() == BlendMode::SrcOver
        && record.kind() == RECORD_KIND_RECT
        && !record.is_stroked()
        && record.radii == [0.0; 4]
}

fn brush_stops<'a>(
    tables: &'a RecordTables,
    brush: &BrushRecord,
) -> Option<(&'a [GradientStopRecord], &'a [f32])> {
    let start = brush.stop_start as usize;
    let stops = tables.stops.get(start..start + brush.stop_count as usize)?;
    let explicit = if brush.explicit_len == u32::MAX {
        &[][..]
    } else {
        let start = brush.explicit_start as usize;
        tables
            .explicit_stops
            .get(start..start + brush.explicit_len as usize)?
    };
    Some((stops, explicit))
}

fn candidate<'a>(scene: &'a CompositorScene, op: &DrawOp) -> Option<Candidate<'a>> {
    let DrawOpKind::Run(index) = op.kind else {
        return None;
    };
    let run = &scene.runs[index];
    let command = run.command?;
    if run.placement.alpha != 1.0 || run.placement.color_filter.is_some() {
        return None;
    }
    let tables = run.tables();
    let segment = tables.segments.get(run.segments.start as usize)?;
    let record = tables.shapes.get(segment.start as usize)?;
    if segment.lane != RecordLane::Shapes
        || segment.blend != BlendMode::SrcOver
        || !plain_rect(&record)
    {
        return None;
    }
    let brush = match record.brush {
        0 => None,
        index => Some(tables.brushes.get(index as usize - 1)?),
    };
    let (stops, explicit) = match brush {
        Some(brush) => brush_stops(tables, brush)?,
        None => (&[][..], &[][..]),
    };
    Some(Candidate {
        run,
        command,
        record,
        brush,
        stops,
        explicit,
    })
}

fn is_opaque(candidate: &Candidate<'_>) -> bool {
    match candidate.brush {
        None => candidate.record.color[3] == 1.0,
        Some(brush) => {
            matches!(
                brush.kind,
                BRUSH_KIND_LINEAR | BRUSH_KIND_RADIAL | BRUSH_KIND_SWEEP
            ) && brush.tile_mode == TileMode::Clamp as u32
                && !candidate.stops.is_empty()
                && candidate.stops.iter().all(|stop| stop.color[3] == 1.0)
        }
    }
}

fn device_edges(rect: [f32; 4], offset: Point, scale: f32, canonicalize: bool) -> Option<Edges> {
    let edge = |value: f32| {
        let device = value * scale;
        whole(if canonicalize {
            canonical(device)
        } else {
            device
        })
    };
    let [x, y, width, height] = rect;
    let edges = Edges {
        left: edge(x + offset.x)?,
        top: edge(y + offset.y)?,
        right: edge(x + width + offset.x)?,
        bottom: edge(y + height + offset.y)?,
    };
    edges.is_area().then_some(edges)
}

fn clip_contains(clip: Rect, edges: &Edges, scale: f32, canonicalize: bool) -> bool {
    let device = if canonicalize {
        canonicalized_scaled_rect(clip, scale)
    } else {
        Rect {
            x: clip.x * scale,
            y: clip.y * scale,
            width: clip.width * scale,
            height: clip.height * scale,
        }
    };
    device.x <= edges.left
        && device.y <= edges.top
        && device.x + device.width >= edges.right
        && device.y + device.height >= edges.bottom
}

fn clamp_to_page(edges: Edges, context: &PrefixContext<'_>) -> Option<Edges> {
    let [page_x, page_y] = context.page_offset;
    whole(page_x)?;
    whole(page_y)?;
    let clamped = Edges {
        left: edges.left.max(page_x),
        top: edges.top.max(page_y),
        right: edges.right.min(page_x + context.page_size.0 as f32),
        bottom: edges.bottom.min(page_y + context.page_size.1 as f32),
    };
    clamped.is_area().then_some(clamped)
}

fn prefix_hash(
    candidate: &Candidate<'_>,
    snap: Point,
    context: &PrefixContext<'_>,
    edges: &Edges,
) -> u64 {
    let mut hasher = capture_hasher();
    bytemuck::bytes_of(&candidate.record).hash(&mut hasher);
    if let Some(brush) = candidate.brush {
        bytemuck::bytes_of(brush).hash(&mut hasher);
        bytemuck::cast_slice::<_, u8>(candidate.stops).hash(&mut hasher);
        for position in candidate.explicit {
            hash_f32_for_cache(*position, &mut hasher);
        }
    }
    let placement = &candidate.run.placement;
    for value in [
        placement.offset.x,
        placement.offset.y,
        snap.x,
        snap.y,
        context.scale,
    ] {
        hash_f32_for_cache(value, &mut hasher);
    }
    match placement.clip {
        Some(clip) => {
            1u8.hash(&mut hasher);
            hash_rect(clip, &mut hasher);
        }
        None => 0u8.hash(&mut hasher),
    }
    match context.base {
        wgpu::LoadOp::Clear(color) => {
            1u8.hash(&mut hasher);
            for channel in [color.r, color.g, color.b, color.a] {
                channel.to_bits().hash(&mut hasher);
            }
        }
        _ => 0u8.hash(&mut hasher),
    }
    std::mem::discriminant(&context.format).hash(&mut hasher);
    let [page_x, page_y] = context.page_offset;
    for value in [
        edges.left,
        edges.top,
        edges.right,
        edges.bottom,
        page_x,
        page_y,
    ] {
        hash_f32_for_cache(value, &mut hasher);
    }
    hasher.finish()
}

pub(crate) fn opaque_prefix(context: &PrefixContext<'_>, ops: &[DrawOp]) -> Option<OpaquePrefix> {
    let op = ops.first()?;
    let candidate = candidate(context.scene, op)?;
    if !is_opaque(&candidate) {
        return None;
    }
    let placement = &candidate.run.placement;
    let scale = context.scale;
    let snap = placement
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, scale))
        .unwrap_or_default();
    let canonicalize = placement.snap_anchor.is_some();
    let offset = Point {
        x: placement.offset.x + snap.x,
        y: placement.offset.y + snap.y,
    };
    let edges = device_edges(candidate.record.rect, offset, scale, canonicalize)?;
    if placement
        .clip
        .is_some_and(|clip| !clip_contains(clip, &edges, scale, canonicalize))
    {
        return None;
    }
    let edges = clamp_to_page(edges, context)?;
    let hash = prefix_hash(&candidate, snap, context, &edges);
    let width = edges.right - edges.left;
    let height = edges.bottom - edges.top;
    Some(OpaquePrefix {
        key: LayerRasterCacheKey::prefix_snapshot(
            hash,
            1,
            Rect {
                x: edges.left,
                y: edges.top,
                width,
                height,
            },
            (width as u32, height as u32),
            ScaleBucket::from_scale(scale),
        ),
        command: candidate.command,
        z_index: op.z_index,
        device_rect: (edges.left, edges.top, width, height),
    })
}
