//! The GPU home of retained text glyph runs: each run's quads take a span of
//! a shared vertex chunk and draw through one quad index pattern, so a run
//! scrolling into view costs a staged copy instead of buffers of its own,
//! and a frame's new runs reach the GPU in as few writes as their spans.

use std::{cell::Cell, ops::Range, rc::Rc};

use crate::{
    frame_graph::{FrameCommandStats, write_buffer},
    render::Vertex,
};

const VERTICES_PER_QUAD: usize = 4;
const INDICES_PER_QUAD: u32 = 6;
/// The first chunk's quads; each later chunk doubles up to the largest,
/// and a run too large for that gets a chunk of its own size.
const MIN_CHUNK_QUADS: u32 = 1024;
const MAX_CHUNK_QUADS: u32 = 8192;
/// No text run holds more glyphs; bounds the index arithmetic.
const MAX_RUN_QUADS: u32 = 1 << 20;

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
    /// The chunk holding the run's vertices.
    pub(crate) fn vertex_buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// The run's indices in the arena's quad index buffer.
    pub(crate) fn indices(&self) -> Range<u32> {
        self.quads.start * INDICES_PER_QUAD..self.quads.end * INDICES_PER_QUAD
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
}

/// A span's vertices waiting in the arena's staging for the frame's flush.
struct StagedSpan {
    chunk: u64,
    first_quad: u32,
    vertices: Range<usize>,
}

/// Retained glyph runs in shared vertex chunks, with the index pattern
/// every run draws its quads through.
#[derive(Default)]
pub(crate) struct GlyphRunArena {
    chunks: Vec<Chunk>,
    next_chunk: u64,
    indices: Option<wgpu::Buffer>,
    index_quads: u32,
    staged_vertices: Vec<Vertex>,
    staged: Vec<StagedSpan>,
    retired: Rc<RetiredSpans>,
}

impl GlyphRunArena {
    /// Stages `quads` into a free span, opening a chunk when none has room.
    /// `None` when there are no quads or more than any text run holds.
    pub(crate) fn insert<I>(&mut self, device: &wgpu::Device, quads: I) -> Option<GlyphRunSpan>
    where
        I: IntoIterator<Item = [Vertex; VERTICES_PER_QUAD]>,
    {
        let start = self.staged_vertices.len();
        for quad in quads {
            self.staged_vertices.extend_from_slice(&quad);
        }
        let count = (self.staged_vertices.len() - start) / VERTICES_PER_QUAD;
        let Some(count) = u32::try_from(count)
            .ok()
            .filter(|count| (1..=MAX_RUN_QUADS).contains(count))
        else {
            self.staged_vertices.truncate(start);
            return None;
        };
        let (chunk, first_quad) = match self.allocate(count) {
            Some(place) => place,
            None => self.open_chunk(device, count),
        };
        self.staged.push(StagedSpan {
            chunk: self.chunks[chunk].id,
            first_quad,
            vertices: start..self.staged_vertices.len(),
        });
        Some(GlyphRunSpan {
            buffer: self.chunks[chunk].buffer.clone(),
            chunk: self.chunks[chunk].id,
            quads: first_quad..first_quad + count,
            retired: Rc::clone(&self.retired),
        })
    }

    /// The quad index pattern every span's `indices` range reads.
    pub(crate) fn index_buffer(&self) -> Option<&wgpu::Buffer> {
        self.indices.as_ref()
    }

    /// Opens a frame: spans dropped by now are free again, and chunks left
    /// empty are released, except one to hold the next runs.
    pub(crate) fn begin_frame(&mut self) {
        for span in self.retired.take() {
            if let Some(chunk) = self.chunks.iter_mut().find(|chunk| chunk.id == span.chunk) {
                chunk.spans.release(span.quads);
            }
        }
        let mut keep_empty = self.chunks.iter().all(|chunk| chunk.spans.is_unused());
        self.chunks.retain(|chunk| {
            if !chunk.spans.is_unused() {
                return true;
            }
            let keep = keep_empty && chunk.spans.capacity() <= MAX_CHUNK_QUADS;
            keep_empty &= !keep;
            keep
        });
    }

