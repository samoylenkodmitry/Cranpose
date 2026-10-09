use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use cranpose_core::collections::map::{Entry, HashMap};
use cranpose_render_common::{graph::DrawCommandId, style_shared::apply_layer_to_color};
use cranpose_ui_graphics::{
    ARC_BUCKETS, BrushRecord, Color, FRAGMENT_KIND_ARC, GradientStopRecord, GraphicsLayer,
    RecordLane, RecordSegment, RecordTables, ShapeRecordBody, ShapeRecordCurve,
    band_class_segments, strip_index_pattern, strip_indices, strip_vertices,
};
use smallvec::SmallVec;

use crate::{
    arc_trig_fill::{ArcTrigFill, TrigBindings},
    frame_graph::{
        FrameCommandRecorder, FrameCommandStats, MapRequests, MappedBuffers, MappedPool,
        TEST_READBACK, UploadMode, UploadPlacement, align_u64_to, create_filled_buffer,
        place_upload, write_mapped,
    },
    geometry::{
        SegmentTransform, canonicalized_scaled_rect, snap_delta_for_anchor,
        snapped_anchor_device_origin,
    },
    idle_pool::IDLE_FRAMES,
    run_geometry::ShapeFill,
    scene::{Placement, RunDraw},
};

pub(crate) const RECORD_CHUNK: usize = 128;
pub(crate) const BRUSH_CHUNK: usize = 256;
pub(crate) const STOP_CHUNK: usize = 256;
pub(crate) const PLACEMENT_CHUNK: usize = 4;

/// Runs with at least this many records keep retained GPU buffers keyed by
/// their command; smaller runs are copied into the frame arena, where
/// consecutive runs share a draw.
pub(crate) const STORE_RUN_MIN_RECORDS: u32 = 64;
const STORE_IDLE_FRAMES: u64 = 120;
const INITIAL_ARENA_RECORDS: usize = 1024;
const INITIAL_BRUSHES: usize = 64;
const INITIAL_STOPS: usize = 128;
const INITIAL_PLACEMENTS: usize = 64;

const PLACEMENT_CANONICALIZE: u32 = 1;
#[cfg(test)]
#[path = "../tests/unit/uniform_placement_chunks.rs"]
mod uniform_placement_chunks;
const PLACEMENT_CLIPPED: u32 = 2;
const PLACEMENT_FILTERED: u32 = 4;
const PLACEMENT_PAINTED: u32 = 8;
const PLACEMENT_TURNED: u32 = 16;

/// The run-table binding mode the device supports: storage buffers hold a
/// recording whole and draw wide arcs as bands; the uniform fallback (the
/// WebGL floor) draws every run from fixed-size chunks as quads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RunBufferMode {
    pub(crate) storage: bool,
    pub(crate) trig_fill: bool,
}

impl RunBufferMode {
    pub(crate) fn for_device(device: &wgpu::Device, downlevel: wgpu::DownlevelFlags) -> Self {
        Self::select(&device.limits(), downlevel)
    }

    pub(crate) fn select(limits: &wgpu::Limits, _downlevel: wgpu::DownlevelFlags) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        if limits.max_storage_buffers_per_shader_stage >= TABLE_COUNT as u32
            && _downlevel.contains(wgpu::DownlevelFlags::VERTEX_STORAGE)
        {
            return Self {
                storage: true,
                trig_fill: _downlevel.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
                    && crate::arc_trig_fill::fits(limits),
            };
        }
        let _ = limits;
        Self {
            storage: false,
            trig_fill: false,
        }
    }

    pub(crate) fn binding_type(self) -> wgpu::BufferBindingType {
        if self.storage {
            wgpu::BufferBindingType::Storage { read_only: true }
        } else {
            wgpu::BufferBindingType::Uniform
        }
    }

    fn usage(self) -> wgpu::BufferUsages {
        if self.storage {
            wgpu::BufferUsages::STORAGE
        } else {
            wgpu::BufferUsages::UNIFORM
        }
    }

    /// The most records one arena chunk holds; unbounded with storage.
    fn arena_records(self) -> usize {
        if self.storage {
            usize::MAX
        } else {
            RECORD_CHUNK
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
const TABLE_COUNT: usize = 3;
const BUFFER_COUNT: usize = 5;
const BODY_BUFFER: usize = 0;
const CURVE_BUFFER: usize = 1;
const BRUSH_BUFFER: usize = 2;
const STOP_BUFFER: usize = 3;
const PLACEMENT_BUFFER: usize = 4;

/// A placement as the vertex stage reads it: the offset with the snap
/// delta folded in, the device clip and its corner radius, the dither
/// origin and the paint.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct PlacementData {
    offset: [f32; 2],
    root_scale: f32,
    flags: u32,
    clip: [f32; 4],
    dither_origin: [f32; 2],
    alpha: f32,
    clip_radius: f32,
    color_matrix: [[f32; 4]; 4],
    color_offset: [f32; 4],
    transform: [f32; 4],
    translation: [f32; 2],
    transform_reserved: [f32; 2],
}

/// `placement`'s clip in device pixels at `root_scale` as x, y, width and
/// height: snapped to the device grid when the placement is.
pub(crate) fn device_clip(placement: &Placement, root_scale: f32) -> Option<[f32; 4]> {
    let clip = placement.clip?;
    let device = if placement.snap_anchor.is_some() {
        canonicalized_scaled_rect(clip, root_scale)
    } else {
        cranpose_ui_graphics::Rect {
            x: clip.x * root_scale,
            y: clip.y * root_scale,
            width: clip.width * root_scale,
            height: clip.height * root_scale,
        }
    };
    Some([device.x, device.y, device.width, device.height])
}

impl PlacementData {
    /// The placement data of `placement` at `root_scale`, drawn in place
    /// under `turn`: the identity except for a layer that turns.
    pub(crate) fn of(placement: &Placement, root_scale: f32, turn: SegmentTransform) -> Self {
        let snap_delta = placement
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
            .unwrap_or_default();
        let canonicalize = placement.snap_anchor.is_some();
        let mut flags = 0;
        if canonicalize {
            flags |= PLACEMENT_CANONICALIZE;
        }
        if placement.paints() {
            flags |= PLACEMENT_PAINTED;
        }
        let clip = match device_clip(placement, root_scale) {
            Some(clip) => {
                flags |= PLACEMENT_CLIPPED;
                clip
            }
            None => [0.0; 4],
        };
        let dither_origin = placement
            .snap_anchor
            .map(|anchor| snapped_anchor_device_origin(anchor, root_scale))
            .unwrap_or_default();
        if !turn.is_identity() {
            flags |= PLACEMENT_TURNED;
        }
        let (transform, translation, _) = turn.uniform_parts();
        let (color_matrix, color_offset) = match placement.color_filter {
            Some(filter) => {
                flags |= PLACEMENT_FILTERED;
                let m = filter.as_matrix();
                let column = |j: usize| [m[j], m[5 + j], m[10 + j], m[15 + j]];
                (
                    [column(0), column(1), column(2), column(3)],
                    [m[4], m[9], m[14], m[19]],
                )
            }
            None => (
                [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                [0.0; 4],
            ),
        };
        Self {
            offset: [
                placement.offset.x + snap_delta.x,
                placement.offset.y + snap_delta.y,
            ],
            root_scale,
            flags,
            clip,
            dither_origin: [dither_origin.x, dither_origin.y],
            alpha: placement.alpha,
            clip_radius: if placement.clip_rounded() {
                placement.clip_radius * root_scale
            } else {
                0.0
            },
            color_matrix,
            color_offset,
            transform,
            translation,
            transform_reserved: [0.0; 2],
        }
    }
}

/// The paint a run's gradient stops were uploaded with: the stops carry
/// the placement's alpha and filter, applied on the CPU once per upload,
/// so a change of paint re-uploads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PaintKey {
    alpha_bits: u32,
    filter: Option<[u32; 20]>,
}

impl PaintKey {
    fn of(placement: &Placement) -> Self {
        Self {
            alpha_bits: placement.alpha.to_bits(),
            filter: placement
                .color_filter
                .map(|filter| filter.as_matrix().map(f32::to_bits)),
        }
    }
}

fn paint_layer(placement: &Placement) -> GraphicsLayer {
    GraphicsLayer {
        alpha: placement.alpha,
        color_filter: placement.color_filter,
        ..GraphicsLayer::default()
    }
}

fn painted_stops(
    stops: &[GradientStopRecord],
    layer: &GraphicsLayer,
    out: &mut Vec<GradientStopRecord>,
) {
    out.clear();
    out.extend(stops.iter().map(|stop| {
        let color = apply_layer_to_color(
            Color(stop.color[0], stop.color[1], stop.color[2], stop.color[3]),
            layer,
        );
        GradientStopRecord {
            color: [color.0, color.1, color.2, color.3],
            position: stop.position,
        }
    }));
}

/// Bytes compared at a time when a stored run's tables change, so the
/// arena whose ball moved re-uploads the ball's chunk, not the arena.
/// Every element size divides it or is divided by it, so a chunk edge is
/// an element edge and a copy-aligned offset.
const UPLOAD_CHUNK_BYTES: usize = 4096;

const ELEMENT_SIZES: [usize; BUFFER_COUNT] = [
    std::mem::size_of::<ShapeRecordBody>(),
    std::mem::size_of::<ShapeRecordCurve>(),
    std::mem::size_of::<BrushRecord>(),
    std::mem::size_of::<GradientStopRecord>(),
    std::mem::size_of::<PlacementData>(),
];
const LABELS: [&str; BUFFER_COUNT] = [
    "Run Record Bodies",
    "Run Record Curves",
    "Run Brushes",
    "Run Gradient Stops",
    "Run Placements",
];

/// The tables a stored run keeps: its record bodies and curves, brushes
/// and stops. The store tier reads its placement from its uniform.
const STORED_TABLES: usize = 4;

/// The most versions a stored run keeps besides the one its draws bind.
const MAX_SPARE_VERSIONS: usize = 3;

/// Brush, stop and placement tables of no entries, which every stored run
/// whose own table is empty binds: most runs paint solid colours, and the
/// store tier never reads its placement. On Metal each buffer, however
/// small, took a page of its own.
pub(crate) struct EmptyRunTables {
    buffers: [Option<wgpu::Buffer>; BUFFER_COUNT],
}

impl EmptyRunTables {
    fn new(device: &wgpu::Device, mode: RunBufferMode, upload: UploadMode) -> Self {
        let buffers = std::array::from_fn(|index| {
            matches!(index, BRUSH_BUFFER | STOP_BUFFER | PLACEMENT_BUFFER).then(|| {
                create_filled_buffer(
                    device,
                    upload,
                    LABELS[index],
                    buffer_usage(mode, index),
                    &vec![0; ELEMENT_SIZES[index]],
                )
            })
        });
        Self { buffers }
    }

    fn buffer(&self, index: usize) -> &wgpu::Buffer {
        self.buffers[index]
            .as_ref()
            .expect("the brush, stop and placement tables have empty stand-ins")
    }
}

/// What making and writing a stored run's tables takes from the store.
struct StoreContext<'a> {
    device: &'a wgpu::Device,
    layout: &'a wgpu::BindGroupLayout,
    upload: UploadMode,
    empty: &'a EmptyRunTables,
    alignment: u64,
}

