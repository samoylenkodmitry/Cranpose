use crate::{gpu_stats::BatchBreaks, render::TargetRect};

// What a pass's batch draws: a draw of another kind than the draw before it
// breaks the batch because the z order interleaves the kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawKind {
    Shapes,
    Images,
    Glyphs,
    Shadows,
    Layers,
    StandIns,
}

// The state a set call changed since the pass's last draw.
const CHANGED_PIPELINE: u8 = 1;
const CHANGED_GROUP: u8 = 1 << 1;
const CHANGED_SCISSOR: u8 = 1 << 2;
const CHANGED_UNIFORM: u8 = 1 << 3;
const CHANGED_BUFFERS: u8 = 1 << 4;

// Why each draw of a pass breaks from the draw before it, counted while the
// GPU stats are on.
#[derive(Default)]
struct BreakCount {
    breaks: BatchBreaks,
    changed: u8,
    kind: Option<DrawKind>,
    last_kind: Option<DrawKind>,
}

impl BreakCount {
    fn draw(&mut self) {
        let kind = self.kind;
        let counter = match (self.last_kind, kind) {
            (None, _) => &mut self.breaks.first,
            (Some(last), Some(kind)) if last != kind => &mut self.breaks.kind,
            _ if self.changed & CHANGED_PIPELINE != 0 => &mut self.breaks.pipeline,
            _ if self.changed & CHANGED_GROUP != 0 => &mut self.breaks.bind_group,
            _ if self.changed & CHANGED_SCISSOR != 0 => &mut self.breaks.scissor,
            _ if self.changed & CHANGED_UNIFORM != 0 => &mut self.breaks.uniform,
            _ if self.changed & CHANGED_BUFFERS != 0 => &mut self.breaks.buffers,
            _ => &mut self.breaks.unchanged,
        };
        *counter = counter.saturating_add(1);
        self.changed = 0;
        self.last_kind = kind.or(self.last_kind);
    }

    fn untracked_draw(&mut self) {
        let counter = match (self.last_kind, self.kind) {
            (None, _) => &mut self.breaks.first,
            (Some(last), Some(kind)) if last != kind => &mut self.breaks.kind,
            _ => &mut self.breaks.untracked,
        };
        *counter = counter.saturating_add(1);
        self.changed = 0;
        self.last_kind = self.kind.or(self.last_kind);
    }
}

/// A render pass that leaves out the set calls that change nothing: wgpu
/// records and validates every set call again, and on the web each one
/// crosses into JavaScript. Consecutive batches often bind the same strip
/// indices and viewport uniform under the same scissor, and a run batch's
/// draws often share a pipeline.
pub(crate) struct TrackedPass<'a, 'p> {
    pass: &'a mut wgpu::RenderPass<'p>,
    scissor: Option<TargetRect>,
    /// The bound index buffer's address, offset, size and index format.
    index: Option<(
        usize,
        wgpu::BufferAddress,
        wgpu::BufferAddress,
        wgpu::IndexFormat,
    )>,
    pipeline: Option<wgpu::RenderPipeline>,
    /// Bind groups 0 and 1 with their dynamic offsets.
    groups: [Option<BoundGroup>; TRACKED_GROUPS],
    /// Vertex buffer slots 0 and 1: buffer, offset and size.
    vertices: [Option<(wgpu::Buffer, wgpu::BufferAddress, wgpu::BufferAddress)>; TRACKED_SLOTS],
    breaks: Option<BreakCount>,
}

const TRACKED_GROUPS: usize = 2;
const TRACKED_SLOTS: usize = 2;
const TRACKED_OFFSETS: usize = 4;

#[derive(PartialEq)]
struct BoundGroup {
    group: wgpu::BindGroup,
    offsets: [wgpu::DynamicOffset; TRACKED_OFFSETS],
    offset_count: usize,
}

impl BoundGroup {
    /// The binding, when its offsets fit what is tracked.
    fn new(group: &wgpu::BindGroup, offsets: &[wgpu::DynamicOffset]) -> Option<Self> {
        let mut tracked = [0; TRACKED_OFFSETS];
        tracked.get_mut(..offsets.len())?.copy_from_slice(offsets);
        Some(Self {
            group: group.clone(),
            offsets: tracked,
            offset_count: offsets.len(),
        })
    }

    fn is(&self, group: &wgpu::BindGroup, offsets: &[wgpu::DynamicOffset]) -> bool {
        self.group == *group && self.offsets.get(..self.offset_count) == Some(offsets)
    }
}

