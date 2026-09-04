use std::{collections::HashMap, sync::Arc};

use bytemuck::{Pod, Zeroable};
use cranpose_render_common::{graph::DrawCommandId, style_shared::apply_layer_to_color};
use cranpose_ui_graphics::{
    ARC_BUCKET_SEGMENTS, ARC_BUCKETS, BrushRecord, Color, GradientStopRecord, GraphicsLayer,
    QUAD_VERTICES, RecordLane, RecordTables, ShapeRecord, band_bucket,
};
use smallvec::SmallVec;

use crate::{
    frame_graph::{FrameCommandStats, write_buffer},
    geometry::{canonicalized_scaled_rect, snap_delta_for_anchor, snapped_anchor_device_origin},
    run_geometry::ShapeFill,
    scene::{Placement, RunDraw},
};

/// The records one uniform-mode chunk holds: the 16 KB WebGL binding floor
/// over the 112-byte record, rounded down to a 256-byte offset multiple.
pub(crate) const RECORD_CHUNK: usize = 128;
pub(crate) const BRUSH_CHUNK: usize = 256;
pub(crate) const STOP_CHUNK: usize = 256;
pub(crate) const PLACEMENT_CHUNK: usize = 64;
const BAND_CHUNK_VEC4S: usize = 4;

/// Runs with at least this many records keep retained GPU buffers keyed by
/// their command; smaller runs are copied into the frame arena, where
/// consecutive runs share a draw.
pub(crate) const STORE_RUN_MIN_RECORDS: u32 = 64;
const STORE_IDLE_FRAMES: u64 = 120;
const INITIAL_STORE_RECORDS: usize = 256;
const INITIAL_ARENA_RECORDS: usize = 1024;
const INITIAL_BRUSHES: usize = 64;
const INITIAL_STOPS: usize = 128;
const INITIAL_PLACEMENTS: usize = 64;
const INITIAL_BAND_VEC4S: usize = 64;
const DUMMY_VEC4S: usize = 4;

const PLACEMENT_CANONICALIZE: u32 = 1;
const PLACEMENT_CLIPPED: u32 = 2;
const PLACEMENT_FILTERED: u32 = 4;
const PLACEMENT_PAINTED: u32 = 8;

/// The run-table binding mode the device supports: storage buffers hold a
/// recording whole and draw wide arcs as bands; the uniform fallback (the
/// WebGL floor) draws every run from fixed-size chunks as quads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RunBufferMode {
    pub(crate) storage: bool,
}

impl RunBufferMode {
    pub(crate) fn for_device(device: &wgpu::Device, downlevel: wgpu::DownlevelFlags) -> Self {
        Self::select(&device.limits(), downlevel)
    }

    pub(crate) fn select(limits: &wgpu::Limits, _downlevel: wgpu::DownlevelFlags) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        if limits.max_storage_buffers_per_shader_stage >= RUN_BINDINGS as u32
            && _downlevel.contains(wgpu::DownlevelFlags::VERTEX_STORAGE)
        {
            return Self { storage: true };
        }
        let _ = limits;
        Self { storage: false }
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
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST
        } else {
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST
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

const RUN_BINDINGS: usize = 5;

/// A placement as the vertex stage reads it: the offset with the snap
/// delta folded in, the device clip, the dither origin and the paint.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct PlacementData {
    offset: [f32; 2],
    root_scale: f32,
    flags: u32,
    clip: [f32; 4],
    dither_origin: [f32; 2],
    alpha: f32,
    reserved: f32,
    color_matrix: [[f32; 4]; 4],
    color_offset: [f32; 4],
}

