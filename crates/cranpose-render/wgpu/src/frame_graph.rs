use crate::gpu_timing::{GpuFrameTimer, GpuSpanId, GpuSpanKind};
use crate::offscreen::OffscreenTarget;
use std::cell::{Cell, OnceCell};
use std::fmt;
use web_time::Instant;

#[derive(Default)]
pub(crate) struct WgpuFrameGraphExecutor {
    transient_textures: TransientTexturePool,
    upload_allocators: FrameUploadAllocators,
    gpu_timer: GpuFrameTimer,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameCommandStats {
    pub(crate) encoder_count: u32,
    pub(crate) submit_count: u32,
    pub(crate) pass_count: u32,
    pub(crate) transient_texture_bytes: u64,
    pub(crate) retained_texture_bytes: u64,
    pub(crate) upload_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct FrameGraphExecution {
    pub(crate) submission: wgpu::SubmissionIndex,
    pub(crate) stats: FrameCommandStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FrameGraphError {
    EmptyGraph,
    NoDeclaredPasses,
    ScheduledPassTwice {
        pass_index: usize,
    },
    CyclicPassDependencies {
        scheduled: usize,
        total: usize,
    },
    PassFailed {
        pass_index: usize,
        label: Option<&'static str>,
        message: String,
    },
}

impl fmt::Display for FrameGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGraph => f.write_str("frame graph contains no passes"),
            Self::NoDeclaredPasses => f.write_str("frame graph declares no WGPU passes"),
            Self::ScheduledPassTwice { pass_index } => {
                write!(f, "frame graph scheduled pass {pass_index} more than once")
            }
            Self::CyclicPassDependencies { scheduled, total } => write!(
                f,
                "frame graph scheduled {scheduled} of {total} passes; dependency cycle detected"
            ),
            Self::PassFailed {
                pass_index,
                label: Some(label),
                message,
            } => write!(
                f,
                "frame graph pass {pass_index} ({label}) failed: {message}"
            ),
            Self::PassFailed {
                pass_index,
                label: None,
                message,
            } => write!(f, "frame graph pass {pass_index} failed: {message}"),
        }
    }
}

impl std::error::Error for FrameGraphError {}

pub(crate) struct WgpuFrameGraph<'graph> {
    label: Option<&'static str>,
    passes: Vec<PassNode<'graph>>,
    resources: ResourceGraph,
}

type PassEncodeResult = Result<(), String>;

impl<'graph> WgpuFrameGraph<'graph> {
    pub(crate) fn new(label: Option<&'static str>) -> Self {
        Self {
            label,
            passes: Vec::new(),
            resources: ResourceGraph::default(),
        }
    }

    pub(crate) fn import_surface(&mut self, label: &'static str) -> TextureHandle {
        self.resources.import_texture(label)
    }

    pub(crate) fn add_fallible_command_pass(
        &mut self,
        label: Option<&'static str>,
        reads: &[TextureHandle],
        writes: &[TextureHandle],
        encode: impl for<'pass> FnOnce(&mut PassContext<'pass>) -> PassEncodeResult + 'graph,
    ) {
        self.add_command_pass_with_count(label, reads, writes, 1, encode);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn add_fallible_recorded_command_pass(
        &mut self,
        label: Option<&'static str>,
        reads: &[TextureHandle],
        writes: &[TextureHandle],
        encode: impl for<'pass> FnOnce(&mut PassContext<'pass>) -> PassEncodeResult + 'graph,
    ) {
        self.add_command_pass_with_count(label, reads, writes, 0, encode);
    }

    fn add_command_pass_with_count(
        &mut self,
        label: Option<&'static str>,
        reads: &[TextureHandle],
        writes: &[TextureHandle],
        declared_pass_count: u32,
        encode: impl for<'pass> FnOnce(&mut PassContext<'pass>) -> PassEncodeResult + 'graph,
    ) {
        self.passes.push(PassNode::Command(CommandPassNode {
            label,
            reads: reads.to_vec(),
            writes: writes.to_vec(),
            declared_pass_count,
            encode: Box::new(encode),
        }));
    }

    pub(crate) fn node_count(&self) -> usize {
        self.passes.len()
    }

    #[cfg(test)]
    pub(crate) fn declared_pass_count(&self) -> u32 {
        self.passes.iter().map(PassNode::declared_pass_count).sum()
    }
}

pub(crate) enum PassNode<'graph> {
    Command(CommandPassNode<'graph>),
}

pub(crate) struct CommandPassNode<'graph> {
    label: Option<&'static str>,
    reads: Vec<TextureHandle>,
    writes: Vec<TextureHandle>,
    declared_pass_count: u32,
    encode: Box<dyn for<'pass> FnOnce(&mut PassContext<'pass>) -> PassEncodeResult + 'graph>,
}

impl CommandPassNode<'_> {
    fn reads(&self) -> &[TextureHandle] {
        &self.reads
    }

    fn writes(&self) -> &[TextureHandle] {
        &self.writes
    }

    fn declared_pass_count(&self) -> u32 {
        self.declared_pass_count
    }
}

