//! The GPU home of retained text glyph runs: each run's glyph instances take
//! a span of one shared instance buffer, so a run scrolling into view costs a
//! staged copy instead of a buffer of its own, a frame's new runs reach the
//! GPU in as few writes as their spans, and draws of any runs merge.

use std::{cell::Cell, ops::Range, rc::Rc};

use crate::{
    frame_graph::{FrameCommandRecorder, FrameCommandStats},
    render::GlyphInstance,
};
/// The fewest quads the buffer holds, and the step its size is rounded up
/// to.
const MIN_QUADS: u32 = 1024;
/// No text run holds more glyphs; bounds the index arithmetic.
const MAX_RUN_QUADS: u32 = 1 << 20;

/// The free quad ranges of the buffer, sorted by start and coalesced.
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

    /// The end of the last allocated quad: every quad past it is free.
    pub(crate) fn used_end(&self) -> u32 {
        match self.free.last() {
            Some(last) if last.end == self.capacity => last.start,
            _ => self.capacity,
        }
    }

    /// Makes the allocator `capacity` quads long, adding free quads at the
    /// end or taking free ones off it; never below [`Self::used_end`].
    pub(crate) fn resize(&mut self, capacity: u32) {
        let capacity = capacity.max(self.used_end());
        if self
            .free
            .last()
            .is_some_and(|last| last.end == self.capacity)
        {
            self.free.pop_if(|last| last.start == capacity);
            if let Some(last) = self.free.last_mut()
                && last.end == self.capacity
            {
                last.end = capacity;
            }
        } else if capacity > self.capacity {
            self.free.push(self.capacity..capacity);
        }
        self.capacity = capacity;
    }
}

/// Spans whose runs were dropped, returned to the buffer when the next frame
/// opens: a draw recorded this frame may still read them.
#[derive(Default)]
struct RetiredSpans(Cell<Vec<Range<u32>>>);

impl RetiredSpans {
    fn push(&self, span: Range<u32>) {
        let mut spans = self.0.take();
        spans.push(span);
        self.0.set(spans);
    }

    fn take(&self) -> Vec<Range<u32>> {
        self.0.take()
    }
}

/// The binding a pulled glyph pipeline reads the arena's instances through:
/// one storage buffer the vertex stage reads.
pub(crate) fn binding_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Retained Glyph Arena Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// One run's quads in the arena. Dropping it frees the quads from the next
/// frame on.
pub(crate) struct GlyphRunSpan {
    quads: Range<u32>,
    retired: Rc<RetiredSpans>,
}

impl GlyphRunSpan {
    /// The run's instances in the arena's buffer.
    pub(crate) fn instances(&self) -> Range<u32> {
        self.quads.clone()
    }
}

impl Drop for GlyphRunSpan {
    fn drop(&mut self) {
        self.retired.push(self.quads.clone());
    }
}

/// The arena's instance buffer and, in an arena that pulls, its binding.
struct Store {
    buffer: wgpu::Buffer,
    binding: Option<Rc<wgpu::BindGroup>>,
}

/// A buffer the arena left this frame. Spans staged before it was left are
/// written to it, then its first `quads` move into the buffer after it, so a
/// pass that drew from it before the move reads what it drew.
struct LeftStore {
    buffer: wgpu::Buffer,
    quads: u32,
    staged: usize,
}

/// A span's instances waiting in the arena's staging for the frame's flush.
struct StagedSpan {
    first_quad: u32,
    instances: Range<usize>,
}

/// Retained glyph runs in one shared instance buffer.
pub(crate) struct GlyphRunArena {
    /// Where pulled glyph pipelines read the buffer, the layout they bind
    /// it through.
    binding_layout: Option<wgpu::BindGroupLayout>,
    /// The most quads the device lets the buffer hold.
    max_quads: u32,
    store: Option<Store>,
    spans: SpanAllocator,
    left: Vec<LeftStore>,
    staged_instances: Vec<GlyphInstance>,
    staged: Vec<StagedSpan>,
    retired: Rc<RetiredSpans>,
}