impl PlacementData {
    pub(crate) fn of(placement: &Placement, root_scale: f32) -> Self {
        let snap_delta = placement
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
            .unwrap_or_default();
        let canonicalize = placement.snap_anchor.is_some();
        let mut flags = 0;
        if canonicalize {
            flags |= PLACEMENT_CANONICALIZE;
        }
        if placement.alpha != 1.0 || placement.color_filter.is_some() {
            flags |= PLACEMENT_PAINTED;
        }
        let clip = match placement.clip {
            Some(clip) => {
                flags |= PLACEMENT_CLIPPED;
                let device = if canonicalize {
                    canonicalized_scaled_rect(clip, root_scale)
                } else {
                    cranpose_ui_graphics::Rect {
                        x: clip.x * root_scale,
                        y: clip.y * root_scale,
                        width: clip.width * root_scale,
                        height: clip.height * root_scale,
                    }
                };
                [device.x, device.y, device.width, device.height]
            }
            None => [0.0; 4],
        };
        let dither_origin = placement
            .snap_anchor
            .map(|anchor| snapped_anchor_device_origin(anchor, root_scale))
            .unwrap_or_default();
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
            reserved: 0.0,
            color_matrix,
            color_offset,
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

/// The five buffers one bind group of run tables reads, with the element
/// capacity of each.
pub(crate) struct RunBuffers {
    buffers: [wgpu::Buffer; RUN_BINDINGS],
    capacities: [usize; RUN_BINDINGS],
    pub(crate) bind_group: wgpu::BindGroup,
    mode: RunBufferMode,
}

/// Bytes compared at a time when a stored run's tables change, so the
/// arena whose ball moved re-uploads the ball's chunk, not the arena.
/// Every element size divides it or is divided by it, so a chunk edge is
/// an element edge and a copy-aligned offset.
const UPLOAD_CHUNK_BYTES: usize = 4096;

const ELEMENT_SIZES: [usize; RUN_BINDINGS] = [
    std::mem::size_of::<ShapeRecord>(),
    std::mem::size_of::<BrushRecord>(),
    std::mem::size_of::<GradientStopRecord>(),
    std::mem::size_of::<[u32; 4]>(),
    std::mem::size_of::<PlacementData>(),
];
const LABELS: [&str; RUN_BINDINGS] = [
    "Run Records",
    "Run Brushes",
    "Run Gradient Stops",
    "Run Band Records",
    "Run Placements",
];

impl RunBuffers {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        mode: RunBufferMode,
        capacities: [usize; RUN_BINDINGS],
    ) -> Self {
        let buffers = std::array::from_fn(|index| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(LABELS[index]),
                size: (ELEMENT_SIZES[index] * capacities[index]) as u64,
                usage: mode.usage(),
                mapped_at_creation: false,
            })
        });
        let bind_group = Self::bind(device, layout, &buffers);
        Self {
            buffers,
            capacities,
            bind_group,
            mode,
        }
    }

    fn bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffers: &[wgpu::Buffer; RUN_BINDINGS],
    ) -> wgpu::BindGroup {
        let entries: [wgpu::BindGroupEntry<'_>; RUN_BINDINGS] =
            std::array::from_fn(|index| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: buffers[index].as_entire_binding(),
            });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Run Tables Bind Group"),
            layout,
            entries: &entries,
        })
    }

    /// Grows any buffer below `needed` elements and rebinds; says, per
    /// buffer, whether it was recreated and so holds nothing yet.
    fn ensure(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        needed: [usize; RUN_BINDINGS],
    ) -> [bool; RUN_BINDINGS] {
        let mut fresh = [false; RUN_BINDINGS];
        for index in 0..RUN_BINDINGS {
            if needed[index] > self.capacities[index] {
                let capacity = needed[index].next_power_of_two();
                self.buffers[index] = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(LABELS[index]),
                    size: (ELEMENT_SIZES[index] * capacity) as u64,
                    usage: self.mode.usage(),
                    mapped_at_creation: false,
                });
                self.capacities[index] = capacity;
                fresh[index] = true;
            }
        }
        if fresh.contains(&true) {
            self.bind_group = Self::bind(device, layout, &self.buffers);
        }
        fresh
    }

    fn write<T: Pod>(&self, queue: &wgpu::Queue, index: usize, data: &[T]) -> FrameCommandStats {
        if data.is_empty() {
            return FrameCommandStats::default();
        }
        write_buffer(queue, &self.buffers[index], 0, bytemuck::cast_slice(data))
    }

    /// Writes what `data` changes against `previous`, the buffer's
    /// contents: the chunks of [`UPLOAD_CHUNK_BYTES`] that differ, joined
    /// when adjacent, and everything past the shorter table; the whole of
    /// `data` when `fresh` says the buffer holds nothing.
    fn write_changed<T: Pod>(
        &self,
        queue: &wgpu::Queue,
        index: usize,
        previous: &[T],
        data: &[T],
        fresh: bool,
    ) -> FrameCommandStats {
        let bytes = bytemuck::cast_slice::<T, u8>(data);
        if fresh {
            return write_buffer(queue, &self.buffers[index], 0, bytes);
        }
        let previous = bytemuck::cast_slice::<T, u8>(previous);
        let shared = previous.len().min(bytes.len());
        let mut stats = FrameCommandStats::default();
        let mut pending = None;
        let mut offset = 0;
        while offset < shared {
            let end = (offset + UPLOAD_CHUNK_BYTES).min(shared);
            let changed = bytes[offset..end] != previous[offset..end];
            match (changed, pending) {
                (true, None) => pending = Some(offset),
                (false, Some(from)) => {
                    stats += write_buffer(
                        queue,
                        &self.buffers[index],
                        from as u64,
                        &bytes[from..offset],
                    );
                    pending = None;
                }
                _ => {}
            }
            offset = end;
        }
        let from = pending.unwrap_or(shared);
        if from < bytes.len() {
            stats += write_buffer(queue, &self.buffers[index], from as u64, &bytes[from..]);
        }
        stats
    }
}

