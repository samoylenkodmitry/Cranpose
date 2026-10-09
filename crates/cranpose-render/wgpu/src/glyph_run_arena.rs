//! The GPU home of retained text glyph runs: each run's glyph instances take
//! a span of a shared instance chunk, so a run scrolling into view costs a
//! staged copy instead of a buffer of its own, and a frame's new runs reach
//! the GPU in as few writes as their spans.

use std::{cell::Cell, ops::Range, rc::Rc};

use crate::{
    frame_graph::{
        FrameCommandRecorder, FrameCommandStats, MapRequests, MappedBuffers, MappedPool,
        TEST_READBACK, UploadMode, write_mapped,
    },
    render::GlyphInstance,
};
/// The first chunk's quads; each later chunk doubles up to the largest,
/// and a run too large for that gets a chunk of its own size.
const MIN_CHUNK_QUADS: u32 = 1024;
/// The largest chunk a mapped arena opens for later runs of a frame.
const MAX_MAPPED_CHUNK_QUADS: u32 = 8192;
/// The largest chunk a copied arena opens: 128 KiB. A copied chunk lives
/// as long as its runs, in a block the GPU allocator shares with the
/// renderer's other buffers and textures. On a Mali-G76, 4 MiB blocks with
/// 2.9 MiB free held no free 512 KiB range, so each 512 KiB chunk took
/// memory of its own; 128 KiB chunks fill the ranges those blocks have.
const MAX_COPIED_CHUNK_QUADS: u32 = 2048;
/// No text run holds more glyphs; bounds the index arithmetic.
const MAX_RUN_QUADS: u32 = 1 << 20;
/// The quads a mapped chunk grows by: one 16 KiB page.
const MAPPED_CHUNK_QUADS: u32 = 256;
/// The empty mapped chunks the arena keeps for later frames.
const MAX_FREE_CHUNKS: usize = 4;

/// The free quad ranges of one chunk, sorted by start and coalesced.
#[derive(Debug)]
pub(crate) struct SpanAllocator {
    capacity: u32,
    free: Vec<Range<u32>>,
}

impl SpanAllocator {
    pub(crate) fn new(capacity: u32) -> Self {
        let mut free = Vec::new();
        if capacity > 0 {
            free.push(0..capacity);
        }
        Self { capacity, free }
    }

    pub(crate) fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Takes `len` quads from the start of the first free range that holds
    /// them.
    pub(crate) fn allocate(&mut self, len: u32) -> Option<u32> {
        if len == 0 {
            return None;
        }
        let slot = self
            .free
            .iter()
            .position(|range| range.end - range.start >= len)?;
        let range = &mut self.free[slot];
        let start = range.start;
        range.start += len;
        if range.start == range.end {
            self.free.remove(slot);
        }
        Some(start)
    }

    /// Returns `span` to the free ranges, merged with the ranges it touches.
    pub(crate) fn release(&mut self, span: Range<u32>) {
        if span.start >= span.end || span.end > self.capacity {
            return;
        }
        let slot = self.free.partition_point(|range| range.start < span.start);
        let joins_previous = slot > 0 && self.free[slot - 1].end == span.start;
        let joins_next = self
            .free
            .get(slot)
            .is_some_and(|next| next.start == span.end);
        match (joins_previous, joins_next) {
            (true, true) => {
                let end = self.free[slot].end;
                self.free[slot - 1].end = end;
                self.free.remove(slot);
            }
            (true, false) => self.free[slot - 1].end = span.end,
            (false, true) => self.free[slot].start = span.start,
            (false, false) => self.free.insert(slot, span),
        }
    }

    /// Whether no quad is allocated.
    pub(crate) fn is_unused(&self) -> bool {
        self.capacity == 0 || self.free.first() == Some(&(0..self.capacity))
    }
}

#[derive(Debug)]
struct RetiredSpan {
    chunk: u64,
    quads: Range<u32>,
}

/// Spans whose runs were dropped, returned to their chunks when the next
/// frame opens: a draw recorded this frame may still read them.
#[derive(Default)]
struct RetiredSpans(Cell<Vec<RetiredSpan>>);

impl RetiredSpans {
    fn push(&self, span: RetiredSpan) {
        let mut spans = self.0.take();
        spans.push(span);
        self.0.set(spans);
    }

    fn take(&self) -> Vec<RetiredSpan> {
        self.0.take()
    }
}