impl GlyphRunArena {
    /// An arena on `device` whose buffer pulled glyph pipelines read through
    /// `binding_layout`, when there is one.
    pub(crate) fn new(
        device: &wgpu::Device,
        binding_layout: Option<wgpu::BindGroupLayout>,
    ) -> Self {
        let limits = device.limits();
        let bytes = if binding_layout.is_some() {
            limits
                .max_buffer_size
                .min(limits.max_storage_buffer_binding_size)
        } else {
            limits.max_buffer_size
        };
        let max_quads = bytes / std::mem::size_of::<GlyphInstance>() as u64;
        Self::empty(binding_layout, u32::try_from(max_quads).unwrap_or(u32::MAX))
    }

    /// An empty arena whose buffer binds as this one's does.
    pub(crate) fn emptied(&self) -> Self {
        Self::empty(self.binding_layout.clone(), self.max_quads)
    }

    fn empty(binding_layout: Option<wgpu::BindGroupLayout>, max_quads: u32) -> Self {
        Self {
            binding_layout,
            max_quads,
            store: None,
            spans: SpanAllocator::new(0),
            left: Vec::new(),
            staged_instances: Vec::new(),
            staged: Vec::new(),
            retired: Rc::default(),
        }
    }

    /// The buffer holding every run's glyph instances, once a run was
    /// inserted.
    pub(crate) fn instance_buffer(&self) -> Option<&wgpu::Buffer> {
        self.store.as_ref().map(|store| &store.buffer)
    }

    /// The buffer bound for pulled glyph pipelines, in an arena that pulls.
    pub(crate) fn binding(&self) -> Option<&Rc<wgpu::BindGroup>> {
        self.store.as_ref()?.binding.as_ref()
    }

    /// Stages `quads` into a free span, growing the buffer when none has
    /// room. `None` when there are no quads, more than any text run holds,
    /// or more than the device lets the buffer hold.
    pub(crate) fn insert<I>(&mut self, device: &wgpu::Device, quads: I) -> Option<GlyphRunSpan>
    where
        I: IntoIterator<Item = GlyphInstance>,
    {
        let start = self.staged_instances.len();
        self.staged_instances.extend(quads);
        let count = self.staged_instances.len() - start;
        let first_quad = u32::try_from(count)
            .ok()
            .filter(|count| (1..=MAX_RUN_QUADS).contains(count))
            .and_then(|count| self.allocate(device, count));
        let Some(first_quad) = first_quad else {
            self.staged_instances.truncate(start);
            return None;
        };
        self.staged.push(StagedSpan {
            first_quad,
            instances: start..self.staged_instances.len(),
        });
        Some(GlyphRunSpan {
            quads: first_quad..first_quad + count as u32,
            retired: Rc::clone(&self.retired),
        })
    }

    /// Opens a frame: spans dropped by now are free again, and the buffer
    /// takes the room that the quads in use or `demand`, the quads of the
    /// runs the last frame drew, need once they no longer fit, so a
    /// screen's runs fill one buffer instead of each that a frame outgrows.
    /// It shrinks to that room once it holds twice that.
    pub(crate) fn begin_frame(&mut self, device: &wgpu::Device, demand: u32) {
        for span in self.retired.take() {
            self.spans.release(span);
        }
        let needed = self.spans.used_end().max(demand);
        if needed == 0 && self.store.is_none() {
            return;
        }
        let room = room_for(needed).min(self.max_quads);
        let capacity = self.spans.capacity();
        if needed > capacity || room.saturating_mul(2) < capacity {
            self.move_to(device, room);
        }
    }