fn packed_bands(tables: &RecordTables) -> (Vec<[u32; 4]>, [u32; ARC_BUCKETS]) {
    let mut flat: Vec<u32> = Vec::new();
    let mut bases = [0u32; ARC_BUCKETS];
    for (bucket, indices) in tables.arc_buckets.iter().enumerate() {
        bases[bucket] = flat.len() as u32;
        flat.extend_from_slice(indices);
    }
    (pack_vec4s(flat), bases)
}

/// A recording's tables resident on the GPU, keyed by its command.
pub(crate) struct StoredRun {
    pub(crate) buffers: RunBuffers,
    tables: Arc<RecordTables>,
    bands: Vec<[u32; 4]>,
    paint: PaintKey,
    /// Where each bucket's entries start in the band table.
    pub(crate) band_bases: [u32; ARC_BUCKETS],
    fill: Option<ShapeFill>,
    fill_scale_bits: u32,
    fill_offset_bits: [u32; 2],
    last_used_frame: u64,
}

/// A draw the pass records for one run: a pipeline and a vertex range
/// into the tables its batch binds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunDrawCall {
    pub(crate) key: crate::render::ShapePipelineKey,
    pub(crate) vertices: std::ops::Range<u32>,
}

/// A draw an arena chunk will make, before the chunk's band table is
/// packed: quads name their vertices; a band draw names its bucket and the
/// bucket entries it covers, resolved to vertices at close.
enum PendingDraw {
    Quads {
        key: crate::render::ShapePipelineKey,
        vertices: std::ops::Range<u32>,
    },
    Bands {
        key: crate::render::ShapePipelineKey,
        bucket: usize,
        entries: std::ops::Range<u32>,
    },
}

/// The CPU side of one arena chunk while a pass fills it.
#[derive(Default)]
pub(crate) struct ArenaStaging {
    records: Vec<ShapeRecord>,
    brushes: Vec<BrushRecord>,
    stops: Vec<GradientStopRecord>,
    placements: Vec<PlacementData>,
    bands: [Vec<u32>; ARC_BUCKETS],
    brush_map: Vec<u32>,
    painted: Vec<GradientStopRecord>,
    pending: Vec<PendingDraw>,
    pub(crate) fill: ShapeFill,
}