/// One run's quads in a chunk of the arena. Dropping it frees the quads
/// from the next frame on.
pub(crate) struct GlyphRunSpan {
    buffer: wgpu::Buffer,
    chunk: u64,
    quads: Range<u32>,
    retired: Rc<RetiredSpans>,
}

impl GlyphRunSpan {
    pub(crate) fn chunk(&self) -> u64 {
        self.chunk
    }

    /// The chunk holding the run's glyph instances.
    pub(crate) fn instance_buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// The run's instances in its chunk.
    pub(crate) fn instances(&self) -> Range<u32> {
        self.quads.clone()
    }
}

impl Drop for GlyphRunSpan {
    fn drop(&mut self) {
        self.retired.push(RetiredSpan {
            chunk: self.chunk,
            quads: self.quads.clone(),
        });
    }
}

struct Chunk {
    id: u64,
    buffer: wgpu::Buffer,
    spans: SpanAllocator,
    /// The quads of runs a mapped chunk still holds.
    live: u32,
    /// Whether a mapped chunk is mapped and takes new runs this frame.
    open: bool,
    evacuated: bool,
}

impl MappedBuffers for Chunk {
    fn mapped_buffers(&self) -> &[wgpu::Buffer] {
        std::slice::from_ref(&self.buffer)
    }
}

/// A span's instances waiting in the arena's staging for the frame's flush.
struct StagedSpan {
    chunk: u64,
    first_quad: u32,
    instances: Range<usize>,
}

/// Retained glyph runs in shared instance chunks.
///
/// A copied arena gives a dropped run's quads to the next runs and copies
/// the frame's new runs into their chunks ahead of its passes. A mapped
/// arena writes new runs into chunks mapped for the frame and unmaps them
/// for its submit; a chunk then only loses runs, and goes back to a pool
/// once it holds none and the GPU is done with it. Runs in a chunk that
/// fell below a quarter full move out: the renderer drops them, and they
/// are written again where they are next drawn.
pub(crate) struct GlyphRunArena {
    chunks: Vec<Chunk>,
    next_chunk: u64,
    staged_instances: Vec<GlyphInstance>,
    staged: Vec<StagedSpan>,
    retired: Rc<RetiredSpans>,
    upload: UploadMode,
    pool: MappedPool<Chunk>,
    requests: MapRequests,
    /// The quads the frame wrote so far, and those of the last frame that
    /// wrote any: a frame's first mapped chunk holds that many.
    frame_quads: u32,
    last_quads: u32,
    written: FrameCommandStats,
    evacuating: Vec<u64>,
}

impl GlyphRunArena {
    pub(crate) fn new(upload: UploadMode) -> Self {
        Self {
            chunks: Vec::new(),
            next_chunk: 0,
            staged_instances: Vec::new(),
            staged: Vec::new(),
            retired: Rc::default(),
            upload,
            pool: MappedPool::default(),
            requests: MapRequests::default(),
            frame_quads: 0,
            last_quads: 0,
            written: FrameCommandStats::default(),
            evacuating: Vec::new(),
        }
    }

    /// Places `quads` in a free span, opening a chunk when none has room:
    /// staged for the frame's flush, or written into a mapped chunk now.
    /// `None` when there are no quads or more than any text run holds.
    pub(crate) fn insert<I>(&mut self, device: &wgpu::Device, quads: I) -> Option<GlyphRunSpan>
    where
        I: IntoIterator<Item = GlyphInstance>,
    {
        let upload = self.upload;
        let start = self.staged_instances.len();
        self.staged_instances.extend(quads);
        let count = self.staged_instances.len() - start;
        let Some(count) = u32::try_from(count)
            .ok()
            .filter(|count| (1..=MAX_RUN_QUADS).contains(count))
        else {
            self.staged_instances.truncate(start);
            return None;
        };
        let (chunk, first_quad) = match self.allocate(count, upload) {
            Some(place) => place,
            None => match upload {
                UploadMode::Copied => self.open_chunk(device, count),
                UploadMode::Mapped => self.open_mapped_chunk(device, count),
            },
        };
        match upload {
            UploadMode::Copied => self.staged.push(StagedSpan {
                chunk: self.chunks[chunk].id,
                first_quad,
                instances: start..self.staged_instances.len(),
            }),
            UploadMode::Mapped => {
                self.written += write_mapped(
                    &self.chunks[chunk].buffer,
                    instance_offset(first_quad),
                    bytemuck::cast_slice(&self.staged_instances[start..]),
                );
                self.staged_instances.truncate(start);
                self.chunks[chunk].live += count;
                self.frame_quads += count;
            }
        }
        Some(GlyphRunSpan {
            buffer: self.chunks[chunk].buffer.clone(),
            chunk: self.chunks[chunk].id,
            quads: first_quad..first_quad + count,
            retired: Rc::clone(&self.retired),
        })
    }

