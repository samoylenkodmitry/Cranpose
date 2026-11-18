#![expect(
    unsafe_code,
    reason = "the pass that converts a decoded picture is Vulkan, called through ash on the renderer's device"
)]

//! The renderer's Vulkan device as raw handles, for the pass that converts a decoded picture.
//!
//! The pass draws into textures that the renderer samples afterwards, so everything that it
//! creates lives on the renderer's device, and its commands go through the renderer's queue as
//! part of a wgpu command buffer: the queue is never used from two threads at once.

use std::sync::Arc;

use ash::vk;
use wgpu::hal::api::Vulkan;

use super::hardware_buffer_extensions::imports;
use crate::GpuContext;

// The pass writes into textures of this format, which the renderer samples as stored.
pub(super) const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub(super) const COLOR: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

// The handles of the device and what every object of the pass needs from it. Each object that
// the GPU may still use holds the `Arc`, so the device and the render pass outlive them.
pub(super) struct RawDevice {
    pub(super) gpu: GpuContext,
    pub(super) device: ash::Device,
    pub(super) hardware_buffer: ash::android::external_memory_android_hardware_buffer::Device,
    pub(super) queue_family: u32,
    pub(super) render_pass: vk::RenderPass,
    memory: vk::PhysicalDeviceMemoryProperties,
}

impl RawDevice {
    // The handles of `gpu`'s device, or `None` when it is not a Vulkan device that was opened with
    // the import extensions.
    pub(super) fn new(gpu: &GpuContext) -> Option<Arc<Self>> {
        // SAFETY: the guard reads the device's handles while this function runs.
        let hal = unsafe { gpu.device.as_hal::<Vulkan>() }?;
        if !imports(hal.enabled_device_extensions()) {
            return None;
        }
        let instance = hal.shared_instance().raw_instance();
        let device = hal.raw_device().clone();
        let hardware_buffer =
            ash::android::external_memory_android_hardware_buffer::Device::new(instance, &device);
        // SAFETY: the physical device belongs to the instance.
        let memory =
            unsafe { instance.get_physical_device_memory_properties(hal.raw_physical_device()) };
        let queue_family = hal.queue_family_index();
        let render_pass = create_render_pass(&device)
            .map_err(|error| log::warn!("android video import: no render pass: {error}"))
            .ok()?;
        Some(Arc::new(Self {
            gpu: gpu.clone(),
            device,
            hardware_buffer,
            queue_family,
            render_pass,
            memory,
        }))
    }

    // The first type of memory that `bits` allows, which is where a hardware buffer's memory lives.
    pub(super) fn memory_type(&self, bits: u32) -> Option<u32> {
        (0..self.memory.memory_type_count).find(|index| bits & (1 << index) != 0)
    }

    // A texture of the ring that the pass draws into. The renderer takes it as sampled and
    // initialized, which is what it holds once the pass has drawn the first picture into it.
    pub(super) fn render_target(&self, width: u32, height: u32) -> Option<wgpu::Texture> {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let descriptor = wgpu::hal::TextureDescriptor {
            label: Some("decoded picture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUses::COLOR_TARGET | wgpu::TextureUses::RESOURCE,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: Vec::new(),
        };
        // SAFETY: the guard reads the device's handles while this function runs, and the texture
        // is created with a descriptor that the device supports.
        let created = unsafe {
            let hal = self.gpu.device.as_hal::<Vulkan>()?;
            wgpu::hal::Device::create_texture(&*hal, &descriptor)
        };
        let hal_texture = created
            .map_err(|error| log::warn!("android video import: no render target: {error}"))
            .ok()?;
        // SAFETY: the texture was created on this device with a descriptor that matches the one
        // below. Its first use is the pass, which starts from an undefined layout, and the barrier
        // below puts the texture in the layout of the state that it is wrapped with.
        let texture = unsafe {
            self.gpu.device.create_texture_from_hal::<Vulkan>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("decoded picture"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: TARGET_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::TextureUses::RESOURCE,
            )
        };
        let image = {
            // SAFETY: the guard reads the texture's handle while this block runs.
            let hal = unsafe { texture.as_hal::<Vulkan>() }?;
            // SAFETY: the handle is read only, and the texture outlives the commands below.
            unsafe { hal.raw_handle() }
        };
        let commands = self.record(|commands| {
            let barrier = vk::ImageMemoryBarrier::default()
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(COLOR);
            // SAFETY: the commands are recording, and the image is valid.
            unsafe {
                self.device.cmd_pipeline_barrier(
                    commands,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
            }
        })?;
        self.gpu.queue.submit([commands]);
        Some(texture)
    }

    // The commands that `record` writes into a new command buffer of the renderer's device.
    // Submit the buffer with the renderer's queue.
    pub(super) fn record(
        &self,
        record: impl FnOnce(vk::CommandBuffer),
    ) -> Option<wgpu::CommandBuffer> {
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("decoded picture"),
            });
        // SAFETY: the encoder is new, so nothing else records into it, and the closure only
        // records commands into the buffer, which wgpu ends and submits.
        let recorded = unsafe {
            encoder.as_hal_mut::<Vulkan, _, _>(|hal| hal.map(|hal| record(hal.raw_handle())))
        };
        recorded.map(|()| encoder.finish())
    }
}

impl Drop for RawDevice {
    fn drop(&mut self) {
        // SAFETY: every object that records this render pass holds the `Arc` of this struct, so
        // none of their commands is pending.
        unsafe { self.device.destroy_render_pass(self.render_pass, None) };
    }
}

// Makes the render pass: one color attachment that the pass overwrites completely and leaves in
// the layout that the renderer samples. The dependencies order the pass after the work that used
// the texture before, and the work that samples it after the pass.
fn create_render_pass(device: &ash::Device) -> Result<vk::RenderPass, vk::Result> {
    let attachments = [vk::AttachmentDescription::default()
        .format(vk::Format::R8G8B8A8_UNORM)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::DONT_CARE)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
    let colors = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&colors)];
    let reads = vk::PipelineStageFlags::VERTEX_SHADER
        | vk::PipelineStageFlags::FRAGMENT_SHADER
        | vk::PipelineStageFlags::COMPUTE_SHADER;
    let dependencies = [
        vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(reads | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
        vk::SubpassDependency::default()
            .src_subpass(0)
            .dst_subpass(vk::SUBPASS_EXTERNAL)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(reads)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ),
    ];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    // SAFETY: the info and the arrays that it points to live through the call.
    unsafe { device.create_render_pass(&info, None) }
}