impl ArenaStaging {
    fn clear(&mut self) {
        self.records.clear();
        self.brushes.clear();
        self.stops.clear();
        self.placements.clear();
        for bucket in &mut self.bands {
            bucket.clear();
        }
        self.pending.clear();
        self.fill = ShapeFill::default();
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.records.capacity() * std::mem::size_of::<ShapeRecord>()
            + self.brushes.capacity() * std::mem::size_of::<BrushRecord>()
            + self.stops.capacity() * std::mem::size_of::<GradientStopRecord>()
            + self.placements.capacity() * std::mem::size_of::<PlacementData>()
    }

    fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    fn fits(&self, mode: RunBufferMode, records: usize, brushes: usize, stops: usize) -> bool {
        if mode.storage {
            return true;
        }
        self.records.len() + records <= RECORD_CHUNK
            && self.brushes.len() + brushes <= BRUSH_CHUNK
            && self.stops.len() + stops <= STOP_CHUNK
            && self.placements.len() < PLACEMENT_CHUNK
    }

    fn push_quads(&mut self, key: crate::render::ShapePipelineKey, vertices: std::ops::Range<u32>) {
        if let Some(PendingDraw::Quads {
            key: last_key,
            vertices: last,
        }) = self.pending.last_mut()
            && *last_key == key
            && last.end == vertices.start
        {
            last.end = vertices.end;
            return;
        }
        self.pending.push(PendingDraw::Quads { key, vertices });
    }

    /// The chunk's draws with the band table packed: bucket after bucket,
    /// so a band draw's entries become vertices from its bucket's base.
    fn resolve_draws(&mut self) -> (Vec<[u32; 4]>, Vec<RunDrawCall>) {
        let mut flat: Vec<u32> = Vec::new();
        let mut bases = [0u32; ARC_BUCKETS];
        for (bucket, indices) in self.bands.iter().enumerate() {
            bases[bucket] = flat.len() as u32;
            flat.extend_from_slice(indices);
        }
        let draws = self
            .pending
            .drain(..)
            .map(|draw| match draw {
                PendingDraw::Quads { key, vertices } => RunDrawCall { key, vertices },
                PendingDraw::Bands {
                    key,
                    bucket,
                    entries,
                } => {
                    let per_band = ARC_BUCKET_SEGMENTS[bucket] * QUAD_VERTICES;
                    let base = bases[bucket];
                    RunDrawCall {
                        key,
                        vertices: (base + entries.start) * per_band
                            ..(base + entries.end) * per_band,
                    }
                }
            })
            .collect();
        (pack_vec4s(flat), draws)
    }
}

fn pack_vec4s(mut flat: Vec<u32>) -> Vec<[u32; 4]> {
    while !flat.len().is_multiple_of(4) {
        flat.push(0);
    }
    flat.as_chunks::<4>().0.to_vec()
}

/// The GPU home of every run: retained tables per command, and the
/// per-pass arena chunks small runs are copied into.
pub(crate) struct RunStore {
    mode: RunBufferMode,
    layout: wgpu::BindGroupLayout,
    stored: HashMap<DrawCommandId, StoredRun>,
    arena_buffers: Vec<RunBuffers>,
    arena_staging: Vec<ArenaStaging>,
    arena_cursor: usize,
    scratch_stops: Vec<GradientStopRecord>,
    frame: u64,
    fill_stats: bool,
}