    /// Opens a frame: spans dropped by now are free again. A copied arena
    /// releases chunks left empty, except one to hold the next runs; a
    /// mapped arena asks its empty chunks back and picks the sparse ones
    /// whose runs move out.
    pub(crate) fn begin_frame(&mut self) {
        let mapped = self.upload == UploadMode::Mapped;
        for span in self.retired.take() {
            if let Some(chunk) = self.chunks.iter_mut().find(|chunk| chunk.id == span.chunk) {
                if mapped {
                    chunk.live -= span.quads.end - span.quads.start;
                } else {
                    chunk.spans.release(span.quads);
                }
            }
        }
        if mapped {
            self.recycle_mapped();
            return;
        }
        let mut keep_empty = self.chunks.iter().all(|chunk| chunk.spans.is_unused());
        self.chunks.retain(|chunk| {
            if !chunk.spans.is_unused() {
                return true;
            }
            let keep = keep_empty && chunk.spans.capacity() <= MAX_COPIED_CHUNK_QUADS;
            keep_empty &= !keep;
            keep
        });
    }

    /// The chunks whose runs the renderer moves out this frame.
    pub(crate) fn evacuating(&self) -> &[u64] {
        &self.evacuating
    }

    /// Sends empty sealed chunks to the pool, whose maps the last submit
    /// already covers, and marks the sparse ones for evacuation: two or
    /// more of them fold into one chunk, and one alone moves when it is
    /// larger than a page.
    fn recycle_mapped(&mut self) {
        let mut index = 0;
        while index < self.chunks.len() {
            let chunk = &self.chunks[index];
            if chunk.live == 0 && !chunk.open {
                let chunk = self.chunks.swap_remove(index);
                if self.pool.len() < MAX_FREE_CHUNKS {
                    self.pool.recycle(chunk, &mut self.requests);
                }
            } else {
                index += 1;
            }
        }
        self.requests.recall();
        self.evacuating.clear();
        self.evacuating.extend(
            self.chunks
                .iter()
                .filter(|chunk| {
                    !chunk.open
                        && !chunk.evacuated
                        && u64::from(chunk.live) * 4 < u64::from(chunk.spans.capacity())
                })
                .map(|chunk| chunk.id),
        );
        if let [only] = self.evacuating[..]
            && self
                .chunks
                .iter()
                .any(|chunk| chunk.id == only && chunk.spans.capacity() <= MAPPED_CHUNK_QUADS)
        {
            self.evacuating.clear();
        }
        for chunk in &mut self.chunks {
            chunk.evacuated |= self.evacuating.contains(&chunk.id);
        }
    }

    /// Writes the frame's new runs: a copied arena's staged spans through
    /// the recorder, joined where adjacent; a mapped arena wrote them as
    /// they came and unmaps its open chunks for the submit.
    pub(crate) fn stage_pending(
        &mut self,
        device: &wgpu::Device,
        recorder: &mut impl FrameCommandRecorder,
    ) -> FrameCommandStats {
        if self.upload == UploadMode::Mapped {
            for chunk in self.chunks.iter_mut().filter(|chunk| chunk.open) {
                chunk.buffer.unmap();
                chunk.open = false;
            }
            if self.frame_quads > 0 {
                self.last_quads = std::mem::take(&mut self.frame_quads);
            }
            return std::mem::take(&mut self.written);
        }
        let mut stats = FrameCommandStats::default();
        let mut index = 0;
        while index < self.staged.len() {
            let first = &self.staged[index];
            let mut end_quad = first.first_quad + span_quads(first);
            let mut instances = first.instances.clone();
            let mut next = index + 1;
            while let Some(span) = self.staged.get(next) {
                if span.chunk != first.chunk
                    || span.first_quad != end_quad
                    || span.instances.start != instances.end
                {
                    break;
                }
                end_quad += span_quads(span);
                instances.end = span.instances.end;
                next += 1;
            }
            if let Some(chunk) = self.chunks.iter().find(|chunk| chunk.id == first.chunk) {
                stats += recorder.stage_frame_buffer_copy(
                    device,
                    &chunk.buffer,
                    instance_offset(first.first_quad),
                    bytemuck::cast_slice(&self.staged_instances[instances]),
                );
            }
            index = next;
        }
        // A screen's first frame stages every run it draws, later frames
        // the few that change: room for four times this frame's quads is
        // given back.
        let room = self.staged_instances.len().max(MIN_CHUNK_QUADS as usize);
        self.staged.clear();
        self.staged_instances.clear();
        if self.staged_instances.capacity() > 4 * room {
            self.staged_instances.shrink_to(room);
        }
        stats
    }