/// One copy of a stored run's tables, each at an aligned offset of one
/// buffer, and the bind group over its brushes and stops.
struct TableVersion {
    buffer: wgpu::Buffer,
    offsets: [u64; STORED_TABLES],
    capacities: [usize; STORED_TABLES],
    bind_group: wgpu::BindGroup,
    trig_fill: Option<TrigBindings>,
    /// The run's update whose bytes it holds.
    held: u64,
}

impl MappedBuffers for TableVersion {
    fn mapped_buffers(&self) -> &[wgpu::Buffer] {
        std::slice::from_ref(&self.buffer)
    }
}

impl TableVersion {
    /// Tables for `needed` elements, mapped from the start in the mapped
    /// mode; an empty brush or stop table binds the empty stand-in.
    fn new(context: &StoreContext<'_>, needed: [usize; STORED_TABLES]) -> Self {
        let capacities = std::array::from_fn(|index| stored_capacity(needed[index]));
        let mut offsets = [0; STORED_TABLES];
        let mut end = 0;
        for index in 0..STORED_TABLES {
            offsets[index] = align_u64_to(end, context.alignment);
            end = offsets[index] + (ELEMENT_SIZES[index] * capacities[index]) as u64;
        }
        let buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Stored Run Tables"),
            size: align_u64_to(end.max(1), wgpu::COPY_BUFFER_ALIGNMENT),
            usage: context
                .upload
                .writable(wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE | TEST_READBACK),
            mapped_at_creation: context.upload == UploadMode::Mapped,
        });
        let region = |index: usize| {
            if capacities[index] == 0 {
                context.empty.buffer(index).as_entire_binding()
            } else {
                wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: offsets[index],
                    size: wgpu::BufferSize::new((ELEMENT_SIZES[index] * capacities[index]) as u64),
                })
            }
        };
        let bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Run Tables Bind Group"),
                layout: context.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: region(BRUSH_BUFFER),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: region(STOP_BUFFER),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: context.empty.buffer(PLACEMENT_BUFFER).as_entire_binding(),
                    },
                ],
            });
        Self {
            buffer,
            offsets,
            capacities,
            bind_group,
            trig_fill: None,
            held: 0,
        }
    }

    fn holds(&self, needed: [usize; STORED_TABLES]) -> bool {
        self.capacities
            .iter()
            .zip(needed)
            .all(|(capacity, needed)| *capacity >= needed)
    }

    fn bytes(&self) -> usize {
        self.buffer.size() as usize
    }

    fn bound(&self) -> StoredTables {
        StoredTables {
            buffer: self.buffer.clone(),
            records: [self.offsets[BODY_BUFFER], self.offsets[CURVE_BUFFER]].map(|offset| {
                u32::try_from(offset).expect("a stored run's tables fit u32 offsets")
            }),
            bind_group: self.bind_group.clone(),
        }
    }

    /// Fills the trig rows of the version's arc records, once its curves
    /// were written.
    fn fill_trig(
        &mut self,
        recorder: &mut impl FrameCommandRecorder,
        fill: &ArcTrigFill,
        rows: usize,
    ) {
        let (buffer, offsets, capacity) =
            (&self.buffer, self.offsets, self.capacities[BODY_BUFFER]);
        let bindings = self.trig_fill.get_or_insert_with(|| {
            fill.bind(
                (buffer, offsets[BODY_BUFFER]),
                (buffer, offsets[CURVE_BUFFER]),
                capacity as u64,
            )
        });
        let mut pass = recorder.begin_compute_pass("Stored Run Arc Trig Fill");
        fill.dispatch(&mut pass, bindings, rows as u32);
    }
}

/// The tables one stored-run batch binds: the version that was current
/// when the batch was prepared, so a later rewrite in the same frame goes
/// to another version and leaves this one as the batch's draws read it.
pub(crate) struct StoredTables {
    buffer: wgpu::Buffer,
    records: [u32; 2],
    bind_group: wgpu::BindGroup,
}

impl StoredTables {
    pub(crate) fn binding(&self) -> ArenaBinding<'_> {
        ArenaBinding {
            records: [&self.buffer, &self.buffer],
            bind_group: &self.bind_group,
            offsets: [self.records[0], self.records[1], 0, 0, 0],
        }
    }
}

