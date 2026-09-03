use std::{
    cell::{Cell, OnceCell},
    fmt,
};

use web_time::Instant;

use crate::{
    offscreen::OffscreenTarget,
    pass_timing::{GpuPassTimingReport, PassTimer},
};

#[derive(Default)]
pub(crate) struct WgpuFrameGraphExecutor {
    transient_textures: TransientTexturePool,
    upload_allocators: FrameUploadAllocators,
    pass_timer: Option<PassTimer>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameCommandStats {
    pub(crate) encoder_count: u32,
    pub(crate) submit_count: u32,
    pub(crate) pass_count: u32,
    pub(crate) pass_pixels: u64,
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
    device: &'pass wgpu::Device,
    queue_handle: &'pass wgpu::Queue,
    pub(crate) encoder: &'pass mut wgpu::CommandEncoder,
    uploads: &'pass mut FrameUploadAllocators,
    transient_textures: &'pass mut TransientTexturePool,
    pending_transient_releases: &'pass mut Vec<(FrameTextureDescriptor, OffscreenTarget)>,
    transient_texture_bytes: &'pass mut u64,
    pass_count: u32,
    pass_timer: Option<&'pass PassTimer>,
}

pub(crate) fn texture_format_bytes_per_pixel(format: wgpu::TextureFormat) -> u64 {
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

    pub(crate) fn init_pass_timing(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if crate::pass_timing::pass_timing_requested() {
            self.pass_timer = PassTimer::for_device(device, queue);
        }
    }

    pub(crate) fn end_pass_timing_frame(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some(timer) = &self.pass_timer else {
            return;
        };
        let _ = device.poll(wgpu::PollType::Poll);
        timer.harvest_completed();
        if let Some(resolve) = timer.frame_resolve() {
            let mut encoder =
                Self::create_command_encoder(device, Some("Pass Timing Resolve Encoder"));
            resolve.encode(&mut encoder);
            Self::submit(queue, encoder);
            resolve.arm_readback();
        }
        timer.finish_frame();
    }

    pub(crate) fn pass_timing_report(&self) -> GpuPassTimingReport {
        self.pass_timer
            .as_ref()
            .map(PassTimer::report)
            .unwrap_or_default()
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
        note_upload_write();
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
            pass_timer: self.pass_timer.as_ref(),
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
        let mut pending_transient_releases = Vec::new();
        let mut encoder = Self::create_command_encoder(device, graph.label);

        if graph.passes.len() == 1 {
            let pass = graph
                .passes
                .into_iter()
                .next()
                .expect("single-pass graph should contain one pass");
            match self.encode_pass_node(
                device,
                queue,
                &mut encoder,
                &mut pending_transient_releases,
                &mut transient_texture_bytes,
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
                    device,
                    queue,
                    &mut encoder,
                    &mut pending_transient_releases,
                    &mut transient_texture_bytes,
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
        if fence_profile::enabled() {
            fence_profile::end_frame(device, queue, &mut encoder);
        }
        let submission = Self::submit_with_timing(queue, encoder);
        release_pending_transients(&mut self.transient_textures, pending_transient_releases);
        let retained_texture_bytes = self.retained_texture_bytes();
        Ok(FrameGraphExecution {
            submission,
            stats: FrameCommandStats {
                encoder_count: 1,
                submit_count: 1,
                pass_count,
                pass_pixels: take_render_pass_pixels(),
                transient_texture_bytes,
                retained_texture_bytes,
                ..FrameCommandStats::default()
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_pass_node(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pending_transient_releases: &mut Vec<(FrameTextureDescriptor, OffscreenTarget)>,
        transient_texture_bytes: &mut u64,
        pass_index: usize,
        pass: PassNode<'_>,
    ) -> Result<u32, FrameGraphError> {
        let pass_label = pass.label();
        let pass_start = Instant::now();
        let mut context = PassContext {
            device,
            queue_handle: queue,
            encoder,
            uploads: &mut self.upload_allocators,
            transient_textures: &mut self.transient_textures,
            pending_transient_releases,
            transient_texture_bytes,
            pass_count: 0,
            pass_timer: self.pass_timer.as_ref(),
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
        let _ = take_upload_write_calls();
        queue.submit(std::iter::once(encoder.finish()))
    }

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
        let upload_writes = take_upload_write_calls();
        let pass_labels = take_render_pass_labels();
        if finish_ms + submit_ms >= threshold_ms {
            let pass_total: u32 = pass_labels.iter().map(|(_, count)| count).sum();
            let mut split = String::new();
            for (label, count) in &pass_labels {
                use std::fmt::Write;
                let _ = write!(split, " [{label}]={count}");
            }
            log::warn!(
                "[wgpu-render-stage:submit] finish_ms={finish_ms:.3} submit_ms={submit_ms:.3} \
                 upload_writes={upload_writes} passes={pass_total}{split}"
            );
        }
        submission
    }
}

std::thread_local! {
    static UPLOAD_WRITE_CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Writes `data` into `buffer` at `offset` ahead of the next submit and
/// accounts for it as an upload; an empty write is skipped.
pub(crate) fn write_buffer(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    offset: u64,
    data: &[u8],
) -> FrameCommandStats {
    if data.is_empty() {
        return FrameCommandStats::default();
    }
    queue.write_buffer(buffer, offset, data);
    note_upload_write();
    FrameCommandStats {
        upload_bytes: data.len() as u64,
        ..FrameCommandStats::default()
    }
}

fn note_upload_write() {
    UPLOAD_WRITE_CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
}

fn take_upload_write_calls() -> u32 {
    UPLOAD_WRITE_CALLS.with(|calls| calls.replace(0))
}

std::thread_local! {
    static RENDER_PASS_LABELS: std::cell::RefCell<Vec<(String, u32)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

std::thread_local! {
    static RENDER_PASS_PIXELS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Accounts one render pass: its first color target's area, the tile
/// traffic a tiling GPU pays to open and close the pass, and its label when
/// the stage telemetry is on.
pub(crate) fn note_render_pass(descriptor: &wgpu::RenderPassDescriptor<'_>) {
    let pixels = descriptor
        .color_attachments
        .iter()
        .flatten()
        .next()
        .map(|attachment| {
            let texture = attachment.view.texture();
            u64::from(texture.width()) * u64::from(texture.height())
        })
        .unwrap_or(0);
    RENDER_PASS_PIXELS.with(|total| total.set(total.get().saturating_add(pixels)));
    if frame_graph_pass_telemetry_threshold_ms().is_none() {
        return;
    }
    let label = fence_profile::bucket_label(descriptor);
    RENDER_PASS_LABELS.with(|labels| {
        let mut labels = labels.borrow_mut();
        match labels.last_mut() {
            Some((name, count)) if *name == label => *count += 1,
            _ => labels.push((label, 1)),
        }
    });
}

fn take_render_pass_pixels() -> u64 {
    RENDER_PASS_PIXELS.with(|total| total.replace(0))
}

fn take_render_pass_labels() -> Vec<(String, u32)> {
    RENDER_PASS_LABELS.with(|labels| std::mem::take(&mut *labels.borrow_mut()))
}

pub(crate) fn frame_graph_pass_telemetry_threshold_ms() -> Option<f64> {
    crate::debug_toggles::debug_toggle("CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY_MS")
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
    fn begin_timed_render_pass(
        &mut self,
        descriptor: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'_>;
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
    fn record_passes(&mut self, count: u32);

    fn record_pass(&mut self) {
        self.record_passes(1);
    }

    fn recorded_pass_count(&self) -> u32;
}

impl FrameCommandRecorder for PassContext<'_> {
    fn begin_timed_render_pass(
        &mut self,
        descriptor: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'_> {
        if fence_profile::enabled() {
            let bucket = fence_profile::bucket_label(descriptor);
            fence_profile::split(self.device, self.queue_handle, self.encoder, Some(&bucket));
        }
        crate::pass_timing::begin_timed_render_pass(self.pass_timer, self.encoder, descriptor)
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

    fn record_passes(&mut self, count: u32) {
        self.pass_count = self.pass_count.saturating_add(count);
    }

    fn recorded_pass_count(&self) -> u32 {
        self.pass_count
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct WgpuFrameEncoder<'a> {
    queue: &'a wgpu::Queue,
    encoder: wgpu::CommandEncoder,
    uploads: &'a mut FrameUploadAllocators,
    transient_releases: PendingTransientReleases<'a>,
    transient_texture_bytes: u64,
    pass_count: u32,
    pass_timer: Option<&'a PassTimer>,
}

#[cfg(target_arch = "wasm32")]
impl WgpuFrameEncoder<'_> {
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
                pass_pixels: take_render_pass_pixels(),
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
    fn begin_timed_render_pass(
        &mut self,
        descriptor: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'_> {
        crate::pass_timing::begin_timed_render_pass(self.pass_timer, &mut self.encoder, descriptor)
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

    fn recorded_pass_count(&self) -> u32 {
        Self::recorded_pass_count(self)
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
    frame_peak_size: u64,
    retained_size: u64,
}

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
        note_upload_write();
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
    use super::{FrameTextureDescriptor, UploadAllocator, WgpuFrameGraph, build_pass_schedule};

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

/// Lives beside the executor because it is the only other place that may
/// finish an encoder and submit it.
pub(crate) mod fence_profile {
    use std::cell::RefCell;

    use web_time::Instant;

    const DEFAULT_REPORT_EVERY_FRAMES: u32 = 60;

    fn report_every_frames() -> u32 {
        static FRAMES: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        *FRAMES.get_or_init(|| {
            crate::debug_toggles::debug_toggle("CRANPOSE_GPU_FENCE_PROFILE")
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|frames| *frames > 0)
                .unwrap_or(DEFAULT_REPORT_EVERY_FRAMES)
        })
    }

    /// Every render pass a frame records, bucketed by label, target size and
    /// load op, with the wall time of each measured through submission fences
    /// for adapters without timestamp queries (`CRANPOSE_GPU_FENCE_PROFILE`,
    /// mirrored as `debug.cranpose.gpu_fence_profile` on Android; a numeric
    /// value is the report period in frames, 60 by default). Every pass
    /// boundary submits the commands recorded so far and waits for the queue
    /// to drain, minus the round trip of an empty submission, so a bucket's
    /// time is the isolated latency of its passes plus the copies encoded
    /// after them. Isolated latency overstates large targets, whose store and
    /// reload a tiler otherwise overlaps with the next pass, so the buckets
    /// are a per-frame pass inventory, not a cost ranking.
    pub(crate) fn enabled() -> bool {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| {
            crate::debug_toggles::debug_toggle("CRANPOSE_GPU_FENCE_PROFILE").is_some()
        })
    }

    /// `CRANPOSE_GPU_FENCE_PROFILE=frame`: one fence per frame instead of one
    /// per pass, so the report is the frame's whole GPU time with the
    /// pipeline intact.
    fn whole_frame() -> bool {
        static WHOLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *WHOLE.get_or_init(|| {
            crate::debug_toggles::debug_toggle("CRANPOSE_GPU_FENCE_PROFILE").as_deref()
                == Some("frame")
        })
    }

    #[derive(Default)]
    struct Profile {
        current_label: Option<String>,
        totals: Vec<(String, f64, u32)>,
        frames: u32,
    }

    thread_local! {
        static PROFILE: RefCell<Profile> = RefCell::new(Profile::default());
    }

    fn submit_and_wait(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: wgpu::CommandEncoder,
    ) -> f64 {
        let start = Instant::now();
        let submission = queue.submit(std::iter::once(encoder.finish()));
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        });
        start.elapsed().as_secs_f64() * 1000.0
    }

    fn new_encoder(device: &wgpu::Device) -> wgpu::CommandEncoder {
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fence profile split"),
        })
    }

    /// Wall time of the recorded commands minus the round trip of an empty
    /// submission, so the fixed cost of the fence itself does not count.
    fn drain(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) -> f64 {
        let finished = std::mem::replace(encoder, new_encoder(device));
        let recorded = submit_and_wait(device, queue, finished);
        let empty = submit_and_wait(device, queue, new_encoder(device));
        (recorded - empty).max(0.0)
    }

    /// The pass label extended with its first color target's size, so passes on
    /// the full-screen target rank apart from the same pass on a small layer.
    pub(crate) fn bucket_label(descriptor: &wgpu::RenderPassDescriptor<'_>) -> String {
        let label = descriptor.label.unwrap_or("<unlabeled>");
        match descriptor
            .color_attachments
            .first()
            .and_then(|attachment| attachment.as_ref())
        {
            Some(attachment) => {
                let texture = attachment.view.texture();
                let load = match attachment.ops.load {
                    wgpu::LoadOp::Load => "load",
                    wgpu::LoadOp::Clear(_) => "clear",
                    wgpu::LoadOp::DontCare(_) => "dontcare",
                };
                format!("{label} {}x{} {load}", texture.width(), texture.height())
            }
            None => label.to_owned(),
        }
    }

    /// Closes the pass that was running, charges its GPU time, and opens the
    /// accounting for `next_label`.
    pub(crate) fn split(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        next_label: Option<&str>,
    ) {
        if whole_frame() && next_label.is_some() {
            PROFILE.with(|profile| {
                profile
                    .borrow_mut()
                    .current_label
                    .get_or_insert_with(|| "frame".to_owned());
            });
            return;
        }
        let elapsed = drain(device, queue, encoder);
        PROFILE.with(|profile| {
            let mut profile = profile.borrow_mut();
            if let Some(label) = profile.current_label.take() {
                match profile
                    .totals
                    .iter_mut()
                    .find(|(name, _, _)| *name == label)
                {
                    Some(entry) => {
                        entry.1 += elapsed;
                        entry.2 += 1;
                    }
                    None => profile.totals.push((label, elapsed, 1)),
                }
            }
            profile.current_label = next_label.map(str::to_owned);
        });
    }

    /// Charges the frame's last pass and prints the per-label ranking once per
    /// report period.
    pub(crate) fn end_frame(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        split(device, queue, encoder, None);
        PROFILE.with(|profile| {
            let mut profile = profile.borrow_mut();
            profile.frames += 1;
            if profile.frames < report_every_frames() {
                return;
            }
            let frames = f64::from(profile.frames);
            let mut totals = std::mem::take(&mut profile.totals);
            totals.sort_by(|a, b| b.1.total_cmp(&a.1));
            let total_ms: f64 = totals.iter().map(|(_, ms, _)| ms).sum::<f64>() / frames;
            let mut line = format!(
                "[gpu-fence] frames={} total={total_ms:.2}ms/frame",
                profile.frames
            );
            for (label, ms, count) in &totals {
                use std::fmt::Write;
                let _ = write!(
                    line,
                    " [{label}]={:.2}ms x{:.1}",
                    ms / frames,
                    f64::from(*count) / frames
                );
            }
            eprintln!("{line}");
            profile.frames = 0;
        });
    }
}