    pub(crate) fn stage_pending(
        &mut self,
        device: &wgpu::Device,
        recorder: &mut impl FrameCommandRecorder,
    ) -> FrameCommandStats {
        let mut stats = FrameCommandStats::default();
        let mut first = 0;
        for (index, left) in self.left.iter().enumerate() {
            stats += write_spans(
                device,
                recorder,
                &left.buffer,
                &self.staged[first..left.staged],
                &self.staged_instances,
            );
            first = left.staged;
            let next = match self.left.get(index + 1) {
                Some(next) => Some(&next.buffer),
                None => self.store.as_ref().map(|store| &store.buffer),
            };
            if let Some(next) = next {
                recorder.copy_frame_buffer(device, &left.buffer, next, instance_offset(left.quads));
            }
        }
        if let Some(store) = &self.store {
            stats += write_spans(
                device,
                recorder,
                &store.buffer,
                &self.staged[first..],
                &self.staged_instances,
            );
        }
        // A screen's first frame stages every run it draws, later frames
        // the few that change: room for four times this frame's quads is
        // given back.
        let room = self.staged_instances.len().max(MIN_QUADS as usize);
        self.left.clear();
        self.staged.clear();
        self.staged_instances.clear();
        if self.staged_instances.capacity() > 4 * room {
            self.staged_instances.shrink_to(room);
        }
        stats
    }

    /// Takes `quads` from the buffer, growing it to twice its size or the
    /// room they need, whichever is more, when they do not fit.
    fn allocate(&mut self, device: &wgpu::Device, quads: u32) -> Option<u32> {
        if let Some(first) = self.spans.allocate(quads) {
            return Some(first);
        }
        let needed = self.spans.used_end().checked_add(quads)?;
        let capacity = room_for(needed)
            .max(self.spans.capacity().saturating_mul(2))
            .min(self.max_quads);
        if capacity < needed {
            return None;
        }
        self.move_to(device, capacity);
        self.spans.allocate(quads)
    }

    /// Replaces the buffer with one of `capacity` quads, into which the
    /// quads in use move when the frame's uploads are staged.
    fn move_to(&mut self, device: &wgpu::Device, capacity: u32) {
        let used = self.spans.used_end();
        self.spans.resize(capacity);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Retained Text Glyph Instances"),
            size: instance_offset(self.spans.capacity()),
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC
                | self
                    .binding_layout
                    .as_ref()
                    .map_or(wgpu::BufferUsages::empty(), |_| wgpu::BufferUsages::STORAGE),
            mapped_at_creation: false,
        });
        let binding = self.binding_layout.as_ref().map(|layout| {
            Rc::new(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Retained Glyph Arena"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            }))
        });
        if let Some(store) = self.store.replace(Store { buffer, binding })
            && used > 0
        {
            self.left.push(LeftStore {
                buffer: store.buffer,
                quads: used,
                staged: self.staged.len(),
            });
        }
    }
}

/// Writes `spans` of `instances` into `buffer`, spans that follow each
/// other in both as one write.
fn write_spans(
    device: &wgpu::Device,
    recorder: &mut impl FrameCommandRecorder,
    buffer: &wgpu::Buffer,
    spans: &[StagedSpan],
    instances: &[GlyphInstance],
) -> FrameCommandStats {
    let mut stats = FrameCommandStats::default();
    let mut index = 0;
    while let Some(first) = spans.get(index) {
        let mut end_quad = first.first_quad + span_quads(first);
        let mut run = first.instances.clone();
        let mut next = index + 1;
        while let Some(span) = spans.get(next) {
            if span.first_quad != end_quad || span.instances.start != run.end {
                break;
            }
            end_quad += span_quads(span);
            run.end = span.instances.end;
            next += 1;
        }
        stats += recorder.stage_frame_buffer_copy(
            device,
            buffer,
            instance_offset(first.first_quad),
            bytemuck::cast_slice(&instances[run]),
        );
        index = next;
    }
    stats
}

/// The quads a buffer for `quads` holds: a quarter more, in whole steps of
/// [`MIN_QUADS`].
fn room_for(quads: u32) -> u32 {
    quads
        .saturating_add(quads / 4)
        .max(MIN_QUADS)
        .checked_next_multiple_of(MIN_QUADS)
        .unwrap_or(u32::MAX)
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