/// A run's tables as the update writes them: the recorded ones it compares
/// against `previous`, and the stops painted with the run's paint.
struct TableUpdate<'a> {
    previous: &'a RecordTables,
    tables: &'a RecordTables,
    painted_stops: &'a [GradientStopRecord],
    stops_changed: bool,
}

impl TableUpdate<'_> {
    fn bytes(&self, table: usize) -> &[u8] {
        match table {
            BODY_BUFFER => bytemuck::cast_slice(self.tables.shapes.bodies()),
            CURVE_BUFFER => bytemuck::cast_slice(self.tables.shapes.curves()),
            BRUSH_BUFFER => bytemuck::cast_slice(&self.tables.brushes),
            _ => bytemuck::cast_slice(self.painted_stops),
        }
    }

    /// Calls `changed` with each byte range of `table` that differs from
    /// the previous tables.
    fn changes(&self, table: usize, changed: impl FnMut(std::ops::Range<usize>)) {
        let previous: &[u8] = match table {
            BODY_BUFFER => bytemuck::cast_slice(self.previous.shapes.bodies()),
            CURVE_BUFFER => bytemuck::cast_slice(self.previous.shapes.curves()),
            BRUSH_BUFFER => bytemuck::cast_slice(&self.previous.brushes),
            _ if self.stops_changed => &[],
            _ => self.bytes(table),
        };
        changed_ranges(previous, self.bytes(table), changed);
    }
}

/// Calls `changed` with the chunks of [`UPLOAD_CHUNK_BYTES`] of `data` that
/// differ from `previous`, joined when adjacent, and with everything past
/// the shorter of the two.
fn changed_ranges(previous: &[u8], data: &[u8], mut changed: impl FnMut(std::ops::Range<usize>)) {
    let shared = previous.len().min(data.len());
    let mut pending = None;
    let mut offset = 0;
    while offset < shared {
        let end = (offset + UPLOAD_CHUNK_BYTES).min(shared);
        let differs = data[offset..end] != previous[offset..end];
        match (differs, pending) {
            (true, None) => pending = Some(offset),
            (false, Some(from)) => {
                changed(from..offset);
                pending = None;
            }
            _ => {}
        }
        offset = end;
    }
    let from = pending.unwrap_or(shared);
    if from < data.len() {
        changed(from..data.len());
    }
}

/// Which bytes of each table a version takes.
#[derive(Clone, Copy)]
enum Writes<'a> {
    /// All of them: the version holds nothing yet.
    Whole,
    /// Those that differ from the previous tables, which it holds.
    Changed,
    /// The chunks a later update than the one it holds changed.
    Since(&'a [Vec<u64>; STORED_TABLES], u64),
}

/// Writes `update` into `version` as `writes` says: through ordered copies
/// in the copied mode, into the mapped buffer in the mapped mode, which
/// is then unmapped for the frame's draws. Fills the trig rows of arc
/// records whose curves were written.
fn write_version(
    context: &StoreContext<'_>,
    recorder: &mut impl FrameCommandRecorder,
    version: &mut TableVersion,
    update: &TableUpdate<'_>,
    writes: Writes<'_>,
    fill: Option<&ArcTrigFill>,
) -> FrameCommandStats {
    let mut stats = FrameCommandStats::default();
    let mut curves_written = false;
    for table in 0..STORED_TABLES {
        let bytes = update.bytes(table);
        let buffer = &version.buffer;
        let base = version.offsets[table];
        let mut write = |range: std::ops::Range<usize>| {
            curves_written |= table == CURVE_BUFFER;
            let offset = base + range.start as u64;
            stats += match context.upload {
                UploadMode::Copied => {
                    recorder.stage_buffer_copy(context.device, buffer, offset, &bytes[range])
                }
                UploadMode::Mapped => write_mapped(buffer, offset, &bytes[range]),
            };
        };
        match writes {
            Writes::Whole if !bytes.is_empty() => write(0..bytes.len()),
            Writes::Whole => {}
            Writes::Changed => update.changes(table, write),
            Writes::Since(stamps, held) => stale_ranges(&stamps[table], held, bytes.len(), write),
        }
    }
    if context.upload == UploadMode::Mapped {
        version.buffer.unmap();
    }
    if let Some(fill) = fill
        && curves_written
        && tables_hold_arcs(update.tables)
    {
        version.fill_trig(recorder, fill, update.tables.shapes.len());
    }
    stats
}

/// Calls `stale` with the byte ranges of a table of `len` bytes whose
/// chunks changed in an update after `held`, joined when adjacent.
fn stale_ranges(
    stamps: &[u64],
    held: u64,
    len: usize,
    mut stale: impl FnMut(std::ops::Range<usize>),
) {
    let mut chunk = 0;
    while chunk < stamps.len() {
        if stamps[chunk] <= held {
            chunk += 1;
            continue;
        }
        let from = chunk;
        while chunk < stamps.len() && stamps[chunk] > held {
            chunk += 1;
        }
        let range = from * UPLOAD_CHUNK_BYTES..(chunk * UPLOAD_CHUNK_BYTES).min(len);
        if !range.is_empty() {
            stale(range);
        }
    }
}

/// The elements each of a stored run's tables holds for `tables`: none
/// for an empty brush or stop table.
fn stored_needs(tables: &RecordTables) -> [usize; STORED_TABLES] {
    [
        tables.shapes.len().max(1),
        tables.shapes.len().max(1),
        tables.brushes.len(),
        tables.stops.len(),
    ]
}

/// The elements a stored run's table takes for `needed`: a quarter more,
/// so a run that grows a little keeps its buffer, in whole groups of 16.
/// A run of 70 records took 256 at the start and the next power of two
/// after: 341 runs held 20 MB of tables on the desktop gauntlet.
fn stored_capacity(needed: usize) -> usize {
    if needed == 0 {
        return 0;
    }
    (needed + needed / 4).next_multiple_of(16)
}

/// The usage of an arena table, short of how the CPU writes it.
fn buffer_usage(mode: RunBufferMode, index: usize) -> wgpu::BufferUsages {
    if index == BODY_BUFFER || index == CURVE_BUFFER {
        if mode.trig_fill {
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE
        } else {
            wgpu::BufferUsages::VERTEX
        }
    } else {
        mode.usage()
    }
}

fn tables_hold_arcs(tables: &RecordTables) -> bool {
    tables.segments.iter().any(segment_holds_arcs)
}

fn active_fill(mode: RunBufferMode, fill: Option<&ArcTrigFill>) -> Option<&ArcTrigFill> {
    mode.trig_fill
        .then(|| fill.expect("a store whose mode fills arc trig rows was handed its fill"))
}

fn segment_holds_arcs(segment: &RecordSegment) -> bool {
    segment.lane == RecordLane::Shapes && segment.kinds & (1 << FRAGMENT_KIND_ARC) != 0
}

/// A recording's tables resident on the GPU, keyed by its command.
///
/// In the copied mode the run keeps one version and copies what changes
/// into it, ordered between the passes that read it. In the mapped mode the
/// CPU writes a version the GPU no longer reads: a change goes to a spare
/// whose map completed once the frames that drew it finished, or to a new
/// one, which takes the chunks changed since the update it holds; the
/// version it replaces waits for its own map. A frame never writes a
/// version that frame or one in flight reads.
pub(crate) struct StoredRun {
    tables: TableVersion,
    spares: MappedPool<TableVersion>,
    /// The mapped mode's update that last changed each chunk of each table.
    stamps: [Vec<u64>; STORED_TABLES],
    update: u64,
    last_changed_frame: u64,
    recorder: Arc<cranpose_ui_graphics::ShapeRecorder>,
    paint: PaintKey,
    fill: Option<ShapeFill>,
    fill_scale_bits: u32,
    fill_offset_bits: [u32; 2],
    fill_window: [u32; 4],
    last_used_frame: u64,
}

