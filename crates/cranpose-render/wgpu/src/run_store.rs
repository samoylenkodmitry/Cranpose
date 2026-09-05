use std::{collections::HashMap, sync::Arc};

use bytemuck::{Pod, Zeroable};
use cranpose_render_common::{graph::DrawCommandId, style_shared::apply_layer_to_color};
use cranpose_ui_graphics::{
    ARC_BUCKETS, BrushRecord, Color, GradientStopRecord, GraphicsLayer, RecordLane, RecordSegment,
    RecordTables, ShapeRecord, band_class_segments, strip_index_pattern, strip_indices,
};
use smallvec::SmallVec;

use crate::{
    frame_graph::{FrameCommandStats, UploadPlacement, place_upload, write_buffer},
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
/// The store tier binds a placement table it never reads: one entry.
const STORE_PLACEMENTS: usize = 1;

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

pub(crate) const RUN_BINDINGS: usize = 4;

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
    std::mem::size_of::<PlacementData>(),
];
const LABELS: [&str; RUN_BINDINGS] = [
    "Run Records",
    "Run Brushes",
    "Run Gradient Stops",
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

/// A recording's tables resident on the GPU, keyed by its command.
pub(crate) struct StoredRun {
    pub(crate) buffers: RunBuffers,
    tables: Arc<RecordTables>,
    paint: PaintKey,
    fill: Option<ShapeFill>,
    fill_scale_bits: u32,
    fill_offset_bits: [u32; 2],
    last_used_frame: u64,
}

/// A draw the pass records for one run: a pipeline and the records it
/// instances, in order, over the strip index pattern of the pipeline's
/// band class, from the tables its batch binds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunDrawCall {
    pub(crate) key: crate::render::ShapePipelineKey,
    pub(crate) records: std::ops::Range<u32>,
}

impl RunDrawCall {
    /// The indices each record of the draw is instanced over.
    pub(crate) fn indices(&self) -> std::ops::Range<u32> {
        0..strip_indices(band_class_segments(self.key.band_class))
    }
}

/// The index buffer every draw at one band class instances over: the
/// class's strip pattern, once.
#[derive(Default)]
struct StripIndexBuffer {
    buffer: Option<wgpu::Buffer>,
}

impl StripIndexBuffer {
    fn ensure(&mut self, device: &wgpu::Device, segments: u32) {
        if self.buffer.is_some() {
            return;
        }
        let indices: Vec<u32> = strip_index_pattern(segments).collect();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Run Strip Indices"),
            size: std::mem::size_of_val(indices.as_slice()) as u64,
            usage: wgpu::BufferUsages::INDEX,
            mapped_at_creation: true,
        });
        buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&indices));
        buffer.unmap();
        self.buffer = Some(buffer);
    }
}

/// The CPU side of one arena chunk while a pass fills it.
#[derive(Default)]
pub(crate) struct ArenaStaging {
    records: Vec<ShapeRecord>,
    brushes: Vec<BrushRecord>,
    stops: Vec<GradientStopRecord>,
    placements: Vec<PlacementData>,
    brush_map: Vec<u32>,
    painted: Vec<GradientStopRecord>,
    draws: Vec<RunDrawCall>,
    pub(crate) fill: ShapeFill,
}

impl ArenaStaging {
    fn clear(&mut self) {
        self.records.clear();
        self.brushes.clear();
        self.stops.clear();
        self.placements.clear();
        self.draws.clear();
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

    /// Records `record` under `key`, extending the last draw when it
    /// continues it.
    fn push_draw(&mut self, key: crate::render::ShapePipelineKey, record: u32) {
        if let Some(last) = self.draws.last_mut()
            && last.key == key
            && last.records.end == record
        {
            last.records.end = record + 1;
            return;
        }
        self.draws.push(RunDrawCall {
            key,
            records: record..record + 1,
        });
    }
}

/// Where one closed chunk's tables sit: the frame's generation of arena
/// buffers and the chunk's dynamic offset into each table.
#[derive(Clone, Copy, Default)]
struct ArenaChunk {
    generation: usize,
    offsets: [u32; RUN_BINDINGS],
}

/// The tables a chunk's draws bind.
pub(crate) struct ArenaBinding<'a> {
    pub(crate) bind_group: &'a wgpu::BindGroup,
    pub(crate) offsets: [u32; RUN_BINDINGS],
}