    /// Writes the staged spans ahead of the submit: one write per stretch of
    /// spans that sit back to back in a chunk and in the staging.
    pub(crate) fn flush(&mut self, queue: &wgpu::Queue) -> FrameCommandStats {
        let mut stats = FrameCommandStats::default();
        let mut index = 0;
        while index < self.staged.len() {
            let first = &self.staged[index];
            let mut end_quad = first.first_quad + span_quads(first);
            let mut vertices = first.vertices.clone();
            let mut next = index + 1;
            while let Some(span) = self.staged.get(next) {
                if span.chunk != first.chunk
                    || span.first_quad != end_quad
                    || span.vertices.start != vertices.end
                {
                    break;
                }
                end_quad += span_quads(span);
                vertices.end = span.vertices.end;
                next += 1;
            }
            if let Some(chunk) = self.chunks.iter().find(|chunk| chunk.id == first.chunk) {
                stats += write_buffer(
                    queue,
                    &chunk.buffer,
                    vertex_offset(first.first_quad),
                    bytemuck::cast_slice(&self.staged_vertices[vertices]),
                );
            }
            index = next;
        }
        self.staged.clear();
        self.staged_vertices.clear();
        stats
    }

    fn allocate(&mut self, quads: u32) -> Option<(usize, u32)> {
        self.chunks
            .iter_mut()
            .enumerate()
            .find_map(|(slot, chunk)| chunk.spans.allocate(quads).map(|first| (slot, first)))
    }

    fn open_chunk(&mut self, device: &wgpu::Device, quads: u32) -> (usize, u32) {
        let largest = self
            .chunks
            .iter()
            .map(|chunk| chunk.spans.capacity())
            .filter(|capacity| *capacity <= MAX_CHUNK_QUADS)
            .max();
        let capacity = largest
            .map_or(MIN_CHUNK_QUADS, |largest| {
                largest.saturating_mul(2).min(MAX_CHUNK_QUADS)
            })
            .max(quads);
        self.ensure_indices(device, capacity);
        let mut spans = SpanAllocator::new(capacity);
        let first_quad = spans.allocate(quads).unwrap_or_default();
        self.chunks.push(Chunk {
            id: self.next_chunk,
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Retained Text Glyph Vertices"),
                size: vertex_offset(capacity),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            spans,
        });
        self.next_chunk += 1;
        (self.chunks.len() - 1, first_quad)
    }

    fn ensure_indices(&mut self, device: &wgpu::Device, quads: u32) {
        if self.indices.is_some() && self.index_quads >= quads {
            return;
        }
        let pattern: Vec<u32> = (0..quads).flat_map(quad_indices).collect();
        self.indices = Some(wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Retained Text Glyph Quad Indices"),
                contents: bytemuck::cast_slice(&pattern),
                usage: wgpu::BufferUsages::INDEX,
            },
        ));
        self.index_quads = quads;
    }
}

/// The two triangles of quad `quad`: corners top-left, top-right,
/// bottom-left, bottom-right.
pub(crate) fn quad_indices(quad: u32) -> [u32; INDICES_PER_QUAD as usize] {
    let base = quad * VERTICES_PER_QUAD as u32;
    [base, base + 1, base + 2, base + 2, base + 1, base + 3]
}

fn span_quads(span: &StagedSpan) -> u32 {
    ((span.vertices.end - span.vertices.start) / VERTICES_PER_QUAD) as u32
}

fn vertex_offset(quads: u32) -> u64 {
    u64::from(quads) * (VERTICES_PER_QUAD * std::mem::size_of::<Vertex>()) as u64
}

#[cfg(test)]
#[path = "tests/glyph_run_arena_tests.rs"]
mod tests;