impl StoredRun {
    fn bytes(&self) -> usize {
        self.tables.bytes() + self.spares.iter().map(TableVersion::bytes).sum::<usize>()
    }

    /// Notes in the stamps the chunks `update` changes and says whether
    /// it changes any.
    fn stamp_changes(&mut self, update: &TableUpdate<'_>) -> bool {
        let stamp = self.update + 1;
        let mut changed = false;
        for (table, stamps) in self.stamps.iter_mut().enumerate() {
            let chunks = update.bytes(table).len().div_ceil(UPLOAD_CHUNK_BYTES);
            changed |= chunks > stamps.len();
            stamps.resize(chunks, stamp);
            update.changes(table, |range| {
                changed = true;
                let chunks =
                    range.start / UPLOAD_CHUNK_BYTES..range.end.div_ceil(UPLOAD_CHUNK_BYTES);
                for chunk in &mut stamps[chunks] {
                    *chunk = stamp;
                }
            });
        }
        changed
    }

    /// Brings the run's tables to `update`, in place in the copied mode and
    /// in another version in the mapped mode.
    fn write(
        &mut self,
        context: &StoreContext<'_>,
        recorder: &mut impl FrameCommandRecorder,
        update: &TableUpdate<'_>,
        fill: Option<&ArcTrigFill>,
        requests: &mut MapRequests,
    ) -> FrameCommandStats {
        let needed = stored_needs(update.tables);
        if context.upload == UploadMode::Copied {
            let writes = if self.tables.holds(needed) {
                Writes::Changed
            } else {
                self.tables = TableVersion::new(context, needed);
                Writes::Whole
            };
            return write_version(context, recorder, &mut self.tables, update, writes, fill);
        }
        if !self.stamp_changes(update) {
            return FrameCommandStats::default();
        }
        self.update += 1;
        let mut version = match self.spares.take(context.device, Vec::pop) {
            Some(spare) if spare.holds(needed) => spare,
            _ => {
                if self.spares.len() >= MAX_SPARE_VERSIONS {
                    self.spares.drop_oldest_mapping();
                }
                TableVersion::new(context, needed)
            }
        };
        let writes = match version.held {
            0 => Writes::Whole,
            held => Writes::Since(&self.stamps, held),
        };
        let stats = write_version(context, recorder, &mut version, update, writes, fill);
        version.held = self.update;
        let replaced = std::mem::replace(&mut self.tables, version);
        self.spares.recycle(replaced, requests);
        stats
    }
}

const JOINED_PLAIN_RECORDS: u32 = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunDrawCall {
    pub(crate) key: crate::render::ShapePipelineKey,
    pub(crate) band_class: u8,
    pub(crate) records: std::ops::Range<u32>,
    plain: u32,
    /// Whether one of the draw's records is an opaque fill with an
    /// interior worth laying down ahead of the paint.
    pub(crate) occluders: bool,
}

impl RunDrawCall {
    fn new(
        key: crate::render::ShapePipelineKey,
        band_class: u8,
        records: std::ops::Range<u32>,
        occluders: bool,
    ) -> Self {
        Self {
            key,
            band_class,
            records,
            plain: 0,
            occluders,
        }
    }

    fn absorb(
        &mut self,
        key: crate::render::ShapePipelineKey,
        band_class: u8,
        records: std::ops::Range<u32>,
        occluders: bool,
    ) -> bool {
        if self.band_class != band_class || self.records.end != records.start {
            return false;
        }
        let joined = self.key.joined();
        let plain = if key == self.key {
            self.plain
        } else if key.joined() != joined {
            return false;
        } else if self.key == joined {
            self.plain + (records.end - records.start)
        } else {
            self.records.end - self.records.start
        };
        if plain > JOINED_PLAIN_RECORDS {
            return false;
        }
        if key != self.key {
            self.key = joined;
        }
        self.plain = plain;
        self.records.end = records.end;
        self.occluders |= occluders;
        true
    }

    /// The indices each record of the draw is instanced over.
    pub(crate) fn indices(&self) -> std::ops::Range<u32> {
        0..strip_indices(band_class_segments(self.band_class))
    }
}

/// The index buffer every draw at one band class instances over: the
/// class's strip pattern, once.
#[derive(Default)]
struct StripIndexBuffer {
    buffer: Option<wgpu::Buffer>,
}

impl StripIndexBuffer {
    fn ensure(&mut self, device: &wgpu::Device, upload: UploadMode, segments: u32) {
        if self.buffer.is_some() {
            return;
        }
        let indices: Vec<u32> = strip_index_pattern(segments).collect();
        self.buffer = Some(create_filled_buffer(
            device,
            upload,
            "Run Strip Indices",
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&indices),
        ));
    }
}

/// The CPU side of one arena chunk while a pass fills it.
#[derive(Default)]
pub(crate) struct ArenaStaging {
    bodies: Vec<ShapeRecordBody>,
    curves: Vec<ShapeRecordCurve>,
    brushes: Vec<BrushRecord>,
    stops: Vec<GradientStopRecord>,
    placements: Vec<PlacementData>,
    brush_map: Vec<u32>,
    painted: Vec<GradientStopRecord>,
    draws: Vec<RunDrawCall>,
    pub(crate) fill: ShapeFill,
    arcs: bool,
}

impl ArenaStaging {
    fn clear(&mut self) {
        self.arcs = false;
        self.bodies.clear();
        self.curves.clear();
        self.brushes.clear();
        self.stops.clear();
        self.placements.clear();
        self.draws.clear();
        self.fill = ShapeFill::default();
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.bodies.capacity() * std::mem::size_of::<ShapeRecordBody>()
            + self.curves.capacity() * std::mem::size_of::<ShapeRecordCurve>()
            + self.brushes.capacity() * std::mem::size_of::<BrushRecord>()
            + self.stops.capacity() * std::mem::size_of::<GradientStopRecord>()
            + self.placements.capacity() * std::mem::size_of::<PlacementData>()
    }

    fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    fn fits(&self, mode: RunBufferMode, records: usize, brushes: usize, stops: usize) -> bool {
        if mode.storage {
            return true;
        }
        self.bodies.len() + records <= RECORD_CHUNK
            && self.brushes.len() + brushes <= BRUSH_CHUNK
            && self.stops.len() + stops <= STOP_CHUNK
            && self.placements.len() < PLACEMENT_CHUNK
    }

    /// Records `record` under `key`, extending the last draw when it
    /// continues it.
    fn push_draw(
        &mut self,
        key: crate::render::ShapePipelineKey,
        band_class: u8,
        record: u32,
        occluders: bool,
    ) {
        let records = record..record + 1;
        if self
            .draws
            .last_mut()
            .is_some_and(|last| last.absorb(key, band_class, records.clone(), occluders))
        {
            return;
        }
        self.draws
            .push(RunDrawCall::new(key, band_class, records, occluders));
    }
}

/// Where one closed chunk's tables sit: the frame's generation of arena
/// buffers and the chunk's dynamic offset into each table.
#[derive(Clone, Copy, Default)]
struct ArenaChunk {
    generation: usize,
    offsets: [u32; BUFFER_COUNT],
}

/// The tables a chunk's draws bind.
pub(crate) struct ArenaBinding<'a> {
    pub(crate) records: [&'a wgpu::Buffer; 2],
    pub(crate) bind_group: &'a wgpu::BindGroup,
    pub(crate) offsets: [u32; BUFFER_COUNT],
}

struct ArenaGeneration {
    buffers: [wgpu::Buffer; BUFFER_COUNT],
    capacities: [u64; BUFFER_COUNT],
    /// The end of each table's placements this frame.
    cursors: [u64; BUFFER_COUNT],
    /// A copied generation's tables this frame, written when it is staged.
    staged: [Vec<u8>; BUFFER_COUNT],
    /// The binding sizes its bind group covers.
    bindings: [u64; BUFFER_COUNT],
    bind_group: wgpu::BindGroup,
    arcs: bool,
    trig_fill: Option<TrigBindings>,
}

