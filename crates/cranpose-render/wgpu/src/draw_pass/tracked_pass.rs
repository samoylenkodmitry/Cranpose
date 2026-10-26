use crate::render::TargetRect;

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
        }
    }

    pub(crate) fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        if self.scissor != Some((x, y, width, height)) {
            self.pass.set_scissor_rect(x, y, width, height);
            self.scissor = Some((x, y, width, height));
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
        }
    }

    pub(crate) fn set_pipeline(&mut self, pipeline: &wgpu::RenderPipeline) {
        if self.pipeline.as_ref() != Some(pipeline) {
            self.pass.set_pipeline(pipeline);
            self.pipeline = Some(pipeline.clone());
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
        match tracked {
            Some(bound) => {
                if !bound
                    .as_ref()
                    .is_some_and(|bound| bound.is(bind_group, offsets))
                {
                    self.pass.set_bind_group(index, bind_group, offsets);
                    *bound = BoundGroup::new(bind_group, offsets);
                }
            }
            None => self.pass.set_bind_group(index, bind_group, offsets),
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
                }
            }
            None => self.pass.set_vertex_buffer(slot, slice),
        }
    }

    pub(crate) fn draw(&mut self, vertices: std::ops::Range<u32>, instances: std::ops::Range<u32>) {
        self.pass.draw(vertices, instances);
    }

    pub(crate) fn draw_indexed(
        &mut self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        self.pass.draw_indexed(indices, base_vertex, instances);
    }

    /// The pass, for a draw that sets what it needs itself: what it binds
    /// is then no longer known.
    pub(crate) fn untracked(&mut self) -> &mut wgpu::RenderPass<'p> {
        self.scissor = None;
        self.index = None;
        self.pipeline = None;
        self.groups = [const { None }; TRACKED_GROUPS];
        self.vertices = [const { None }; TRACKED_SLOTS];
        self.pass
    }
}