/// One buffer per table with every chunk of the frame laid in it at an
/// aligned offset, the bytes staged on the CPU until the frame's flush.
struct ArenaGeneration {
    buffers: [wgpu::Buffer; RUN_BINDINGS],
    capacities: [u64; RUN_BINDINGS],
    staged: [Vec<u8>; RUN_BINDINGS],
    bind_group: wgpu::BindGroup,
}

impl ArenaGeneration {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        mode: RunBufferMode,
        capacities: [u64; RUN_BINDINGS],
        bindings: [u64; RUN_BINDINGS],
    ) -> Self {
        let buffers = std::array::from_fn(|index| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(LABELS[index]),
                size: capacities[index],
                usage: mode.usage(),
                mapped_at_creation: false,
            })
        });
        let bind_group = Self::bind(device, layout, &buffers, bindings);
        Self {
            buffers,
            capacities,
            staged: Default::default(),
            bind_group,
        }
    }

    fn bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffers: &[wgpu::Buffer; RUN_BINDINGS],
        bindings: [u64; RUN_BINDINGS],
    ) -> wgpu::BindGroup {
        let entries: [wgpu::BindGroupEntry<'_>; RUN_BINDINGS] =
            std::array::from_fn(|index| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffers[index],
                    offset: 0,
                    size: wgpu::BufferSize::new(bindings[index]),
                }),
            });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Run Arena Bind Group"),
            layout,
            entries: &entries,
        })
    }
}

/// The frame's arena: the chunks every pass closed, each a set of offsets
/// into the generation of buffers that held it, and the staging of the
/// chunk being filled. A chunk that outgrows the buffers opens a larger
/// generation; the one it left stays bound to the draws already recorded
/// until the flush writes both. Each table is bound at a fixed size, the
/// widest chunk seen, so a chunk's dynamic offset needs that much room
/// after it.
struct ArenaTables {
    mode: RunBufferMode,
    alignment: u64,
    bindings: [u64; RUN_BINDINGS],
    generations: Vec<ArenaGeneration>,
    chunks: Vec<ArenaChunk>,
    staging: ArenaStaging,
}

const INITIAL_ARENA_CAPACITIES: [usize; RUN_BINDINGS] = [
    INITIAL_ARENA_RECORDS,
    INITIAL_BRUSHES,
    INITIAL_STOPS,
    INITIAL_PLACEMENTS,
];

const UNIFORM_CHUNKS: [usize; RUN_BINDINGS] =
    [RECORD_CHUNK, BRUSH_CHUNK, STOP_CHUNK, PLACEMENT_CHUNK];