impl MappedBuffers for ArenaGeneration {
    fn mapped_buffers(&self) -> &[wgpu::Buffer] {
        &self.buffers
    }
}

impl ArenaGeneration {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        (mode, upload): (RunBufferMode, UploadMode),
        capacities: [u64; BUFFER_COUNT],
        bindings: [u64; BUFFER_COUNT],
    ) -> Self {
        let buffers = std::array::from_fn(|index| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(LABELS[index]),
                size: capacities[index],
                usage: upload.writable(buffer_usage(mode, index)),
                mapped_at_creation: upload == UploadMode::Mapped,
            })
        });
        let bind_group = Self::bind(device, layout, &buffers, bindings);
        Self {
            buffers,
            capacities,
            cursors: [0; BUFFER_COUNT],
            staged: Default::default(),
            bindings,
            bind_group,
            arcs: false,
            trig_fill: None,
        }
    }

    fn bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffers: &[wgpu::Buffer; BUFFER_COUNT],
        bindings: [u64; BUFFER_COUNT],
    ) -> wgpu::BindGroup {
        let entries =
            [(1, BRUSH_BUFFER), (2, STOP_BUFFER), (3, PLACEMENT_BUFFER)].map(|(binding, index)| {
                wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffers[index],
                        offset: 0,
                        size: wgpu::BufferSize::new(bindings[index]),
                    }),
                }
            });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Run Arena Bind Group"),
            layout,
            entries: &entries,
        })
    }

    fn holds(&self, bytes: [u64; BUFFER_COUNT]) -> bool {
        self.capacities
            .iter()
            .zip(bytes)
            .all(|(capacity, bytes)| *capacity >= bytes)
    }

    fn placed(&self) -> bool {
        self.cursors.iter().any(|cursor| *cursor > 0)
    }

    fn rebind(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        bindings: [u64; BUFFER_COUNT],
    ) {
        self.bind_group = Self::bind(device, layout, &self.buffers, bindings);
        self.bindings = bindings;
    }

    /// Places `bytes` at `offset` of `table`: staged for the frame's copy,
    /// or written where the buffer is mapped.
    fn write(
        &mut self,
        table: usize,
        offset: u64,
        bytes: &[u8],
        upload: UploadMode,
    ) -> FrameCommandStats {
        self.cursors[table] = offset + bytes.len() as u64;
        match upload {
            UploadMode::Copied => {
                let staged = &mut self.staged[table];
                staged.resize(offset as usize, 0);
                staged.extend_from_slice(bytes);
                FrameCommandStats::default()
            }
            UploadMode::Mapped => write_mapped(&self.buffers[table], offset, bytes),
        }
    }

    /// Opens the generation for a frame.
    fn clear(&mut self) {
        self.arcs = false;
        self.cursors = [0; BUFFER_COUNT];
        for staged in &mut self.staged {
            staged.clear();
        }
    }
}

/// The frame's arena tables. A copied arena keeps its last generation from
/// frame to frame, since the copies into it queue behind the draws that
/// read it. A mapped arena writes each placement into a generation it
/// keeps mapped until the frame is staged; its generations then cycle
/// through a [`MappedPool`] while the GPU reads them.
struct ArenaTables {
    mode: RunBufferMode,
    upload: UploadMode,
    alignment: u64,
    bindings: [u64; BUFFER_COUNT],
    generations: Vec<ArenaGeneration>,
    chunks: Vec<ArenaChunk>,
    staging: ArenaStaging,
    pool: MappedPool<ArenaGeneration>,
    requests: MapRequests,
    /// The bytes of each table the last mapped frame placed: a frame opens
    /// in a generation that holds them.
    last_totals: [u64; BUFFER_COUNT],
    /// Whether the frame's mapped generations were unmapped for its submit.
    sealed: bool,
    /// The frame's mapped writes so far, reported when it is staged.
    written: FrameCommandStats,
}

/// The capacities of the generation a placement that does not fit `current`
/// opens: what each table's placement asked for, at least the tables'
/// initial sizes.
fn grown_capacities(
    placements: &[UploadPlacement; BUFFER_COUNT],
    current: Option<&ArenaGeneration>,
) -> [u64; BUFFER_COUNT] {
    std::array::from_fn(|index| {
        let least = (INITIAL_ARENA_CAPACITIES[index] * ELEMENT_SIZES[index]) as u64;
        match placements[index] {
            UploadPlacement::Grow(capacity) => capacity.max(least),
            UploadPlacement::At(_) => current
                .map_or(least, |generation| generation.capacities[index])
                .max(least),
        }
    })
}

const INITIAL_ARENA_CAPACITIES: [usize; BUFFER_COUNT] = [
    INITIAL_ARENA_RECORDS,
    INITIAL_ARENA_RECORDS,
    INITIAL_BRUSHES,
    INITIAL_STOPS,
    INITIAL_PLACEMENTS,
];

const UNIFORM_CHUNKS: [usize; BUFFER_COUNT] = [
    RECORD_CHUNK,
    RECORD_CHUNK,
    BRUSH_CHUNK,
    STOP_CHUNK,
    PLACEMENT_CHUNK,
];

impl ArenaTables {
    fn new(mode: RunBufferMode, upload: UploadMode, alignment: u64) -> Self {
        let bindings = std::array::from_fn(|index| {
            let elements = if mode.storage {
                1
            } else {
                UNIFORM_CHUNKS[index]
            };
            (elements * ELEMENT_SIZES[index]) as u64
        });
        Self {
            mode,
            upload,
            alignment,
            bindings,
            generations: Vec::new(),
            chunks: Vec::new(),
            staging: ArenaStaging::default(),
            pool: MappedPool::default(),
            requests: MapRequests::default(),
            last_totals: [0; BUFFER_COUNT],
            sealed: false,
            written: FrameCommandStats::default(),
        }
    }

    fn place(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        tables: [&[u8]; BUFFER_COUNT],
    ) -> ArenaChunk {
        if self.mode.storage {
            for (binding, table) in self.bindings.iter_mut().zip(tables) {
                *binding = (*binding).max(table.len() as u64);
            }
        }
        let current = self.generations.last();
        let placements: [UploadPlacement; BUFFER_COUNT] = std::array::from_fn(|index| {
            place_upload(
                current.map_or(0, |generation| generation.cursors[index]),
                tables[index].len() as u64,
                self.bindings[index],
                self.table_alignment(index),
                current.map(|generation| generation.capacities[index]),
            )
        });
        // The chunks a generation holds bind its bind group: one bound with
        // smaller tables takes no more chunks.
        let stale = current.is_some_and(|generation| generation.bindings != self.bindings);
        let grows = placements
            .iter()
            .any(|placement| matches!(placement, UploadPlacement::Grow(_)))
            || (stale && current.is_some_and(ArenaGeneration::placed));
        if grows {
            let capacities = grown_capacities(&placements, current);
            let generation = self.open(device, layout, capacities);
            self.generations.push(generation);
        } else if stale && let Some(generation) = self.generations.last_mut() {
            generation.rebind(device, layout, self.bindings);
        }
        let index = self.generations.len() - 1;
        let generation = &mut self.generations[index];
        let upload = self.upload;
        let mut written = FrameCommandStats::default();
        let offsets = std::array::from_fn(|table| {
            let offset = match placements[table] {
                UploadPlacement::At(offset) if !grows => offset,
                _ => 0,
            };
            written += generation.write(table, offset, tables[table], upload);
            u32::try_from(offset).expect("a frame's arena tables fit a dynamic offset")
        });
        self.written += written;
        ArenaChunk {
            generation: index,
            offsets,
        }
    }