impl RunStore {
    pub(crate) fn new(device: &wgpu::Device, mode: RunBufferMode) -> Self {
        let binding = |index: u32| wgpu::BindGroupLayoutEntry {
            binding: index,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: mode.binding_type(),
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Run Tables Bind Group Layout"),
            entries: &[binding(0), binding(1), binding(2), binding(3), binding(4)],
        });
        Self {
            mode,
            layout,
            stored: HashMap::new(),
            arena_buffers: Vec::new(),
            arena_staging: Vec::new(),
            arena_cursor: 0,
            fill_stats: false,
            scratch_stops: Vec::new(),
            frame: 0,
        }
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
    pub(crate) fn begin_frame(&mut self, fill_stats: bool) {
        self.fill_stats = fill_stats;
        self.frame += 1;
        self.arena_cursor = 0;
        let frame = self.frame;
        self.stored
            .retain(|_, run| frame - run.last_used_frame <= STORE_IDLE_FRAMES);
    }

    pub(crate) fn end_frame(&mut self) {
        const ARENA_POOL_MARGIN: usize = 4;
        self.arena_buffers
            .truncate(self.arena_cursor.saturating_add(ARENA_POOL_MARGIN));
        self.arena_staging
            .truncate(self.arena_cursor.saturating_add(ARENA_POOL_MARGIN));
    }

    pub(crate) fn stored_count(&self) -> usize {
        self.stored.len()
    }

    pub(crate) fn stored_bytes(&self) -> usize {
        self.stored
            .values()
            .map(|run| {
                run.buffers
                    .capacities
                    .iter()
                    .zip(ELEMENT_SIZES)
                    .map(|(capacity, size)| capacity * size)
                    .sum::<usize>()
            })
            .sum()
    }

    pub(crate) fn arena_staging_bytes(&self) -> usize {
        self.arena_staging
            .iter()
            .map(ArenaStaging::heap_bytes)
            .sum()
    }

    pub(crate) fn stored(&self, command: &DrawCommandId) -> Option<&StoredRun> {
        self.stored.get(command)
    }

    pub(crate) fn arena(&self, chunk: usize) -> &RunBuffers {
        &self.arena_buffers[chunk]
    }

    /// Whether `run` keeps retained buffers rather than joining the arena.
    pub(crate) fn is_stored(&self, run: &RunDraw) -> bool {
        self.mode.storage && run.command.is_some() && run.record_count() >= STORE_RUN_MIN_RECORDS
    }

    /// Brings a stored run's tables up to date: nothing is written when the
    /// recorder handed back the same tables, or new tables with the same
    /// bytes, under the same paint. Returns the upload stats and the run's
    /// fill for `root_scale`.
    pub(crate) fn upload_stored(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        run: &RunDraw,
        root_scale: f32,
    ) -> (FrameCommandStats, Option<ShapeFill>) {
        let command = run.command.expect("a stored run has a command");
        let paint = PaintKey::of(&run.placement);
        let layout = &self.layout;
        let frame = self.frame;
        let mode = self.mode;
        let scratch_stops = &mut self.scratch_stops;
        let entry = self.stored.entry(command).or_insert_with(|| StoredRun {
            buffers: RunBuffers::new(
                device,
                layout,
                mode,
                [
                    INITIAL_STORE_RECORDS,
                    INITIAL_BRUSHES,
                    INITIAL_STOPS,
                    INITIAL_BAND_VEC4S,
                    DUMMY_VEC4S,
                ],
            ),
            tables: Arc::new(RecordTables::default()),
            bands: Vec::new(),
            paint,
            band_bases: [0; ARC_BUCKETS],
            fill: None,
            fill_scale_bits: 0,
            fill_offset_bits: [0; 2],
            last_used_frame: 0,
        });
        let first_use = entry.last_used_frame == 0;
        entry.last_used_frame = frame;
        let same_paint = entry.paint == paint;
        let mut stats = FrameCommandStats::default();
        let mut stops_changed = first_use;
        if first_use || !Arc::ptr_eq(&entry.tables, &run.tables) {
            let tables = &*run.tables;
            let (bands, bases) = packed_bands(tables);
            let fresh = entry.buffers.ensure(
                device,
                layout,
                [
                    tables.shapes.len().max(1),
                    tables.brushes.len().max(1),
                    tables.stops.len().max(1),
                    bands.len().max(DUMMY_VEC4S),
                    DUMMY_VEC4S,
                ],
            );
            let previous = &*entry.tables;
            stats += entry.buffers.write_changed(
                queue,
                0,
                &previous.shapes,
                &tables.shapes,
                first_use || fresh[0],
            );
            stats += entry.buffers.write_changed(
                queue,
                1,
                &previous.brushes,
                &tables.brushes,
                first_use || fresh[1],
            );
            stats +=
                entry
                    .buffers
                    .write_changed(queue, 3, &entry.bands, &bands, first_use || fresh[3]);
            stops_changed |= fresh[2] || previous.stops != tables.stops;
            entry.bands = bands;
            entry.band_bases = bases;
            entry.tables = Arc::clone(&run.tables);
        }
        let changed = stats.upload_bytes > 0 || stops_changed;
        if stops_changed || !same_paint {
            painted_stops(
                &run.tables.stops,
                &paint_layer(&run.placement),
                scratch_stops,
            );
            stats += entry.buffers.write(queue, 2, scratch_stops);
            entry.paint = paint;
        }
        let offset_bits = [
            run.placement.offset.x.to_bits(),
            run.placement.offset.y.to_bits(),
        ];
        if changed
            || entry.fill_scale_bits != root_scale.to_bits()
            || entry.fill_offset_bits != offset_bits
        {
            entry.fill = None;
            entry.fill_scale_bits = root_scale.to_bits();
            entry.fill_offset_bits = offset_bits;
        }
        let fill = self.fill_stats.then(|| {
            *entry.fill.get_or_insert_with(|| {
                ShapeFill::of_tables(&run.tables, run.placement.offset, root_scale, true)
            })
        });
        (stats, fill)
    }