impl PassNode<'_> {
    fn label(&self) -> Option<&'static str> {
        match self {
            Self::Command(pass) => pass.label,
        }
    }

    fn reads(&self) -> &[TextureHandle] {
        match self {
            Self::Command(pass) => pass.reads(),
        }
    }

    fn writes(&self) -> &[TextureHandle] {
        match self {
            Self::Command(pass) => pass.writes(),
        }
    }

    fn declared_pass_count(&self) -> u32 {
        match self {
            Self::Command(pass) => pass.declared_pass_count(),
        }
    }

    fn encode(
        self,
        pass_index: usize,
        context: &mut PassContext<'_>,
    ) -> Result<u32, FrameGraphError> {
        let declared_pass_count = self.declared_pass_count();
        match self {
            Self::Command(pass) => {
                let pass_label = pass.label;
                (pass.encode)(context).map_err(|message| FrameGraphError::PassFailed {
                    pass_index,
                    label: pass_label,
                    message,
                })?;
            }
        }
        Ok(context.recorded_pass_count().max(declared_pass_count))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextureHandle(usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextureResource {
    label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTextureDescriptor {
    pub(crate) label: &'static str,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: wgpu::TextureFormat,
}

impl FrameTextureDescriptor {
    pub(crate) fn render_attachment(
        label: &'static str,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Self {
        Self {
            label,
            width: width.max(1),
            height: height.max(1),
            format,
        }
    }

    fn estimated_bytes(self) -> u64 {
        (self.width as u64)
            .saturating_mul(self.height as u64)
            .saturating_mul(texture_format_bytes_per_pixel(self.format))
    }

    fn is_pool_compatible_with(self, other: Self) -> bool {
        self.width == other.width && self.height == other.height && self.format == other.format
    }
}

#[derive(Default)]
pub(crate) struct TransientTexturePool {
    available: Vec<PooledTransientTexture>,
}

struct PooledTransientTexture {
    descriptor: FrameTextureDescriptor,
    target: OffscreenTarget,
}

const MAX_RETAINED_TRANSIENT_TEXTURES: usize = 16;

impl TransientTexturePool {
    fn acquire(
        &mut self,
        device: &wgpu::Device,
        descriptor: FrameTextureDescriptor,
    ) -> OffscreenTarget {
        if let Some(index) = self
            .available
            .iter()
            .position(|entry| entry.descriptor.is_pool_compatible_with(descriptor))
        {
            return self.available.swap_remove(index).target;
        }

        OffscreenTarget::new_labeled(
            device,
            descriptor.format,
            descriptor.width,
            descriptor.height,
            descriptor.label,
        )
    }

    fn release(&mut self, descriptor: FrameTextureDescriptor, target: OffscreenTarget) {
        if self.available.len() < MAX_RETAINED_TRANSIENT_TEXTURES {
            self.available
                .push(PooledTransientTexture { descriptor, target });
        }
    }

    fn len(&self) -> usize {
        self.available.len()
    }

    fn estimated_bytes(&self) -> u64 {
        self.available
            .iter()
            .map(|entry| entry.descriptor.estimated_bytes())
            .sum()
    }
}

#[derive(Default)]
pub(crate) struct ResourceGraph {
    textures: Vec<TextureResource>,
}

impl ResourceGraph {
    fn import_texture(&mut self, label: &'static str) -> TextureHandle {
        let handle = TextureHandle(self.textures.len());
        self.textures.push(TextureResource { label });
        handle
    }
}

pub(crate) struct PassContext<'pass> {
    queue_handle: &'pass wgpu::Queue,
    pub(crate) encoder: &'pass mut wgpu::CommandEncoder,
    uploads: &'pass mut FrameUploadAllocators,
    transient_textures: &'pass mut TransientTexturePool,
    pending_transient_releases: &'pass mut Vec<(FrameTextureDescriptor, OffscreenTarget)>,
    transient_texture_bytes: &'pass mut u64,
    staged_upload_cursor: &'pass mut u64,
    pass_count: u32,
    gpu_timer: &'pass mut GpuFrameTimer,
}

fn texture_format_bytes_per_pixel(format: wgpu::TextureFormat) -> u64 {
    match format {
        wgpu::TextureFormat::R8Unorm
        | wgpu::TextureFormat::R8Snorm
        | wgpu::TextureFormat::R8Uint
        | wgpu::TextureFormat::R8Sint => 1,
        wgpu::TextureFormat::R16Uint
        | wgpu::TextureFormat::R16Sint
        | wgpu::TextureFormat::R16Unorm
        | wgpu::TextureFormat::R16Snorm
        | wgpu::TextureFormat::R16Float
        | wgpu::TextureFormat::Rg8Unorm
        | wgpu::TextureFormat::Rg8Snorm
        | wgpu::TextureFormat::Rg8Uint
        | wgpu::TextureFormat::Rg8Sint => 2,
        wgpu::TextureFormat::R32Uint
        | wgpu::TextureFormat::R32Sint
        | wgpu::TextureFormat::R32Float
        | wgpu::TextureFormat::Rg16Uint
        | wgpu::TextureFormat::Rg16Sint
        | wgpu::TextureFormat::Rg16Unorm
        | wgpu::TextureFormat::Rg16Snorm
        | wgpu::TextureFormat::Rg16Float
        | wgpu::TextureFormat::Rgba8Unorm
        | wgpu::TextureFormat::Rgba8UnormSrgb
        | wgpu::TextureFormat::Rgba8Snorm
        | wgpu::TextureFormat::Rgba8Uint
        | wgpu::TextureFormat::Rgba8Sint
        | wgpu::TextureFormat::Bgra8Unorm
        | wgpu::TextureFormat::Bgra8UnormSrgb
        | wgpu::TextureFormat::Rgb10a2Uint
        | wgpu::TextureFormat::Rgb10a2Unorm
        | wgpu::TextureFormat::Rg11b10Ufloat
        | wgpu::TextureFormat::Depth24Plus
        | wgpu::TextureFormat::Depth24PlusStencil8
        | wgpu::TextureFormat::Depth32Float => 4,
        wgpu::TextureFormat::Rg32Uint
        | wgpu::TextureFormat::Rg32Sint
        | wgpu::TextureFormat::Rg32Float
        | wgpu::TextureFormat::Rgba16Uint
        | wgpu::TextureFormat::Rgba16Sint
        | wgpu::TextureFormat::Rgba16Unorm
        | wgpu::TextureFormat::Rgba16Snorm
        | wgpu::TextureFormat::Rgba16Float
        | wgpu::TextureFormat::Depth32FloatStencil8 => 8,
        wgpu::TextureFormat::Rgba32Uint
        | wgpu::TextureFormat::Rgba32Sint
        | wgpu::TextureFormat::Rgba32Float => 16,
        _ => 4,
    }
}

impl WgpuFrameGraphExecutor {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn retained_texture_count(&self) -> usize {
        self.transient_textures.len()
    }

    pub(crate) fn retained_texture_bytes(&self) -> u64 {
        self.transient_textures.estimated_bytes()
    }

    pub(crate) fn reset_upload_allocators(&mut self) {
        self.upload_allocators.reset();
    }

    pub(crate) fn upload_texture(
        &mut self,
        queue: &wgpu::Queue,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) -> FrameCommandStats {
        queue.write_texture(destination, data, data_layout, size);
        FrameCommandStats {
            upload_bytes: data.len() as u64,
            ..FrameCommandStats::default()
        }
    }

    pub(crate) fn upload_buffer(
        &self,
        queue: &wgpu::Queue,
        buffer: &wgpu::Buffer,
        offset: u64,
        data: &[u8],
    ) -> FrameCommandStats {
        if data.is_empty() {
            return FrameCommandStats::default();
        }
        queue.write_buffer(buffer, offset, data);
        FrameCommandStats {
            upload_bytes: data.len() as u64,
            ..FrameCommandStats::default()
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn begin<'a>(
        &'a mut self,
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        label: Option<&'static str>,
    ) -> WgpuFrameEncoder<'a> {
        WgpuFrameEncoder {
            queue,
            encoder: Self::create_command_encoder(device, label),
            uploads: &mut self.upload_allocators,
            transient_releases: PendingTransientReleases::new(&mut self.transient_textures),
            transient_texture_bytes: 0,
            pass_count: 0,
            gpu_timer: &mut self.gpu_timer,
        }
    }

    pub(crate) fn execute_recorded_graph(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        graph: WgpuFrameGraph<'_>,
    ) -> Result<FrameGraphExecution, FrameGraphError> {
        if graph.node_count() == 0 {
            return Err(FrameGraphError::EmptyGraph);
        }
        let mut pass_count = 0u32;
        let mut transient_texture_bytes = 0u64;
        let mut staged_upload_cursor = 0u64;
        let mut pending_transient_releases = Vec::new();
        let mut encoder = Self::create_command_encoder(device, graph.label);
        self.gpu_timer.begin_frame(device, queue, &mut encoder);

        if graph.passes.len() == 1 {
            let pass = graph
                .passes
                .into_iter()
                .next()
                .expect("single-pass graph should contain one pass");
            match self.encode_pass_node(
                queue,
                &mut encoder,
                &mut pending_transient_releases,
                &mut transient_texture_bytes,
                &mut staged_upload_cursor,
                0,
                pass,
            ) {
                Ok(recorded_pass_count) => {
                    pass_count = pass_count.saturating_add(recorded_pass_count);
                }
                Err(error) => {
                    release_pending_transients(
                        &mut self.transient_textures,
                        pending_transient_releases,
                    );
                    return Err(error);
                }
            }
        } else {
            let ordered_passes = build_pass_schedule(&graph.passes)?;
            let mut passes = graph.passes.into_iter().map(Some).collect::<Vec<_>>();
            for pass_index in ordered_passes {
                let Some(pass) = passes[pass_index].take() else {
                    return Err(FrameGraphError::ScheduledPassTwice { pass_index });
                };
                match self.encode_pass_node(
                    queue,
                    &mut encoder,
                    &mut pending_transient_releases,
                    &mut transient_texture_bytes,
                    &mut staged_upload_cursor,
                    pass_index,
                    pass,
                ) {
                    Ok(recorded_pass_count) => {
                        pass_count = pass_count.saturating_add(recorded_pass_count);
                    }
                    Err(error) => {
                        release_pending_transients(
                            &mut self.transient_textures,
                            pending_transient_releases,
                        );
                        return Err(error);
                    }
                }
            }
        }
        if pass_count == 0 {
            release_pending_transients(&mut self.transient_textures, pending_transient_releases);
            return Err(FrameGraphError::NoDeclaredPasses);
        }
        self.gpu_timer.end_frame(&mut encoder);
        let submission = Self::submit_with_timing(queue, encoder);
        self.gpu_timer.after_submit(device);
        release_pending_transients(&mut self.transient_textures, pending_transient_releases);
        let retained_texture_bytes = self.retained_texture_bytes();
        Ok(FrameGraphExecution {
            submission,
            stats: FrameCommandStats {
                encoder_count: 1,
                submit_count: 1,
                pass_count,
                transient_texture_bytes,
                retained_texture_bytes,
                ..FrameCommandStats::default()
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_pass_node(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pending_transient_releases: &mut Vec<(FrameTextureDescriptor, OffscreenTarget)>,
        transient_texture_bytes: &mut u64,
        staged_upload_cursor: &mut u64,
        pass_index: usize,
        pass: PassNode<'_>,
    ) -> Result<u32, FrameGraphError> {
        let pass_label = pass.label();
        let pass_start = Instant::now();
        let mut context = PassContext {
            queue_handle: queue,
            encoder,
            uploads: &mut self.upload_allocators,
            transient_textures: &mut self.transient_textures,
            pending_transient_releases,
            transient_texture_bytes,
            staged_upload_cursor,
            pass_count: 0,
            gpu_timer: &mut self.gpu_timer,
        };
        let recorded_pass_count = pass.encode(pass_index, &mut context)?;
        log_frame_graph_pass_timing(pass_start, pass_label, pass_index);
        Ok(recorded_pass_count)
    }

    fn create_command_encoder(
        device: &wgpu::Device,
        label: Option<&'static str>,
    ) -> wgpu::CommandEncoder {
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label })
    }

    fn submit(queue: &wgpu::Queue, encoder: wgpu::CommandEncoder) -> wgpu::SubmissionIndex {
        queue.submit(std::iter::once(encoder.finish()))
    }

    /// [`Self::submit`] with the `finish`/`submit` split reported separately.
    ///
    /// These are the two calls at the end of a frame that hand the recorded
    /// commands to the driver, and on a translated backend they are where the
    /// command stream is marshalled and the round trip is paid. Timing them
    /// apart from the recording that produced them is what separates "the
    /// renderer built too much work" from "the driver is expensive per frame".
    fn submit_with_timing(
        queue: &wgpu::Queue,
        encoder: wgpu::CommandEncoder,
    ) -> wgpu::SubmissionIndex {
        let Some(threshold_ms) = frame_graph_pass_telemetry_threshold_ms() else {
            return Self::submit(queue, encoder);
        };
        let finish_start = Instant::now();
        let command_buffer = encoder.finish();
        let submit_start = Instant::now();
        let submission = queue.submit(std::iter::once(command_buffer));
        let submit_end = Instant::now();

        let finish_ms = submit_start.duration_since(finish_start).as_secs_f64() * 1000.0;
        let submit_ms = submit_end.duration_since(submit_start).as_secs_f64() * 1000.0;
        if finish_ms + submit_ms >= threshold_ms {
            log::warn!(
                "[wgpu-render-stage:submit] finish_ms={finish_ms:.3} submit_ms={submit_ms:.3}"
            );
        }
        submission
    }
}

fn frame_graph_pass_telemetry_threshold_ms() -> Option<f64> {
    std::env::var("CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY_MS")
        .ok()
        .and_then(|raw| raw.parse::<f64>().ok())
        .filter(|threshold| *threshold >= 0.0)
}

fn log_frame_graph_pass_timing(start: Instant, label: Option<&'static str>, pass_index: usize) {
    let Some(threshold_ms) = frame_graph_pass_telemetry_threshold_ms() else {
        return;
    };
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    if total_ms < threshold_ms {
        return;
    }
    log::warn!(
        "[wgpu-render-stage:frame-graph-pass] total_ms={total_ms:.2} index={} label={}",
        pass_index,
        label.unwrap_or("<unnamed>")
    );
}

pub(crate) trait FrameCommandRecorder {
    fn encoder(&mut self) -> &mut wgpu::CommandEncoder;
    fn upload_uniform(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> wgpu::BindGroup;
    fn upload_vertex(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
        device: &wgpu::Device,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> wgpu::Buffer;
    fn acquire_transient_offscreen(
        &mut self,
        device: &wgpu::Device,
        descriptor: FrameTextureDescriptor,
    ) -> OffscreenTarget;
    fn release_transient_offscreen(
        &mut self,
        descriptor: FrameTextureDescriptor,
        target: OffscreenTarget,
    );
    fn allocate_staged_upload_bytes(&mut self, bytes: u64) -> u64;
    fn record_passes(&mut self, count: u32);

    fn record_pass(&mut self) {
        self.record_passes(1);
    }

    fn recorded_pass_count(&self) -> u32;

    fn begin_gpu_span(
        &mut self,
        kind: GpuSpanKind,
        width: u32,
        height: u32,
    ) -> Option<GpuSpanId>;

    fn end_gpu_span(&mut self, span: Option<GpuSpanId>);
}

impl FrameCommandRecorder for PassContext<'_> {
    fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder
    }

    fn upload_uniform(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> wgpu::BindGroup {
        self.uploads.upload_uniform(
            id,
            spec,
            device,
            self.queue_handle,
            layout,
            bytes,
            uploaded_bytes,
        )
    }

    fn upload_vertex(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
        device: &wgpu::Device,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> wgpu::Buffer {
        self.uploads
            .upload_vertex(id, spec, device, self.queue_handle, bytes, uploaded_bytes)
    }

    fn acquire_transient_offscreen(
        &mut self,
        device: &wgpu::Device,
        descriptor: FrameTextureDescriptor,
    ) -> OffscreenTarget {
        *self.transient_texture_bytes =
            (*self.transient_texture_bytes).saturating_add(descriptor.estimated_bytes());
        self.transient_textures.acquire(device, descriptor)
    }

    fn release_transient_offscreen(
        &mut self,
        descriptor: FrameTextureDescriptor,
        target: OffscreenTarget,
    ) {
        self.pending_transient_releases.push((descriptor, target));
    }

    fn allocate_staged_upload_bytes(&mut self, bytes: u64) -> u64 {
        let aligned = align_u64_to(*self.staged_upload_cursor, wgpu::COPY_BUFFER_ALIGNMENT);
        *self.staged_upload_cursor = aligned.saturating_add(bytes);
        aligned
    }

    fn record_passes(&mut self, count: u32) {
        self.pass_count = self.pass_count.saturating_add(count);
    }

    fn recorded_pass_count(&self) -> u32 {
        self.pass_count
    }

    fn begin_gpu_span(&mut self, kind: GpuSpanKind, width: u32, height: u32) -> Option<GpuSpanId> {
        self.gpu_timer.begin_span(self.encoder, kind, width, height)
    }

    fn end_gpu_span(&mut self, span: Option<GpuSpanId>) {
        self.gpu_timer.end_span(self.encoder, span);
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct WgpuFrameEncoder<'a> {
    queue: &'a wgpu::Queue,
    encoder: wgpu::CommandEncoder,
    uploads: &'a mut FrameUploadAllocators,
    transient_releases: PendingTransientReleases<'a>,
    gpu_timer: &'a mut GpuFrameTimer,
    transient_texture_bytes: u64,
    pass_count: u32,
}

#[cfg(target_arch = "wasm32")]
impl WgpuFrameEncoder<'_> {
    pub(crate) fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
        &mut self.encoder
    }

    pub(crate) fn record_passes(&mut self, count: u32) {
        self.pass_count = self.pass_count.saturating_add(count);
    }

    pub(crate) fn recorded_pass_count(&self) -> u32 {
        self.pass_count
    }

    pub(crate) fn finish(self) -> FrameGraphExecution {
        let pass_count = self.pass_count;
        let transient_texture_bytes = self.transient_texture_bytes;
        let mut transient_releases = self.transient_releases;
        let submission = WgpuFrameGraphExecutor::submit(self.queue, self.encoder);
        transient_releases.release_pending();
        let retained_texture_bytes = transient_releases.retained_texture_bytes();
        FrameGraphExecution {
            submission,
            stats: FrameCommandStats {
                encoder_count: 1,
                submit_count: 1,
                pass_count,
                transient_texture_bytes,
                retained_texture_bytes,
                ..FrameCommandStats::default()
            },
        }
    }
}

#[cfg(target_arch = "wasm32")]
struct PendingTransientReleases<'a> {
    transient_textures: &'a mut TransientTexturePool,
    pending: Vec<(FrameTextureDescriptor, OffscreenTarget)>,
}

#[cfg(target_arch = "wasm32")]
impl<'a> PendingTransientReleases<'a> {
    fn new(transient_textures: &'a mut TransientTexturePool) -> Self {
        Self {
            transient_textures,
            pending: Vec::new(),
        }
    }

    fn acquire(
        &mut self,
        device: &wgpu::Device,
        descriptor: FrameTextureDescriptor,
    ) -> OffscreenTarget {
        self.transient_textures.acquire(device, descriptor)
    }

    fn push_release(&mut self, descriptor: FrameTextureDescriptor, target: OffscreenTarget) {
        self.pending.push((descriptor, target));
    }

    fn release_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        release_pending_transients(self.transient_textures, pending);
    }

    fn retained_texture_bytes(&self) -> u64 {
        self.transient_textures.estimated_bytes()
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for PendingTransientReleases<'_> {
    fn drop(&mut self) {
        self.release_pending();
    }
}

#[cfg(target_arch = "wasm32")]
impl FrameCommandRecorder for WgpuFrameEncoder<'_> {
    fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
        Self::encoder(self)
    }

    fn upload_uniform(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> wgpu::BindGroup {
        self.uploads
            .upload_uniform(id, spec, device, self.queue, layout, bytes, uploaded_bytes)
    }

    fn upload_vertex(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
        device: &wgpu::Device,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> wgpu::Buffer {
        self.uploads
            .upload_vertex(id, spec, device, self.queue, bytes, uploaded_bytes)
    }

    fn acquire_transient_offscreen(
        &mut self,
        device: &wgpu::Device,
        descriptor: FrameTextureDescriptor,
    ) -> OffscreenTarget {
        self.transient_texture_bytes = self
            .transient_texture_bytes
            .saturating_add(descriptor.estimated_bytes());
        self.transient_releases.acquire(device, descriptor)
    }

    fn release_transient_offscreen(
        &mut self,
        descriptor: FrameTextureDescriptor,
        target: OffscreenTarget,
    ) {
        self.transient_releases.push_release(descriptor, target);
    }

    fn record_passes(&mut self, count: u32) {
        Self::record_passes(self, count);
    }

    fn allocate_staged_upload_bytes(&mut self, _bytes: u64) -> u64 {
        0
    }

    fn recorded_pass_count(&self) -> u32 {
        Self::recorded_pass_count(self)
    }

    fn begin_gpu_span(&mut self, kind: GpuSpanKind, width: u32, height: u32) -> Option<GpuSpanId> {
        self.gpu_timer
            .begin_span(&mut self.encoder, kind, width, height)
    }

    fn end_gpu_span(&mut self, span: Option<GpuSpanId>) {
        self.gpu_timer.end_span(&mut self.encoder, span);
    }
}

fn align_u64_to(value: u64, alignment: u64) -> u64 {
    debug_assert!(alignment > 0);
    value.div_ceil(alignment) * alignment
}

fn release_pending_transients(
    transient_pool: &mut TransientTexturePool,
    pending_releases: Vec<(FrameTextureDescriptor, OffscreenTarget)>,
) {
    for (descriptor, target) in pending_releases {
        transient_pool.release(descriptor, target);
    }
}

fn build_pass_schedule(passes: &[PassNode<'_>]) -> Result<Vec<usize>, FrameGraphError> {
    let mut dependency_count = vec![0usize; passes.len()];
    let mut dependents = vec![Vec::new(); passes.len()];
    let mut last_access = Vec::<Option<usize>>::new();
    let mut last_writer = Vec::<Option<usize>>::new();

    for (pass_index, pass) in passes.iter().enumerate() {
        for handle in pass.reads() {
            if handle.0 >= last_writer.len() {
                last_writer.resize(handle.0 + 1, None);
            }
            if let Some(writer) = last_writer[handle.0] {
                add_pass_dependency(&mut dependency_count, &mut dependents, pass_index, writer);
            }
        }
        for handle in pass.writes() {
            if handle.0 >= last_access.len() {
                last_access.resize(handle.0 + 1, None);
            }
            if let Some(accessor) = last_access[handle.0] {
                add_pass_dependency(&mut dependency_count, &mut dependents, pass_index, accessor);
            }
        }
        for handle in pass.reads() {
            if handle.0 >= last_access.len() {
                last_access.resize(handle.0 + 1, None);
            }
            last_access[handle.0] = Some(pass_index);
        }
        for handle in pass.writes() {
            if handle.0 >= last_writer.len() {
                last_writer.resize(handle.0 + 1, None);
            }
            last_writer[handle.0] = Some(pass_index);
            last_access[handle.0] = Some(pass_index);
        }
    }

    let mut ready = dependency_count
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(passes.len());

    while let Some(pass_index) = ready.first().copied() {
        ready.remove(0);
        ordered.push(pass_index);
        for dependent in &dependents[pass_index] {
            dependency_count[*dependent] -= 1;
            if dependency_count[*dependent] == 0 {
                ready.push(*dependent);
            }
        }
    }

    if ordered.len() != passes.len() {
        return Err(FrameGraphError::CyclicPassDependencies {
            scheduled: ordered.len(),
            total: passes.len(),
        });
    }
    Ok(ordered)
}

fn add_pass_dependency(
    dependency_count: &mut [usize],
    dependents: &mut [Vec<usize>],
    pass_index: usize,
    dependency: usize,
) {
    if dependency == pass_index || dependents[dependency].contains(&pass_index) {
        return;
    }
    dependency_count[pass_index] += 1;
    dependents[dependency].push(pass_index);
}

struct UploadSlot {
    buffer: wgpu::Buffer,
    size: u64,
    bind_group: OnceCell<wgpu::BindGroup>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UploadAllocatorId {
    BlurHorizontal,
    BlurVertical,
    Offset,
    Blit,
    ProjectiveBlitUniform,
    ProjectiveBlitVertex,
    EffectUniform,
    BlurRoundedMask,
}

impl UploadAllocatorId {
    fn index(self) -> usize {
        match self {
            Self::BlurHorizontal => 0,
            Self::BlurVertical => 1,
            Self::Offset => 2,
            Self::Blit => 3,
            Self::ProjectiveBlitUniform => 4,
            Self::ProjectiveBlitVertex => 5,
            Self::EffectUniform => 6,
            Self::BlurRoundedMask => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UploadAllocatorKind {
    Uniform,
    Vertex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UploadAllocatorSpec {
    buffer_label: &'static str,
    bind_group_label: Option<&'static str>,
    size: u64,
    kind: UploadAllocatorKind,
}

impl UploadAllocatorSpec {
    pub(crate) fn uniform(
        buffer_label: &'static str,
        bind_group_label: &'static str,
        size: u64,
    ) -> Self {
        Self {
            buffer_label,
            bind_group_label: Some(bind_group_label),
            size,
            kind: UploadAllocatorKind::Uniform,
        }
    }

    pub(crate) fn vertex(buffer_label: &'static str, size: u64) -> Self {
        Self {
            buffer_label,
            bind_group_label: None,
            size,
            kind: UploadAllocatorKind::Vertex,
        }
    }
}

#[derive(Default)]
pub(crate) struct FrameUploadAllocators {
    allocators: Vec<Option<UploadAllocator>>,
}

impl FrameUploadAllocators {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upload_uniform(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> wgpu::BindGroup {
        debug_assert_eq!(
            spec.kind,
            UploadAllocatorKind::Uniform,
            "upload_uniform requires a uniform allocator spec"
        );
        self.allocator_mut(id, spec)
            .upload_uniform(device, queue, layout, bytes, uploaded_bytes)
            .clone()
    }

    pub(crate) fn upload_vertex(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> wgpu::Buffer {
        debug_assert_eq!(
            spec.kind,
            UploadAllocatorKind::Vertex,
            "upload_vertex requires a vertex allocator spec"
        );
        self.allocator_mut(id, spec)
            .upload_vertex(device, queue, bytes, uploaded_bytes)
            .clone()
    }

    pub(crate) fn reset(&mut self) {
        for allocator in self.allocators.iter_mut().flatten() {
            allocator.reset();
        }
    }

    fn allocator_mut(
        &mut self,
        id: UploadAllocatorId,
        spec: UploadAllocatorSpec,
    ) -> &mut UploadAllocator {
        let index = id.index();
        if self.allocators.len() <= index {
            self.allocators.resize_with(index + 1, || None);
        }
        let allocator = self.allocators[index].get_or_insert_with(|| UploadAllocator::new(spec));
        debug_assert!(
            allocator.matches(spec),
            "frame upload allocator id reused with a different spec"
        );
        allocator
    }
}

pub(crate) struct UploadAllocator {
    buffer_label: &'static str,
    bind_group_label: Option<&'static str>,
    size: u64,
    usage: wgpu::BufferUsages,
    cursor: usize,
    slots: Vec<UploadSlot>,
    /// Largest slot size any upload has asked for since the last
    /// [`UploadAllocator::reset`].
    frame_peak_size: u64,
    /// Slot size the allocator is willing to carry between frames. Tracks the
    /// high-water mark upwards immediately and downwards only after a
    /// [`OVERSIZED_SLOT_COLLAPSE_FACTOR`] collapse.
    retained_size: u64,
}

/// How far a frame's demand has to fall before an oversized upload slot is
/// released. Matches the hysteresis the text scratch buffers use: a scene that
/// needs a large slot needs it every frame, so releasing it at the end of each
/// one just recreates the same buffer on the next.
const OVERSIZED_SLOT_COLLAPSE_FACTOR: u64 = 4;

impl UploadAllocator {
    fn new(spec: UploadAllocatorSpec) -> Self {
        match spec.kind {
            UploadAllocatorKind::Uniform => Self::uniform(
                spec.buffer_label,
                spec.bind_group_label.unwrap_or("Uniform Upload Bind Group"),
                spec.size,
            ),
            UploadAllocatorKind::Vertex => Self::vertex(spec.buffer_label, spec.size),
        }
    }

    pub(crate) fn uniform(
        buffer_label: &'static str,
        bind_group_label: &'static str,
        size: u64,
    ) -> Self {
        Self {
            buffer_label,
            bind_group_label: Some(bind_group_label),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            cursor: 0,
            slots: Vec::new(),
            frame_peak_size: 0,
            retained_size: 0,
        }
    }

    pub(crate) fn vertex(buffer_label: &'static str, size: u64) -> Self {
        Self {
            buffer_label,
            bind_group_label: None,
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            cursor: 0,
            slots: Vec::new(),
            frame_peak_size: 0,
            retained_size: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.cursor = 0;
        // A scene that needs an oversized slot needs it on every frame, so
        // releasing it at the end of each one only recreates the same buffer
        // (and its bind group) on the next — a `vkCreateBuffer` /
        // `vkDestroyBuffer` pair per slot per frame, which is dear on a
        // translated driver. Follow the high-water mark up immediately and
        // back down only once frames have collapsed to a quarter of it, so a
        // one-off spike is still released without charging the steady state.
        if self.frame_peak_size > 0 {
            self.retained_size = if self
                .frame_peak_size
                .saturating_mul(OVERSIZED_SLOT_COLLAPSE_FACTOR)
                <= self.retained_size
            {
                self.frame_peak_size
            } else {
                self.retained_size.max(self.frame_peak_size)
            };
            self.frame_peak_size = 0;
        }
        let retain_limit = self.size.max(self.retained_size);
        self.slots
            .retain(|slot| Self::should_retain_slot_size(retain_limit, slot.size));
    }

    fn matches(&self, spec: UploadAllocatorSpec) -> bool {
        self.buffer_label == spec.buffer_label
            && self.bind_group_label == spec.bind_group_label
            && self.size == spec.size
            && match spec.kind {
                UploadAllocatorKind::Uniform => self.usage.contains(wgpu::BufferUsages::UNIFORM),
                UploadAllocatorKind::Vertex => self.usage.contains(wgpu::BufferUsages::VERTEX),
            }
    }

    pub(crate) fn upload_uniform<'a>(
        &'a mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> &'a wgpu::BindGroup {
        debug_assert!(
            self.usage.contains(wgpu::BufferUsages::UNIFORM),
            "upload_uniform requires a uniform allocator"
        );
        let index = self.upload(device, queue, bytes, uploaded_bytes);
        let label = self.bind_group_label.unwrap_or("Uniform Upload Bind Group");
        self.slots[index].bind_group.get_or_init(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.slots[index].buffer.as_entire_binding(),
                }],
            })
        })
    }

    pub(crate) fn upload_vertex<'a>(
        &'a mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> &'a wgpu::Buffer {
        debug_assert!(
            self.usage.contains(wgpu::BufferUsages::VERTEX),
            "upload_vertex requires a vertex allocator"
        );
        let index = self.upload(device, queue, bytes, uploaded_bytes);
        &self.slots[index].buffer
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        uploaded_bytes: &Cell<u64>,
    ) -> usize {
        let required_size = self.required_slot_size(bytes.len());
        self.frame_peak_size = self.frame_peak_size.max(required_size);
        if self.cursor == self.slots.len() {
            self.slots.push(self.create_slot(device, required_size));
        }
        let index = self.cursor;
        self.cursor += 1;
        if self.slots[index].size < required_size {
            self.slots[index] = self.create_slot(device, required_size);
        }
        queue.write_buffer(&self.slots[index].buffer, 0, bytes);
        uploaded_bytes.set(uploaded_bytes.get().saturating_add(bytes.len() as u64));
        index
    }

    fn create_slot(&self, device: &wgpu::Device, size: u64) -> UploadSlot {
        UploadSlot {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.buffer_label),
                size,
                usage: self.usage,
                mapped_at_creation: false,
            }),
            size,
            bind_group: OnceCell::new(),
        }
    }

    fn required_slot_size(&self, byte_len: usize) -> u64 {
        self.size.max(align_u64_to(
            byte_len.max(1) as u64,
            wgpu::COPY_BUFFER_ALIGNMENT,
        ))
    }

    /// Whether a slot survives [`UploadAllocator::reset`]. `retain_limit` is the
    /// allocator's nominal size raised to its recent high-water mark, so slots
    /// the current scene keeps asking for are kept and slots left over from a
    /// larger past frame are released.
    fn should_retain_slot_size(retain_limit: u64, slot_size: u64) -> bool {
        slot_size <= retain_limit
    }

    #[cfg(test)]
    fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{build_pass_schedule, FrameTextureDescriptor, UploadAllocator, WgpuFrameGraph};

    #[test]
    fn upload_allocator_reset_rewinds_cursor() {
        let mut allocator = UploadAllocator::uniform("test buffer", "test bind group", 64);
        allocator.cursor = 3;
        allocator.reset();
        assert_eq!(allocator.cursor, 0);
        assert_eq!(allocator.slot_count(), 0);
    }

    #[test]
    fn upload_allocator_grows_slot_size_for_large_payloads() {
        let allocator = UploadAllocator::uniform("test buffer", "test bind group", 64);

        assert_eq!(allocator.required_slot_size(8), 64);
        assert_eq!(
            allocator.required_slot_size(65),
            super::align_u64_to(65, wgpu::COPY_BUFFER_ALIGNMENT)
        );
    }

    #[test]
    fn upload_allocator_does_not_retain_oversized_slots_after_reset() {
        assert!(UploadAllocator::should_retain_slot_size(64, 64));
        assert!(!UploadAllocator::should_retain_slot_size(64, 256));
    }

    #[test]
    fn upload_allocator_keeps_slots_a_steady_scene_asks_for_every_frame() {
        let mut allocator = UploadAllocator::uniform("test buffer", "test bind group", 64);
        // Two frames that both need a 256-byte slot must not recreate it.
        allocator.frame_peak_size = 256;
        allocator.reset();
        assert!(UploadAllocator::should_retain_slot_size(
            allocator.size.max(allocator.retained_size),
            256
        ));
        allocator.frame_peak_size = 256;
        allocator.reset();
        assert_eq!(allocator.retained_size, 256);
    }

    #[test]
    fn upload_allocator_releases_a_slot_once_demand_collapses() {
        let mut allocator = UploadAllocator::uniform("test buffer", "test bind group", 64);
        allocator.frame_peak_size = 1024;
        allocator.reset();
        assert_eq!(allocator.retained_size, 1024);
        // A quarter of the high-water mark is the point where it is given back.
        allocator.frame_peak_size = 256;
        allocator.reset();
        assert_eq!(allocator.retained_size, 256);
        assert!(!UploadAllocator::should_retain_slot_size(
            allocator.size.max(allocator.retained_size),
            1024
        ));
    }

    #[test]
    fn pass_schedule_orders_reads_after_last_writer() {
        let mut graph = WgpuFrameGraph::new(None);
        let target = graph.import_surface("surface");
        graph.add_fallible_command_pass(Some("writer"), &[], &[target], |_| Ok(()));
        graph.add_fallible_command_pass(Some("independent"), &[], &[], |_| Ok(()));
        graph.add_fallible_command_pass(Some("reader"), &[target], &[], |_| Ok(()));

        let order = build_pass_schedule(&graph.passes).expect("valid pass schedule");
        let writer_index = order
            .iter()
            .position(|index| *index == 0)
            .expect("writer pass should be scheduled");
        let reader_index = order
            .iter()
            .position(|index| *index == 2)
            .expect("reader pass should be scheduled");

        assert!(writer_index < reader_index);
    }

    #[test]
    fn pass_schedule_keeps_later_writes_after_earlier_reads() {
        let mut graph = WgpuFrameGraph::new(None);
        let target = graph.import_surface("surface");
        let dependency = graph.import_surface("dependency");
        graph.add_fallible_command_pass(Some("dependency writer"), &[], &[dependency], |_| Ok(()));
        graph.add_fallible_command_pass(Some("target reader"), &[target, dependency], &[], |_| {
            Ok(())
        });
        graph.add_fallible_command_pass(Some("target writer"), &[], &[target], |_| Ok(()));

        let order = build_pass_schedule(&graph.passes).expect("valid pass schedule");
        let reader_index = order
            .iter()
            .position(|index| *index == 1)
            .expect("reader pass should be scheduled");
        let writer_index = order
            .iter()
            .position(|index| *index == 2)
            .expect("writer pass should be scheduled");

        assert!(reader_index < writer_index);
    }

    #[test]
    fn transient_texture_descriptor_clamps_and_accounts_bytes() {
        let descriptor = FrameTextureDescriptor::render_attachment(
            "scratch",
            0,
            2,
            wgpu::TextureFormat::Bgra8Unorm,
        );

        assert_eq!(descriptor.width, 1);
        assert_eq!(descriptor.height, 2);
        assert_eq!(descriptor.estimated_bytes(), 8);
    }

    #[test]
    fn frame_graph_records_imported_texture_resources() {
        let mut graph = WgpuFrameGraph::new(None);
        let handle = graph.import_surface("surface");

        assert_eq!(handle.0, 0);
        assert_eq!(graph.resources.textures[0].label, "surface");
    }

    #[test]
    fn command_nodes_declare_wgpu_pass_count() {
        let mut graph = WgpuFrameGraph::new(None);
        let source = graph.import_surface("source");
        let dest = graph.import_surface("dest");

        graph.add_fallible_command_pass(Some("copy"), &[source], &[dest], |_| Ok(()));

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.declared_pass_count(), 1);
    }
}