    /// A generation of at least `capacities` bytes per table: a mapped
    /// arena reuses a free one, which at a frame's start holds the last
    /// frame's tables with room to grow, and makes one when none is free.
    /// Free generations that do not hold the frame's tables go, so the
    /// pool keeps only generations a frame can take.
    fn open(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        capacities: [u64; BUFFER_COUNT],
    ) -> ArenaGeneration {
        let modes = (self.mode, self.upload);
        if self.upload == UploadMode::Copied {
            return ArenaGeneration::new(device, layout, modes, capacities, self.bindings);
        }
        let wanted = if self.generations.is_empty() {
            std::array::from_fn(|index| {
                let last = self.last_totals[index];
                capacities[index].max(align_u64_to(last + last / 4, self.alignment))
            })
        } else {
            capacities
        };
        let taken = self.pool.take(device, |free| {
            free.retain(|generation| generation.holds(wanted));
            free.pop()
        });
        match taken {
            Some(mut generation) => {
                generation.clear();
                if generation.bindings != self.bindings {
                    generation.rebind(device, layout, self.bindings);
                }
                generation
            }
            None => ArenaGeneration::new(device, layout, modes, wanted, self.bindings),
        }
    }

    fn table_alignment(&self, table: usize) -> u64 {
        if self.mode.trig_fill && table == BODY_BUFFER {
            self.alignment * (ELEMENT_SIZES[BODY_BUFFER] / ELEMENT_SIZES[CURVE_BUFFER]) as u64
        } else {
            self.alignment
        }
    }

    fn fill_trig(
        &mut self,
        device: &wgpu::Device,
        recorder: &mut impl FrameCommandRecorder,
        fill: &ArcTrigFill,
    ) {
        let mut generations = self
            .generations
            .iter_mut()
            .filter(|generation| generation.arcs)
            .peekable();
        if generations.peek().is_none() {
            return;
        }
        let mut pass = recorder.begin_frame_compute_pass(device, "Arena Arc Trig Fill");
        for generation in generations {
            let rows = generation.cursors[CURVE_BUFFER] / ELEMENT_SIZES[CURVE_BUFFER] as u64;
            let buffers = &generation.buffers;
            let capacity = [BODY_BUFFER, CURVE_BUFFER]
                .map(|table| generation.capacities[table] / ELEMENT_SIZES[table] as u64);
            let bind_group = generation.trig_fill.get_or_insert_with(|| {
                fill.bind(
                    (&buffers[BODY_BUFFER], 0),
                    (&buffers[CURVE_BUFFER], 0),
                    capacity[0].min(capacity[1]),
                )
            });
            fill.dispatch(&mut pass, bind_group, rows as u32);
        }
    }

    /// Writes the frame's tables: a copied arena's staged bytes through
    /// the recorder, one copy per table; a mapped arena wrote them as they
    /// were placed and unmaps its generations for the submit.
    fn stage_pending(
        &mut self,
        device: &wgpu::Device,
        recorder: &mut impl FrameCommandRecorder,
    ) -> FrameCommandStats {
        if self.upload == UploadMode::Mapped {
            for generation in &self.generations {
                for buffer in &generation.buffers {
                    buffer.unmap();
                }
            }
            self.sealed = true;
            if !self.generations.is_empty() {
                self.last_totals = std::array::from_fn(|table| {
                    self.generations
                        .iter()
                        .map(|generation| generation.cursors[table])
                        .sum()
                });
            }
            return std::mem::take(&mut self.written);
        }
        let mut stats = FrameCommandStats::default();
        for generation in &mut self.generations {
            for (buffer, staged) in generation.buffers.iter().zip(&mut generation.staged) {
                if staged.is_empty() {
                    continue;
                }
                let padded = staged.len().div_ceil(wgpu::COPY_BUFFER_ALIGNMENT as usize)
                    * wgpu::COPY_BUFFER_ALIGNMENT as usize;
                staged.resize(padded, 0);
                stats += recorder.stage_frame_buffer_copy(device, buffer, 0, staged);
            }
        }
        stats
    }

    /// Ends the frame: a mapped arena's staged generations go to the pool,
    /// to map again once the frame is submitted; otherwise the last
    /// generation stays for the next frame. A mapped generation the frame
    /// never staged is still mapped and stays writable.
    fn reset(&mut self) {
        self.written = FrameCommandStats::default();
        if std::mem::take(&mut self.sealed) {
            for generation in self.generations.drain(..) {
                self.pool.recycle(generation, &mut self.requests);
            }
            return;
        }
        let keep = self.generations.len().saturating_sub(1);
        self.generations.drain(..keep);
        for generation in &mut self.generations {
            generation.clear();
        }
    }
}

pub(crate) fn draw_vertices(records: u32, band_class: u8) -> u64 {
    u64::from(records) * u64::from(strip_vertices(band_class_segments(band_class)))
}

/// The GPU home of every run: retained tables per command, and the
/// per-pass arena chunks small runs are copied into.
pub(crate) struct RunStore {
    mode: RunBufferMode,
    upload: UploadMode,
    alignment: u64,
    layout: wgpu::BindGroupLayout,
    stored: HashMap<DrawCommandId, StoredRun>,
    empty_tables: EmptyRunTables,
    arena: ArenaTables,
    scratch_stops: Vec<GradientStopRecord>,
    strip_indices: [StripIndexBuffer; ARC_BUCKETS],
    frame: u64,
    fill_stats: bool,
    trig_fill: Option<ArcTrigFill>,
    /// The stored runs' replaced versions, mapped again once the frame
    /// that replaced them is submitted.
    requests: MapRequests,
}