impl ArenaTables {
    fn new(mode: RunBufferMode, alignment: u64) -> Self {
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
            alignment,
            bindings,
            generations: Vec::new(),
            chunks: Vec::new(),
            staging: ArenaStaging::default(),
        }
    }

    fn place(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        tables: [&[u8]; RUN_BINDINGS],
    ) -> ArenaChunk {
        let mut rebind = false;
        for (binding, table) in self.bindings.iter_mut().zip(tables) {
            let needed = table.len() as u64;
            if self.mode.storage && needed > *binding {
                *binding = needed;
                rebind = true;
            }
        }
        let current = self.generations.last();
        let placements: [UploadPlacement; RUN_BINDINGS] = std::array::from_fn(|index| {
            place_upload(
                current.map_or(0, |generation| generation.staged[index].len() as u64),
                tables[index].len() as u64,
                self.bindings[index],
                self.alignment,
                current.map(|generation| generation.capacities[index]),
            )
        });
        let grows = placements
            .iter()
            .any(|placement| matches!(placement, UploadPlacement::Grow(_)));
        if grows {
            let capacities = std::array::from_fn(|index| {
                let least = (INITIAL_ARENA_CAPACITIES[index] * ELEMENT_SIZES[index]) as u64;
                match placements[index] {
                    UploadPlacement::Grow(capacity) => capacity.max(least),
                    UploadPlacement::At(_) => current
                        .map_or(least, |generation| generation.capacities[index])
                        .max(least),
                }
            });
            self.generations.push(ArenaGeneration::new(
                device,
                layout,
                self.mode,
                capacities,
                self.bindings,
            ));
        } else if rebind && let Some(generation) = self.generations.last_mut() {
            generation.bind_group =
                ArenaGeneration::bind(device, layout, &generation.buffers, self.bindings);
        }
        let index = self.generations.len() - 1;
        let generation = &mut self.generations[index];
        let offsets = std::array::from_fn(|table| {
            let offset = match placements[table] {
                UploadPlacement::At(offset) if !grows => offset,
                _ => 0,
            };
            let staged = &mut generation.staged[table];
            staged.resize(offset as usize, 0);
            staged.extend_from_slice(tables[table]);
            u32::try_from(offset).expect("a frame's arena tables fit a dynamic offset")
        });
        ArenaChunk {
            generation: index,
            offsets,
        }
    }

    fn flush(&mut self, queue: &wgpu::Queue) -> FrameCommandStats {
        let mut stats = FrameCommandStats::default();
        for generation in &mut self.generations {
            for (buffer, staged) in generation.buffers.iter().zip(&mut generation.staged) {
                if staged.is_empty() {
                    continue;
                }
                let padded = staged.len().div_ceil(wgpu::COPY_BUFFER_ALIGNMENT as usize)
                    * wgpu::COPY_BUFFER_ALIGNMENT as usize;
                staged.resize(padded, 0);
                stats += write_buffer(queue, buffer, 0, staged);
                staged.clear();
            }
        }
        let keep = self.generations.len().saturating_sub(1);
        self.generations.drain(..keep);
        stats
    }
}