    /// Opens the arena chunk a pass appends to.
    pub(crate) fn open_arena(&mut self, device: &wgpu::Device) -> usize {
        let chunk = self.arena_cursor;
        self.arena_cursor += 1;
        while self.arena_buffers.len() <= chunk {
            self.arena_buffers.push(RunBuffers::new(
                device,
                &self.layout,
                self.mode,
                [
                    if self.mode.storage {
                        INITIAL_ARENA_RECORDS
                    } else {
                        RECORD_CHUNK
                    },
                    if self.mode.storage {
                        INITIAL_BRUSHES
                    } else {
                        BRUSH_CHUNK
                    },
                    if self.mode.storage {
                        INITIAL_STOPS
                    } else {
                        STOP_CHUNK
                    },
                    if self.mode.storage {
                        DUMMY_VEC4S
                    } else {
                        BAND_CHUNK_VEC4S
                    },
                    if self.mode.storage {
                        INITIAL_PLACEMENTS
                    } else {
                        PLACEMENT_CHUNK
                    },
                ],
            ));
        }
        while self.arena_staging.len() <= chunk {
            self.arena_staging.push(ArenaStaging::default());
        }
        self.arena_staging[chunk].clear();
        chunk
    }

    /// Whether `run`'s next part fits the open chunk; a uniform chunk that
    /// cannot take another record closes and the pass opens the next.
    pub(crate) fn arena_accepts(&self, chunk: usize, run: &RunDraw) -> bool {
        let staging = &self.arena_staging[chunk];
        if staging.is_empty() {
            return true;
        }
        staging.fits(
            self.mode,
            1,
            run.tables.brushes.len().min(BRUSH_CHUNK),
            run.tables.stops.len().min(STOP_CHUNK),
        )
    }