impl RunStore {
    pub(crate) fn new(device: &wgpu::Device, mode: RunBufferMode, upload: UploadMode) -> Self {
        let binding = |index: u32| wgpu::BindGroupLayoutEntry {
            binding: index,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: mode.binding_type(),
                has_dynamic_offset: true,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Run Tables Bind Group Layout"),
            entries: &[binding(1), binding(2), binding(3)],
        });
        let limits = device.limits();
        let alignment = u64::from(if mode.storage {
            limits.min_storage_buffer_offset_alignment
        } else {
            limits.min_uniform_buffer_offset_alignment
        })
        .max(wgpu::COPY_BUFFER_ALIGNMENT);
        let alignment = match upload {
            UploadMode::Copied => alignment,
            UploadMode::Mapped => alignment.max(wgpu::MAP_ALIGNMENT),
        };
        Self {
            mode,
            upload,
            alignment,
            layout,
            stored: HashMap::default(),
            empty_tables: EmptyRunTables::new(device, mode, upload),
            arena: ArenaTables::new(mode, upload, alignment),
            fill_stats: false,
            scratch_stops: Vec::new(),
            strip_indices: Default::default(),
            frame: 0,
            trig_fill: None,
            requests: MapRequests::default(),
        }
    }

    pub(crate) fn attach_trig_fill(&mut self, fill: ArcTrigFill) {
        debug_assert!(
            self.mode.trig_fill,
            "only a filling mode runs the arc trig fill"
        );
        self.trig_fill = Some(fill);
    }

    pub(crate) fn fill_arena_trig(
        &mut self,
        device: &wgpu::Device,
        recorder: &mut impl FrameCommandRecorder,
    ) {
        if let Some(fill) = active_fill(self.mode, self.trig_fill.as_ref()) {
            self.arena.fill_trig(device, recorder, fill);
        }
    }

    /// The index buffer draws at band class `class` instance over; every
    /// draw's was created when the draw was recorded.
    pub(crate) fn strip_index_buffer(&self, class: u8) -> &wgpu::Buffer {
        self.strip_indices[class as usize]
            .buffer
            .as_ref()
            .expect("a draw's strip index buffer was created when the draw was recorded")
    }

    fn ensure_strip_indices(&mut self, device: &wgpu::Device, draws: &[RunDrawCall]) {
        for draw in draws {
            let class = draw.band_class;
            self.strip_indices[class as usize].ensure(
                device,
                self.upload,
                band_class_segments(class),
            );
        }
    }

    /// Appends the draws one stored run takes to `out`: its segments in
    /// record order, one draw for each run of them a single pipeline draws.
    pub(crate) fn stored_run_draws(
        &mut self,
        device: &wgpu::Device,
        run: &RunDraw,
        key_for: &mut dyn FnMut(&RecordSegment) -> crate::render::ShapePipelineKey,
        out: &mut Vec<RunDrawCall>,
    ) {
        let start = out.len();
        for segment in run.segment_records() {
            let key = key_for(segment);
            let band_class = if self.mode.storage {
                segment.band_class
            } else {
                0
            };
            let records = segment.start..segment.start + segment.count;
            let occluders = segment.occluders;
            if !out[start..]
                .last_mut()
                .is_some_and(|last| last.absorb(key, band_class, records.clone(), occluders))
            {
                out.push(RunDrawCall::new(key, band_class, records, occluders));
            }
        }
        self.ensure_strip_indices(device, &out[start..]);
    }

    pub(crate) fn mode(&self) -> RunBufferMode {
        self.mode
    }

    pub(crate) fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Starts a frame: the arena chunks are free again and runs unused
    /// for a while leave the store.
    /// Opens a frame; `fill_stats` says whether the frame's fill estimate
    /// is wanted, the only reason to walk every record a second time.
    pub(crate) fn invalidate_uploads(&mut self) {
        self.stored.clear();
        self.arena.reset();
    }

    pub(crate) fn begin_frame(&mut self, fill_stats: bool) {
        self.fill_stats = fill_stats;
        self.frame += 1;
        self.arena.chunks.clear();
        let frame = self.frame;
        self.stored.retain(|_, run| {
            if frame - run.last_changed_frame > IDLE_FRAMES {
                run.spares.clear();
            }
            frame - run.last_used_frame <= STORE_IDLE_FRAMES
        });
    }

    pub(crate) fn finish_frame(&mut self) {
        self.arena.reset();
    }

    /// Asks the frame's replaced tables back once the frame is submitted.
    pub(crate) fn recall(&mut self) {
        self.requests.recall();
        self.arena.requests.recall();
    }

    pub(crate) fn stage_pending(
        &mut self,
        device: &wgpu::Device,
        recorder: &mut impl FrameCommandRecorder,
    ) -> FrameCommandStats {
        self.arena.stage_pending(device, recorder)
    }

    pub(crate) fn stored_count(&self) -> usize {
        self.stored.len()
    }

    pub(crate) fn stored_bytes(&self) -> usize {
        self.stored.values().map(StoredRun::bytes).sum()
    }

    pub(crate) fn arena_staging_bytes(&self) -> usize {
        self.arena.staging.heap_bytes()
            + self
                .arena
                .generations
                .iter()
                .flat_map(|generation| generation.staged.iter())
                .map(Vec::capacity)
                .sum::<usize>()
    }

    /// The tables a closed chunk's draws bind: the frame's arena bind
    /// group and the chunk's dynamic offsets into it.
    pub(crate) fn arena_binding(&self, chunk: usize) -> ArenaBinding<'_> {
        let chunk = self.arena.chunks[chunk];
        let generation = &self.arena.generations[chunk.generation];
        ArenaBinding {
            records: [
                &generation.buffers[BODY_BUFFER],
                &generation.buffers[CURVE_BUFFER],
            ],
            bind_group: &generation.bind_group,
            offsets: chunk.offsets,
        }
    }

    /// Whether `run` keeps retained buffers rather than joining the arena.
    pub(crate) fn is_stored(&self, run: &RunDraw) -> bool {
        self.mode.storage && run.command.is_some() && run.record_count() >= STORE_RUN_MIN_RECORDS
    }

    /// Brings a stored run's tables up to date: nothing is written when the
    /// recorder handed back the same tables, or new tables with the same
    /// bytes, under the same paint. Returns the upload stats, the run's
    /// fill for `root_scale` and the tables its draws bind.
    pub(crate) fn upload_stored(
        &mut self,
        device: &wgpu::Device,
        recorder: &mut impl FrameCommandRecorder,
        run: &RunDraw,
        root_scale: f32,
        window: &std::ops::Range<u32>,
        draws: &[RunDrawCall],
    ) -> (FrameCommandStats, Option<ShapeFill>, StoredTables) {
        let command = run.command.expect("a stored run has a command");
        let paint = PaintKey::of(&run.placement);
        let frame = self.frame;
        let fill_stats = self.fill_stats;
        let trig_fill = active_fill(self.mode, self.trig_fill.as_ref());
        let context = StoreContext {
            device,
            layout: &self.layout,
            upload: self.upload,
            empty: &self.empty_tables,
            alignment: self.alignment,
        };
        let tables = run.tables();
        let scratch_stops = &mut self.scratch_stops;
        let (entry, stats) = match self.stored.entry(command) {
            Entry::Occupied(entry) => {
                let entry = entry.into_mut();
                let mut stats = FrameCommandStats::default();
                if !Arc::ptr_eq(&entry.recorder, &run.recorder) || entry.paint != paint {
                    let previous_recorder =
                        std::mem::replace(&mut entry.recorder, Arc::clone(&run.recorder));
                    let previous = previous_recorder.tables();
                    let stops_changed = entry.paint != paint || previous.stops != tables.stops;
                    painted_stops(&tables.stops, &paint_layer(&run.placement), scratch_stops);
                    let update = TableUpdate {
                        previous,
                        tables,
                        painted_stops: scratch_stops,
                        stops_changed,
                    };
                    stats = entry.write(&context, recorder, &update, trig_fill, &mut self.requests);
                    if fill_stats && previous.segments != tables.segments {
                        entry.fill = None;
                    }
                    if stats.upload_bytes > 0 || stops_changed {
                        entry.fill = None;
                        entry.last_changed_frame = frame;
                    }
                    entry.paint = paint;
                }
                (entry, stats)
            }
            Entry::Vacant(slot) => {
                painted_stops(&tables.stops, &paint_layer(&run.placement), scratch_stops);
                let update = TableUpdate {
                    previous: tables,
                    tables,
                    painted_stops: scratch_stops,
                    stops_changed: true,
                };
                let mut version = TableVersion::new(&context, stored_needs(tables));
                let stats = write_version(
                    &context,
                    recorder,
                    &mut version,
                    &update,
                    Writes::Whole,
                    trig_fill,
                );
                version.held = 1;
                let stamps = std::array::from_fn(|table| match context.upload {
                    UploadMode::Copied => Vec::new(),
                    UploadMode::Mapped => {
                        vec![1; update.bytes(table).len().div_ceil(UPLOAD_CHUNK_BYTES)]
                    }
                });
                let entry = slot.insert(StoredRun {
                    tables: version,
                    spares: MappedPool::default(),
                    stamps,
                    update: 1,
                    last_changed_frame: frame,
                    recorder: Arc::clone(&run.recorder),
                    paint,
                    fill: None,
                    fill_scale_bits: 0,
                    fill_offset_bits: [0; 2],
                    fill_window: [0; 4],
                    last_used_frame: frame,
                });
                (entry, stats)
            }
        };
        entry.last_used_frame = frame;
        let fill = fill_stats.then(|| {
            let offset_bits = [
                run.placement.offset.x.to_bits(),
                run.placement.offset.y.to_bits(),
            ];
            let fill_window = [
                run.segments.start,
                run.segments.end,
                window.start,
                window.end,
            ];
            if entry.fill_scale_bits != root_scale.to_bits()
                || entry.fill_offset_bits != offset_bits
                || entry.fill_window != fill_window
            {
                entry.fill = None;
                entry.fill_scale_bits = root_scale.to_bits();
                entry.fill_offset_bits = offset_bits;
                entry.fill_window = fill_window;
            }
            *entry.fill.get_or_insert_with(|| {
                ShapeFill::of_draws(
                    run.tables(),
                    run.placement.offset,
                    root_scale,
                    draws.iter().map(|draw| {
                        (
                            draw.records.clone(),
                            Some(band_class_segments(draw.band_class)),
                        )
                    }),
                )
            })
        });
        (stats, fill, entry.tables.bound())
    }

    /// Opens the arena chunk a pass appends to.
    pub(crate) fn open_arena(&mut self) -> usize {
        let chunk = self.arena.chunks.len();
        self.arena.chunks.push(ArenaChunk::default());
        self.arena.staging.clear();
        chunk
    }

    /// The records the open chunk holds, the instance index of its next.
    pub(crate) fn open_arena_records(&self) -> u32 {
        self.arena.staging.bodies.len() as u32
    }

    /// Whether `run`'s next part fits the open chunk; a uniform chunk that
    /// cannot take another record closes and the pass opens the next.
    pub(crate) fn arena_accepts(&self, chunk: usize, run: &RunDraw) -> bool {
        debug_assert_eq!(
            chunk + 1,
            self.arena.chunks.len(),
            "only the open chunk accepts"
        );
        let staging = &self.arena.staging;
        if staging.is_empty() {
            return true;
        }
        staging.fits(
            self.mode,
            1,
            run.tables().brushes.len().min(BRUSH_CHUNK),
            run.tables().stops.len().min(STOP_CHUNK),
        )
    }

    /// Copies `run` into the open chunk, one placement for all its records,
    /// its brushes and painted stops re-based onto the chunk's tables, and
    /// records the draw each segment takes, its records in order at the
    /// segment's vertex budget. Returns how many records were taken from
    /// `from`; the caller continues with the rest in a new chunk when a
    /// uniform chunk fills mid-run.
    pub(crate) fn append_arena(
        &mut self,
        chunk: usize,
        run: &RunDraw,
        window: std::ops::Range<u32>,
        (root_scale, turn): (f32, SegmentTransform),
        key_for: &mut dyn FnMut(&RecordSegment) -> crate::render::ShapePipelineKey,
        wanted: &mut SmallVec<[(crate::render::ShapePipelineKey, u64); 4]>,
    ) -> u32 {
        let from = window.start;
        let mode = self.mode;
        let fill_stats = self.fill_stats;
        debug_assert_eq!(
            chunk + 1,
            self.arena.chunks.len(),
            "only the open chunk appends"
        );
        let staging = &mut self.arena.staging;
        let tables = run.tables();
        let placement_index = staging.placements.len() as u32;
        staging
            .placements
            .push(PlacementData::of(&run.placement, root_scale, turn));
        staging.brush_map.clear();
        staging.brush_map.resize(tables.brushes.len(), u32::MAX);
        let layer = paint_layer(&run.placement);
        let record_limit = mode.arena_records();
        let mut taken = 0u32;
        let mut skipped = 0u32;
        for segment in run.segment_records() {
            let key = key_for(segment);
            let band_class = if mode.storage { segment.band_class } else { 0 };
            let class_segments = mode
                .storage
                .then(|| band_class_segments(segment.band_class));
            staging.arcs |= segment_holds_arcs(segment);
            let segment_start = taken;
            let mut stopped = false;
            for index in segment.range() {
                if skipped < from {
                    skipped += 1;
                    continue;
                }
                if from + taken >= window.end || staging.bodies.len() >= record_limit {
                    stopped = true;
                    break;
                }
                let mut body = tables.shapes.bodies()[index];
                if body.brush != 0 {
                    let source = (body.brush - 1) as usize;
                    if staging.brush_map[source] == u32::MAX {
                        let brush = tables.brushes[source];
                        let stop_range = brush.stop_start as usize
                            ..(brush.stop_start + brush.stop_count) as usize;
                        if !mode.storage
                            && (staging.brushes.len() >= BRUSH_CHUNK
                                || staging.stops.len() + brush.stop_count as usize > STOP_CHUNK)
                        {
                            stopped = true;
                            break;
                        }
                        painted_stops(&tables.stops[stop_range], &layer, &mut staging.painted);
                        let stop_start = staging.stops.len() as u32;
                        staging.stops.extend_from_slice(&staging.painted);
                        staging.brushes.push(BrushRecord {
                            stop_start,
                            ..brush
                        });
                        staging.brush_map[source] = staging.brushes.len() as u32;
                    }
                    body.brush = staging.brush_map[source];
                }
                body.placement = placement_index;
                let record_index = staging.bodies.len() as u32;
                staging.bodies.push(body);
                staging.curves.push(tables.shapes.curves()[index]);
                staging.push_draw(key, band_class, record_index, segment.occluders);
                if fill_stats {
                    staging.fill.add_record(
                        &tables.shapes.get(index).expect("recorded shape index"),
                        run.placement.offset,
                        root_scale,
                        class_segments,
                    );
                }
                taken += 1;
            }
            if taken > segment_start {
                let vertices = draw_vertices(taken - segment_start, band_class);
                match wanted.iter_mut().find(|(wanted, _)| *wanted == key) {
                    Some((_, total)) => *total += vertices,
                    None => wanted.push((key, vertices)),
                }
            }
            if stopped {
                break;
            }
        }
        taken
    }

    /// Moves the draws the open chunk recorded since it opened or was last
    /// cut onto `out`, with the strip indices they draw, leaving the chunk
    /// open: the records appended next start draws of their own.
    pub(crate) fn cut_arena(
        &mut self,
        device: &wgpu::Device,
        chunk: usize,
        out: &mut Vec<RunDrawCall>,
    ) {
        debug_assert_eq!(
            chunk + 1,
            self.arena.chunks.len(),
            "only the open chunk cuts"
        );
        for draw in &self.arena.staging.draws {
            let class = draw.band_class;
            self.strip_indices[class as usize].ensure(
                device,
                self.upload,
                band_class_segments(class),
            );
        }
        out.append(&mut self.arena.staging.draws);
    }

    /// Places the open chunk's tables in the frame's arena, moves the draws
    /// recorded since its last cut onto `out` and returns the chunk's fill.
    pub(crate) fn close_arena(
        &mut self,
        device: &wgpu::Device,
        chunk: usize,
        out: &mut Vec<RunDrawCall>,
    ) -> Option<ShapeFill> {
        if self.arena.staging.is_empty() {
            return None;
        }
        self.cut_arena(device, chunk, out);
        let staging = std::mem::take(&mut self.arena.staging);
        let placed = self.arena.place(
            device,
            &self.layout,
            [
                bytemuck::cast_slice(&staging.bodies),
                bytemuck::cast_slice(&staging.curves),
                bytemuck::cast_slice(&staging.brushes),
                bytemuck::cast_slice(&staging.stops),
                bytemuck::cast_slice(&staging.placements),
            ],
        );
        self.arena.generations[placed.generation].arcs |= staging.arcs;
        self.arena.chunks[chunk] = placed;
        let fill = self.fill_stats.then_some(staging.fill);
        self.arena.staging = staging;
        fill
    }
}

/// The shape segments of `run` that draw: not the content markers and not
/// the other lane.
pub(crate) fn run_has_shapes(run: &RunDraw) -> bool {
    run.tables().segments[run.segments.start as usize..run.segments.end as usize]
        .iter()
        .any(|segment| segment.lane == RecordLane::Shapes && segment.count > 0)
}

#[cfg(test)]
#[path = "tests/run_store_tests.rs"]
mod tests;