/// The GPU home of every run: retained tables per command, and the
/// per-pass arena chunks small runs are copied into.
pub(crate) struct RunStore {
    mode: RunBufferMode,
    layout: wgpu::BindGroupLayout,
    stored: HashMap<DrawCommandId, StoredRun>,
    arena: ArenaTables,
    scratch_stops: Vec<GradientStopRecord>,
    strip_indices: [StripIndexBuffer; ARC_BUCKETS],
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
                has_dynamic_offset: true,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Run Tables Bind Group Layout"),
            entries: &[binding(0), binding(1), binding(2), binding(3)],
        });
        let limits = device.limits();
        let alignment = u64::from(if mode.storage {
            limits.min_storage_buffer_offset_alignment
        } else {
            limits.min_uniform_buffer_offset_alignment
        })
        .max(wgpu::COPY_BUFFER_ALIGNMENT);
        Self {
            mode,
            layout,
            stored: HashMap::new(),
            arena: ArenaTables::new(mode, alignment),
            fill_stats: false,
            scratch_stops: Vec::new(),
            strip_indices: Default::default(),
            frame: 0,
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
            let class = draw.key.band_class;
            self.strip_indices[class as usize].ensure(device, band_class_segments(class));
        }
    }

    /// The draws one stored run takes for one segment range: one per
    /// segment, its records in order.
    pub(crate) fn stored_run_draws(
        &mut self,
        device: &wgpu::Device,
        run: &RunDraw,
        key_for: &mut dyn FnMut(&RecordSegment) -> crate::render::ShapePipelineKey,
        out: &mut SmallVec<[RunDrawCall; 8]>,
    ) {
        for segment in run.segment_records() {
            out.push(RunDrawCall {
                key: key_for(segment),
                records: segment.start..segment.start + segment.count,
            });
        }
        self.ensure_strip_indices(device, out);
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
        self.arena.chunks.clear();
        let frame = self.frame;
        self.stored
            .retain(|_, run| frame - run.last_used_frame <= STORE_IDLE_FRAMES);
    }

    /// Writes the frame's arena tables, one write per table, ahead of the
    /// submit.
    pub(crate) fn flush(&mut self, queue: &wgpu::Queue) -> FrameCommandStats {
        self.arena.flush(queue)
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
        self.arena.staging.heap_bytes()
            + self
                .arena
                .generations
                .iter()
                .flat_map(|generation| generation.staged.iter())
                .map(Vec::capacity)
                .sum::<usize>()
    }

    pub(crate) fn stored(&self, command: &DrawCommandId) -> Option<&StoredRun> {
        self.stored.get(command)
    }

    /// The tables a closed chunk's draws bind: the frame's arena bind
    /// group and the chunk's dynamic offsets into it.
    pub(crate) fn arena_binding(&self, chunk: usize) -> ArenaBinding<'_> {
        let chunk = self.arena.chunks[chunk];
        ArenaBinding {
            bind_group: &self.arena.generations[chunk.generation].bind_group,
            offsets: chunk.offsets,
        }
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
                    STORE_PLACEMENTS,
                ],
            ),
            tables: Arc::new(RecordTables::default()),
            paint,
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
            let fresh = entry.buffers.ensure(
                device,
                layout,
                [
                    tables.shapes.len().max(1),
                    tables.brushes.len().max(1),
                    tables.stops.len().max(1),
                    STORE_PLACEMENTS,
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
            stops_changed |= fresh[2] || previous.stops != tables.stops;
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
    pub(crate) fn open_arena(&mut self) -> usize {
        let chunk = self.arena.chunks.len();
        self.arena.chunks.push(ArenaChunk::default());
        self.arena.staging.clear();
        chunk
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
            run.tables.brushes.len().min(BRUSH_CHUNK),
            run.tables.stops.len().min(STOP_CHUNK),
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
        from: u32,
        root_scale: f32,
        key_for: &mut dyn FnMut(&RecordSegment) -> crate::render::ShapePipelineKey,
    ) -> u32 {
        let mode = self.mode;
        let fill_stats = self.fill_stats;
        debug_assert_eq!(
            chunk + 1,
            self.arena.chunks.len(),
            "only the open chunk appends"
        );
        let staging = &mut self.arena.staging;
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
            let key = key_for(segment);
            let class_segments = mode
                .storage
                .then(|| band_class_segments(segment.band_class));
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
                staging.push_draw(key, record_index);
                if fill_stats {
                    staging.fill.add_record(
                        &record,
                        run.placement.offset,
                        root_scale,
                        class_segments,
                    );
                }
                taken += 1;
            }
            if !segment_complete {
                return taken;
            }
        }
        taken
    }

    /// Places the open chunk's tables in the frame's arena; returns the
    /// draws and the chunk's fill.
    pub(crate) fn close_arena(
        &mut self,
        device: &wgpu::Device,
        chunk: usize,
    ) -> (Vec<RunDrawCall>, Option<ShapeFill>) {
        debug_assert_eq!(
            chunk + 1,
            self.arena.chunks.len(),
            "only the open chunk closes"
        );
        if self.arena.staging.is_empty() {
            return (Vec::new(), None);
        }
        let mut staging = std::mem::take(&mut self.arena.staging);
        let draws = std::mem::take(&mut staging.draws);
        for draw in &draws {
            let class = draw.key.band_class;
            self.strip_indices[class as usize].ensure(device, band_class_segments(class));
        }
        let placed = self.arena.place(
            device,
            &self.layout,
            [
                bytemuck::cast_slice(&staging.records),
                bytemuck::cast_slice(&staging.brushes),
                bytemuck::cast_slice(&staging.stops),
                bytemuck::cast_slice(&staging.placements),
            ],
        );
        self.arena.chunks[chunk] = placed;
        let fill = self.fill_stats.then_some(staging.fill);
        self.arena.staging = staging;
        (draws, fill)
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
}