    /// Copies `run` into the open chunk, one placement for all its records,
    /// its brushes and painted stops re-based onto the chunk's tables, and
    /// records the draws its segments take: the quads, then the bands of
    /// each bucket the segment put arcs in. Returns how many records were
    /// taken from `from`; the caller continues with the rest in a new
    /// chunk when a uniform chunk fills mid-run.
    pub(crate) fn append_arena(
        &mut self,
        chunk: usize,
        run: &RunDraw,
        from: u32,
        root_scale: f32,
        key_for: &mut dyn FnMut(
            &cranpose_ui_graphics::RecordSegment,
            Option<usize>,
        ) -> crate::render::ShapePipelineKey,
    ) -> u32 {
        let mode = self.mode;
        let fill_stats = self.fill_stats;
        let staging = &mut self.arena_staging[chunk];
        let tables = &*run.tables;
        let placement_index = staging.placements.len() as u32;
        staging
            .placements
            .push(PlacementData::of(&run.placement, root_scale));
        staging.brush_map.clear();
        staging.brush_map.resize(tables.brushes.len(), u32::MAX);
        let layer = paint_layer(&run.placement);
        let record_limit = mode.arena_records();
        let mut taken = 0u32;
        let mut skipped = 0u32;
        for segment in run.segment_records() {
            let key = key_for(segment, None);
            let bands_before: [u32; ARC_BUCKETS] =
                std::array::from_fn(|bucket| staging.bands[bucket].len() as u32);
            let mut segment_complete = true;
            for index in segment.range() {
                if skipped < from {
                    skipped += 1;
                    continue;
                }
                if staging.records.len() >= record_limit {
                    segment_complete = false;
                    break;
                }
                let mut record = tables.shapes[index];
                if record.brush != 0 {
                    let source = (record.brush - 1) as usize;
                    if staging.brush_map[source] == u32::MAX {
                        let brush = tables.brushes[source];
                        let stop_range = brush.stop_start as usize
                            ..(brush.stop_start + brush.stop_count) as usize;
                        if !mode.storage
                            && (staging.brushes.len() >= BRUSH_CHUNK
                                || staging.stops.len() + brush.stop_count as usize > STOP_CHUNK)
                        {
                            segment_complete = false;
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
                    record.brush = staging.brush_map[source];
                }
                record.reserved = placement_index;
                let record_index = staging.records.len() as u32;
                staging.records.push(record);
                staging.push_quads(
                    key,
                    record_index * QUAD_VERTICES..(record_index + 1) * QUAD_VERTICES,
                );
                if mode.storage && record.is_banded() {
                    staging.bands[band_bucket(record.band_segments())].push(record_index);
                }
                if fill_stats {
                    staging.fill.add_record(
                        &record,
                        run.placement.offset,
                        root_scale,
                        mode.storage,
                    );
                }
                taken += 1;
            }
            for (bucket, before) in bands_before.into_iter().enumerate() {
                let after = staging.bands[bucket].len() as u32;
                if after > before {
                    staging.pending.push(PendingDraw::Bands {
                        key: key_for(segment, Some(bucket)),
                        bucket,
                        entries: before..after,
                    });
                }
            }
            if !segment_complete {
                return taken;
            }
        }
        taken
    }

    /// Uploads the open chunk's tables; returns the stats and the draws.
    pub(crate) fn close_arena(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chunk: usize,
    ) -> (FrameCommandStats, Vec<RunDrawCall>, Option<ShapeFill>) {
        let staging = &mut self.arena_staging[chunk];
        let buffers = &mut self.arena_buffers[chunk];
        if staging.is_empty() {
            return (FrameCommandStats::default(), Vec::new(), None);
        }
        let (bands, draws) = staging.resolve_draws();
        buffers.ensure(
            device,
            &self.layout,
            [
                staging.records.len(),
                staging.brushes.len().max(1),
                staging.stops.len().max(1),
                bands.len().max(DUMMY_VEC4S),
                staging.placements.len(),
            ],
        );
        let mut stats = buffers.write(queue, 0, &staging.records);
        stats += buffers.write(queue, 1, &staging.brushes);
        stats += buffers.write(queue, 2, &staging.stops);
        stats += buffers.write(queue, 3, &bands);
        stats += buffers.write(queue, 4, &staging.placements);
        (stats, draws, self.fill_stats.then_some(staging.fill))
    }
}

/// The draws one stored run takes for one segment range: the quad draw of
/// the segment's records, then one band draw per bucket the segment has
/// banded arcs in.
pub(crate) fn stored_run_draws(
    stored: &StoredRun,
    run: &RunDraw,
    key_for: &mut dyn FnMut(
        &cranpose_ui_graphics::RecordSegment,
        Option<usize>,
    ) -> crate::render::ShapePipelineKey,
    out: &mut SmallVec<[RunDrawCall; 8]>,
) {
    let tables = &*run.tables;
    for segment in run.segment_records() {
        let start = segment.start;
        let end = segment.start + segment.count;
        out.push(RunDrawCall {
            key: key_for(segment, None),
            vertices: start * QUAD_VERTICES..end * QUAD_VERTICES,
        });
        if !tables.shapes[segment.range()]
            .iter()
            .any(ShapeRecord::is_banded)
        {
            continue;
        }
        for (bucket, indices) in tables.arc_buckets.iter().enumerate() {
            let first = indices.partition_point(|index| *index < start);
            let last = indices.partition_point(|index| *index < end);
            if first == last {
                continue;
            }
            let per_band = ARC_BUCKET_SEGMENTS[bucket] * QUAD_VERTICES;
            let base = stored.band_bases[bucket];
            out.push(RunDrawCall {
                key: key_for(segment, Some(bucket)),
                vertices: (base + first as u32) * per_band..(base + last as u32) * per_band,
            });
        }
    }
}

/// The shape segments of `run` that draw: not the content markers and not
/// the other lane.
pub(crate) fn run_has_shapes(run: &RunDraw) -> bool {
    run.tables.segments[run.segments.start as usize..run.segments.end as usize]
        .iter()
        .any(|segment| segment.lane == RecordLane::Shapes && segment.count > 0)
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::{ColorFilter, Point};

    use super::*;

    #[test]
    fn a_placement_folds_its_snap_delta_clip_and_filter_into_the_uniform() {
        let placement = Placement {
            offset: Point::new(10.25, 20.0),
            snap_anchor: Some(crate::scene::SnapAnchor::rigid(Point::new(0.3, 0.0))),
            clip: Some(cranpose_ui_graphics::Rect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            }),
            alpha: 0.5,
            color_filter: Some(ColorFilter::modulate(Color(0.5, 0.25, 1.0, 1.0))),
        };
        let data = PlacementData::of(&placement, 2.0);
        assert_eq!(
            data.flags,
            PLACEMENT_CANONICALIZE | PLACEMENT_CLIPPED | PLACEMENT_FILTERED | PLACEMENT_PAINTED
        );
        assert_eq!(data.root_scale, 2.0);
        assert!((data.offset[0] - 10.45).abs() < 1e-5, "{:?}", data.offset);
        assert_eq!(data.clip, [2.0, 4.0, 6.0, 8.0]);
        assert_eq!(data.alpha, 0.5);
        assert_eq!(data.color_matrix[0][0], 0.5);
        assert_eq!(data.color_matrix[1][1], 0.25);
        assert_eq!(data.color_offset, [0.0; 4]);
        let plain = PlacementData::of(&Placement::at(Point::default(), None, None), 1.0);
        assert_eq!(plain.flags, 0);
        assert_eq!(plain.color_matrix[2][2], 1.0);
        assert_eq!(std::mem::size_of::<PlacementData>(), 128);
    }

    #[test]
    fn bands_pack_bucket_by_bucket_with_their_bases() {
        let mut tables = RecordTables::default();
        tables.arc_buckets[0] = vec![3, 5];
        tables.arc_buckets[2] = vec![7, 8, 9];
        let (packed, bases) = packed_bands(&tables);
        assert_eq!(bases, [0, 2, 2, 5, 5, 5, 5]);
        assert_eq!(packed, vec![[3, 5, 7, 8], [9, 0, 0, 0]]);
    }
}