impl<'a, 'p> TrackedPass<'a, 'p> {
    pub(crate) fn new(pass: &'a mut wgpu::RenderPass<'p>) -> Self {
        Self {
            pass,
            scissor: None,
            index: None,
            pipeline: None,
            groups: [const { None }; TRACKED_GROUPS],
            vertices: [const { None }; TRACKED_SLOTS],
            breaks: crate::gpu_stats::gpu_stats_enabled().then(BreakCount::default),
        }
    }

    // Notes the kind the next draws are of.
    pub(crate) fn begin_kind(&mut self, kind: DrawKind) {
        if let Some(breaks) = &mut self.breaks {
            breaks.kind = Some(kind);
        }
    }

    // Why the pass's draws broke from the draws before them, while the GPU
    // stats are on.
    pub(crate) fn breaks(&self) -> Option<BatchBreaks> {
        self.breaks.as_ref().map(|count| count.breaks)
    }

    fn changed(&mut self, state: u8) {
        if let Some(breaks) = &mut self.breaks {
            breaks.changed |= state;
        }
    }

    pub(crate) fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        if self.scissor != Some((x, y, width, height)) {
            self.pass.set_scissor_rect(x, y, width, height);
            self.scissor = Some((x, y, width, height));
            self.changed(CHANGED_SCISSOR);
        }
    }

    /// Binds `slice` as `u32` indices.
    pub(crate) fn set_index_buffer(
        &mut self,
        slice: wgpu::BufferSlice<'_>,
        format: wgpu::IndexFormat,
    ) {
        let bound = (
            std::ptr::from_ref(slice.buffer()).addr(),
            slice.offset(),
            slice.size(),
            format,
        );
        if self.index != Some(bound) {
            self.pass.set_index_buffer(slice, format);
            self.index = Some(bound);
            self.changed(CHANGED_BUFFERS);
        }
    }

    pub(crate) fn set_pipeline(&mut self, pipeline: &wgpu::RenderPipeline) {
        if self.pipeline.as_ref() != Some(pipeline) {
            self.pass.set_pipeline(pipeline);
            self.pipeline = Some(pipeline.clone());
            self.changed(CHANGED_PIPELINE);
        }
    }

    pub(crate) fn set_bind_group(
        &mut self,
        index: u32,
        bind_group: &wgpu::BindGroup,
        offsets: &[wgpu::DynamicOffset],
    ) {
        let tracked = usize::try_from(index)
            .ok()
            .and_then(|index| self.groups.get_mut(index));
        let changed = match tracked {
            Some(bound) => {
                let changed = !bound
                    .as_ref()
                    .is_some_and(|bound| bound.is(bind_group, offsets));
                if changed {
                    self.pass.set_bind_group(index, bind_group, offsets);
                    *bound = BoundGroup::new(bind_group, offsets);
                }
                changed
            }
            None => {
                self.pass.set_bind_group(index, bind_group, offsets);
                true
            }
        };
        if changed {
            self.changed(if index == 0 {
                CHANGED_UNIFORM
            } else {
                CHANGED_GROUP
            });
        }
    }

    pub(crate) fn set_vertex_buffer(&mut self, slot: u32, slice: wgpu::BufferSlice<'_>) {
        let bound = (slice.buffer(), slice.offset(), slice.size());
        let tracked = usize::try_from(slot)
            .ok()
            .and_then(|slot| self.vertices.get_mut(slot));
        match tracked {
            Some(tracked) => {
                if tracked
                    .as_ref()
                    .map(|(buffer, offset, size)| (buffer, *offset, *size))
                    != Some(bound)
                {
                    self.pass.set_vertex_buffer(slot, slice);
                    *tracked = Some((bound.0.clone(), bound.1, bound.2));
                    self.changed(CHANGED_BUFFERS);
                }
            }
            None => {
                self.pass.set_vertex_buffer(slot, slice);
                self.changed(CHANGED_BUFFERS);
            }
        }
    }

    pub(crate) fn draw(&mut self, vertices: std::ops::Range<u32>, instances: std::ops::Range<u32>) {
        self.pass.draw(vertices, instances);
        if let Some(breaks) = &mut self.breaks {
            breaks.draw();
        }
    }

    pub(crate) fn draw_indexed(
        &mut self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        self.pass.draw_indexed(indices, base_vertex, instances);
        if let Some(breaks) = &mut self.breaks {
            breaks.draw();
        }
    }

    /// The pass, for a draw that sets what it needs itself: what it binds
    /// is then no longer known.
    pub(crate) fn untracked(&mut self) -> &mut wgpu::RenderPass<'p> {
        if let Some(breaks) = &mut self.breaks {
            breaks.untracked_draw();
        }
        self.scissor = None;
        self.index = None;
        self.pipeline = None;
        self.groups = [const { None }; TRACKED_GROUPS];
        self.vertices = [const { None }; TRACKED_SLOTS];
        self.pass
    }
}