    /// A span in a chunk that takes runs: any in a copied arena, an open
    /// one in a mapped arena.
    fn allocate(&mut self, quads: u32, upload: UploadMode) -> Option<(usize, u32)> {
        self.chunks
            .iter_mut()
            .enumerate()
            .filter(|(_, chunk)| upload == UploadMode::Copied || chunk.open)
            .find_map(|(slot, chunk)| chunk.spans.allocate(quads).map(|first| (slot, first)))
    }

    /// Opens a mapped chunk for `quads`: the frame's first holds what the
    /// last frame wrote, a later one twice the frame's largest; a free one
    /// that holds them is reused.
    fn open_mapped_chunk(&mut self, device: &wgpu::Device, quads: u32) -> (usize, u32) {
        let largest_open = self
            .chunks
            .iter()
            .filter(|chunk| chunk.open)
            .map(|chunk| chunk.spans.capacity())
            .max();
        let hint = match largest_open {
            None => self.last_quads + self.last_quads / 4,
            Some(largest) => largest.saturating_mul(2).min(MAX_MAPPED_CHUNK_QUADS),
        };
        let wanted = quads
            .max(hint)
            .next_multiple_of(MAPPED_CHUNK_QUADS)
            .max(MAPPED_CHUNK_QUADS);
        let id = self.next_chunk;
        self.next_chunk += 1;
        let taken = self.pool.take(device, |free| {
            free.retain(|chunk| chunk.spans.capacity() >= wanted / 4);
            let slot = free
                .iter()
                .position(|chunk| (quads..=wanted * 2).contains(&chunk.spans.capacity()))?;
            Some(free.swap_remove(slot))
        });
        let chunk = match taken {
            Some(chunk) => Chunk {
                id,
                spans: SpanAllocator::new(chunk.spans.capacity()),
                live: 0,
                open: true,
                evacuated: false,
                buffer: chunk.buffer,
            },
            None => Chunk {
                id,
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Retained Text Glyph Instances"),
                    size: instance_offset(wanted),
                    usage: UploadMode::Mapped.writable(wgpu::BufferUsages::VERTEX | TEST_READBACK),
                    mapped_at_creation: true,
                }),
                spans: SpanAllocator::new(wanted),
                live: 0,
                open: true,
                evacuated: false,
            },
        };
        self.chunks.push(chunk);
        let slot = self.chunks.len() - 1;
        let first_quad = self.chunks[slot].spans.allocate(quads).unwrap_or_default();
        (slot, first_quad)
    }

    fn open_chunk(&mut self, device: &wgpu::Device, quads: u32) -> (usize, u32) {
        let largest = self
            .chunks
            .iter()
            .map(|chunk| chunk.spans.capacity())
            .filter(|capacity| *capacity <= MAX_COPIED_CHUNK_QUADS)
            .max();
        let capacity = largest
            .map_or(MIN_CHUNK_QUADS, |largest| {
                largest.saturating_mul(2).min(MAX_COPIED_CHUNK_QUADS)
            })
            .max(quads);
        let mut spans = SpanAllocator::new(capacity);
        let first_quad = spans.allocate(quads).unwrap_or_default();
        self.chunks.push(Chunk {
            id: self.next_chunk,
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Retained Text Glyph Instances"),
                size: instance_offset(capacity),
                usage: UploadMode::Copied.writable(wgpu::BufferUsages::VERTEX),
                mapped_at_creation: false,
            }),
            spans,
            live: 0,
            open: false,
            evacuated: false,
        });
        self.next_chunk += 1;
        (self.chunks.len() - 1, first_quad)
    }
}

fn span_quads(span: &StagedSpan) -> u32 {
    (span.instances.end - span.instances.start) as u32
}

fn instance_offset(quads: u32) -> u64 {
    u64::from(quads) * std::mem::size_of::<GlyphInstance>() as u64
}

#[cfg(test)]
#[path = "tests/glyph_run_arena_tests.rs"]
mod tests;
