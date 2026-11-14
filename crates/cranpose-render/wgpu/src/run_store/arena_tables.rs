// The frame's arena tables: the per-pass chunks small runs are copied
// into, in generations a copied arena keeps and a mapped arena cycles.

use super::{
    ArcTrigFill, ArenaChunk, ArenaGeneration, ArenaStaging, BODY_BUFFER, BRUSH_CHUNK, BUFFER_COUNT,
    CURVE_BUFFER, ELEMENT_SIZES, FrameCommandRecorder, FrameCommandStats, INITIAL_ARENA_RECORDS,
    INITIAL_BRUSHES, INITIAL_PLACEMENTS, INITIAL_STOPS, MapRequests, MappedPool, PLACEMENT_CHUNK,
    RECORD_CHUNK, RunBufferMode, STOP_CHUNK, UploadMode, UploadPlacement, align_u64_to,
    place_upload,
};
use crate::idle_pool::RecentPeak;

// A copied arena keeps its last generation from frame to frame, since the
// copies into it queue behind the draws that read it. A mapped arena writes
// each placement into a generation it keeps mapped until the frame is
// staged; its generations then cycle through a `MappedPool` while the GPU
// reads them.
pub(super) struct ArenaTables {
    mode: RunBufferMode,
    upload: UploadMode,
    alignment: u64,
    pub(super) bindings: [u64; BUFFER_COUNT],
    pub(super) generations: Vec<ArenaGeneration>,
    pub(super) chunks: Vec<ArenaChunk>,
    pub(super) staging: ArenaStaging,
    pool: MappedPool<ArenaGeneration>,
    pub(super) requests: MapRequests,
    // The bytes of each table the last mapped frame placed: a frame opens
    // in a generation that holds them.
    last_totals: [u64; BUFFER_COUNT],
    // Whether the frame's mapped generations were unmapped for its submit.
    sealed: bool,
    // The frame's mapped writes so far, reported when it is staged.
    written: FrameCommandStats,
    // The most bytes of each table a frame placed lately: a generation far
    // larger than recent frames fill goes, so the first frame of a screen,
    // which places every run, does not hold its size for the frames after.
    recent: [RecentPeak; BUFFER_COUNT],
}

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
    pub(super) fn new(mode: RunBufferMode, upload: UploadMode, alignment: u64) -> Self {
        Self {
            mode,
            upload,
            alignment,
            bindings: first_bindings(mode),
            generations: Vec::new(),
            chunks: Vec::new(),
            staging: ArenaStaging::default(),
            pool: MappedPool::default(),
            requests: MapRequests::default(),
            last_totals: [0; BUFFER_COUNT],
            sealed: false,
            written: FrameCommandStats::default(),
            recent: std::array::from_fn(|_| RecentPeak::default()),
        }
    }

    pub(super) fn place(
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

    // A generation of at least `capacities` bytes per table: a mapped arena
    // reuses a free one, which at a frame's start holds the last frame's
    // tables with room to grow, and makes one when none is free. Free
    // generations that do not hold the frame's tables go, and those far
    // larger than recent frames fill.
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
        let recent = &self.recent;
        let taken = self.pool.take(device, |free| {
            free.retain(|generation| generation.holds(wanted) && !oversized(generation, recent));
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

    pub(super) fn fill_trig(
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
            fill.dispatch(
                &mut pass,
                bind_group,
                u32::try_from(rows).unwrap_or(u32::MAX),
            );
        }
    }

    pub(super) fn stage_pending(
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
                let alignment = wgpu::COPY_BUFFER_ALIGNMENT as usize;
                let padded = staged.len().div_ceil(alignment) * alignment;
                staged.resize(padded, 0);
                stats += recorder.stage_frame_buffer_copy(device, buffer, 0, staged);
            }
        }
        stats
    }

    // Ends the frame: a mapped arena's staged generations go to the pool,
    // to map again once the frame is submitted; otherwise the last
    // generation stays for the next frame, unless it is far larger than
    // recent frames fill. A mapped generation the frame never staged is
    // still mapped and stays writable.
    pub(super) fn reset(&mut self) {
        self.written = FrameCommandStats::default();
        let first = first_bindings(self.mode);
        for (table, recent) in self.recent.iter_mut().enumerate() {
            let placed: u64 = self
                .generations
                .iter()
                .map(|generation| generation.cursors[table])
                .sum();
            recent.reach(placed);
            recent.end_frame();
            // A storage binding grows to the largest table a frame placed;
            // far above recent frames it goes back, and grows again on
            // demand, so new generations open at what frames now place.
            if far_above(self.bindings[table], recent, first[table]) {
                self.bindings[table] = first[table];
            }
        }
        if std::mem::take(&mut self.sealed) {
            for generation in self.generations.drain(..) {
                self.pool.recycle(generation, &mut self.requests);
            }
            return;
        }
        let last_oversized = self
            .generations
            .last()
            .is_some_and(|generation| oversized(generation, &self.recent));
        let kept = usize::from(!last_oversized && !self.generations.is_empty());
        let cut = self.generations.len() - kept;
        self.generations.drain(..cut);
        for generation in &mut self.generations {
            generation.clear();
        }
    }
}

// The binding sizes an arena starts with: one element a table for storage
// bindings, which grow with the tables, and the fixed windows of uniform
// ones.
fn first_bindings(mode: RunBufferMode) -> [u64; BUFFER_COUNT] {
    std::array::from_fn(|index| {
        let elements = if mode.storage {
            1
        } else {
            UNIFORM_CHUNKS[index]
        };
        (elements * ELEMENT_SIZES[index]) as u64
    })
}

fn bytes(size: u64) -> usize {
    usize::try_from(size).unwrap_or(usize::MAX)
}

// Whether `generation` holds more than four times what `recent` frames
// placed in one of its tables, past the size a table opens at.
#[inline(never)]
fn oversized(generation: &ArenaGeneration, recent: &[RecentPeak; BUFFER_COUNT]) -> bool {
    (0..BUFFER_COUNT).any(|table| {
        let least = (INITIAL_ARENA_CAPACITIES[table] * ELEMENT_SIZES[table])
            .max(bytes(crate::frame_graph::MIN_UPLOAD_BUFFER_BYTES));
        far_above(generation.capacities[table], &recent[table], least as u64)
    })
}

// Whether `size` lies above four times what `recent` frames placed, and four
// times `least`.
fn far_above(size: u64, recent: &RecentPeak, least: u64) -> bool {
    size > 4 * recent.value().max(least)
}
