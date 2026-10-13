use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use cranpose::GpuContext;
use objc2::{Message, rc::Retained, runtime::ProtocolObject};
use objc2_metal::{
    MTLBlitCommandEncoder, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLDevice,
    MTLSharedEvent, MTLTexture,
};
use wgpu::hal::{self, api::Metal};

use crate::{InteropError, NativeTexture};

const MAX_COMMAND_BUFFERS: usize = 4096;

pub(crate) struct QueueBridge {
    engine_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    cranpose_done: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    engine_done: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    cranpose_value: AtomicU64,
    engine_value: AtomicU64,
}

pub(crate) type EngineTexture = Retained<ProtocolObject<dyn MTLTexture>>;

/// # Safety
///
/// `device` must point to a live `MTLDevice` and `queue` to a live
/// `MTLCommandQueue` of that device. Both are retained here.
pub(crate) unsafe fn gpu_context(
    device: u64,
    queue: u64,
) -> Result<(GpuContext, QueueBridge), InteropError> {
    let device: Retained<ProtocolObject<dyn MTLDevice>> =
        unsafe { Retained::retain(device as usize as *mut ProtocolObject<dyn MTLDevice>) }
            .ok_or(InteropError::MissingHandle("MTLDevice"))?;
    let engine_queue: Retained<ProtocolObject<dyn MTLCommandQueue>> =
        unsafe { Retained::retain(queue as usize as *mut ProtocolObject<dyn MTLCommandQueue>) }
            .ok_or(InteropError::MissingHandle("MTLCommandQueue"))?;
    let own_queue = device
        .newCommandQueueWithMaxCommandBufferCount(MAX_COMMAND_BUFFERS)
        .ok_or(InteropError::MissingHandle("Cranpose's MTLCommandQueue"))?;
    let cranpose_done = device
        .newSharedEvent()
        .ok_or(InteropError::MissingHandle("MTLSharedEvent"))?;
    let engine_done = device
        .newSharedEvent()
        .ok_or(InteropError::MissingHandle("MTLSharedEvent"))?;
    let name = device.name().to_string();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::METAL))
        .into_iter()
        .find(|adapter| adapter.get_info().name == name)
        .ok_or_else(|| InteropError::NoAdapter(name.clone()))?;
    let features = adapter.features() - wgpu::Features::all_experimental_mask();
    let limits = adapter.limits();
    let open = hal::OpenDevice::<Metal> {
        device: unsafe { hal::metal::Device::device_from_raw(device, features, &limits) },
        queue: unsafe { hal::metal::Queue::queue_from_raw(own_queue, 1.0) },
    };
    let (device, queue) = unsafe {
        adapter.create_device_from_hal(
            open,
            &wgpu::DeviceDescriptor {
                label: Some("Cranpose on the engine's device"),
                required_features: features,
                required_limits: limits,
                ..Default::default()
            },
        )
    }?;
    if let Some(hal_queue) = unsafe { queue.as_hal::<Metal>() } {
        hal_queue
            .enable_strict_event_sync()
            .map_err(|_| InteropError::MissingHandle("event sync on Cranpose's queue"))?;
    }
    Ok((
        GpuContext {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
        },
        QueueBridge {
            engine_queue,
            cranpose_done,
            engine_done,
            cranpose_value: AtomicU64::new(0),
            engine_value: AtomicU64::new(0),
        },
    ))
}

/// # Safety
///
/// `handle` must point to a live `MTLTexture`.
pub(crate) unsafe fn engine_texture(handle: u64) -> Result<EngineTexture, InteropError> {
    unsafe { Retained::retain(handle as usize as *mut ProtocolObject<dyn MTLTexture>) }
        .ok_or(InteropError::MissingHandle("MTLTexture"))
}

pub(crate) fn twin_texture(gpu: &GpuContext, texture: NativeTexture) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("engine texture twin"),
        size: wgpu::Extent3d {
            width: texture.width,
            height: texture.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: texture.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn raw_texture(texture: &wgpu::Texture) -> Option<EngineTexture> {
    // SAFETY: the hal texture is read only to retain its MTLTexture.
    let hal_texture = unsafe { texture.as_hal::<Metal>() }?;
    Some(hal_texture.raw_handle().retain())
}

fn hal_queue(gpu: &GpuContext) -> Option<impl std::ops::Deref<Target = hal::metal::Queue>> {
    // SAFETY: the hal queue is used only to stage events for wgpu's own
    // next submit, which wgpu-hal's Metal queue supports.
    unsafe { gpu.queue.as_hal::<Metal>() }
}

impl QueueBridge {
    pub(crate) fn receive(
        &self,
        gpu: &GpuContext,
        engine: &EngineTexture,
        cranpose: &wgpu::Texture,
    ) {
        let Some(cranpose) = raw_texture(cranpose) else {
            return;
        };
        let after = self.cranpose_value.load(Ordering::Acquire);
        self.copy_on_engine_queue(gpu, after, engine, &cranpose);
    }

    pub(crate) fn begin_frame(&self, gpu: &GpuContext) -> u64 {
        let value = self.cranpose_value.load(Ordering::Acquire) + 1;
        if let Some(queue) = hal_queue(gpu) {
            queue.add_signal_event(self.cranpose_done.clone(), value);
        }
        value
    }

    pub(crate) fn finish_frame(
        &self,
        gpu: &GpuContext,
        value: u64,
        drew: bool,
        cranpose: &wgpu::Texture,
        engine: &EngineTexture,
    ) {
        if !drew {
            if let Some(queue) = hal_queue(gpu) {
                queue.remove_signal_event(&self.cranpose_done);
            }
            return;
        }
        self.cranpose_value.store(value, Ordering::Release);
        let Some(cranpose) = raw_texture(cranpose) else {
            return;
        };
        self.copy_on_engine_queue(gpu, value, &cranpose, engine);
    }

    fn copy_on_engine_queue(
        &self,
        gpu: &GpuContext,
        after: u64,
        from: &EngineTexture,
        to: &EngineTexture,
    ) {
        let Some(command_buffer) = self.engine_queue.commandBuffer() else {
            return;
        };
        let signal = self.engine_value.fetch_add(1, Ordering::AcqRel) + 1;
        command_buffer.encodeWaitForEvent_value(self.cranpose_done.as_ref(), after);
        if let Some(blit) = command_buffer.blitCommandEncoder() {
            // SAFETY: both textures are live and 2D with one size and format;
            // the twin of an engine texture is created that way.
            unsafe { blit.copyFromTexture_toTexture(from, to) };
            blit.endEncoding();
        }
        command_buffer.encodeSignalEvent_value(self.engine_done.as_ref(), signal);
        command_buffer.commit();
        if let Some(queue) = hal_queue(gpu) {
            queue.add_wait_event(self.engine_done.clone(), signal);
        }
    }
}
