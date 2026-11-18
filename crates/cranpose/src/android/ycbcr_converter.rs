#![expect(
    unsafe_code,
    reason = "a decoded picture's hardware buffer is a C object, and the conversion pass is recorded through Vulkan"
)]

//! Shows a decoded picture on the GPU: the picture's hardware buffer becomes a Vulkan image, and
//! one pass converts it to RGB in a texture that Cranpose draws.
//!
//! The buffer is imported with no copy. The sampler of the pass holds the YCbCr conversion that the
//! driver suggests for the buffer's format, so the pass writes the encoded R'G'B' values of the
//! picture, as the Apple converter does. The picture stays alive until the GPU has read it: the
//! work that the queue holds owns the frame, and the callback of the queue's submission drops it.

use std::sync::Arc;

use ash::vk;
use cranpose_ui::ImageBitmap;
use wgpu::hal::api::Vulkan;

use super::{
    crop_window::CropWindow,
    hardware_buffer_image::{BufferProperties, HardwareBufferImage},
    raw_device::{COLOR, RawDevice, TARGET_FORMAT},
    ycbcr_pipeline::{SetLease, YcbcrFormat, YcbcrPipeline},
};
use crate::{ExternalFrames, FrameTexture, FrameWriter, GpuContext, VideoFrame};

// Converts the pictures of one decoder for one GPU context.
pub(crate) struct YcbcrConverter {
    gpu: GpuContext,
    raw: Arc<RawDevice>,
    frames: ExternalFrames,
    writer: FrameWriter,
    size: (u32, u32),
    // The pipeline of the last picture's format: a stream's pictures have one format
    pipeline: Option<Arc<YcbcrPipeline>>,
}

// What the GPU reads and writes while it converts one picture, and drops once it is done. wgpu
// does not see the raw commands, so only this struct keeps the target texture alive while the pass
// runs. The frame comes last, so the decoder's image is free again only after the objects that
// name it are gone.
struct InFlight {
    _framebuffer: Framebuffer,
    _target: FrameTexture,
    _image: HardwareBufferImage,
    _set: SetLease,
    _frame: VideoFrame,
}

// The framebuffer of one target view.
struct Framebuffer {
    raw: Arc<RawDevice>,
    raw_framebuffer: vk::Framebuffer,
}

impl YcbcrConverter {
    // Makes the converter, or `None` when the context's device is not a Vulkan device that was
    // opened with the extensions that import a hardware buffer.
    pub(crate) fn new(gpu: &GpuContext) -> Option<Self> {
        let raw = RawDevice::new(gpu).or_else(|| {
            log::warn!(
                "player surface: the renderer's device cannot import a decoder's hardware buffers"
            );
            None
        })?;
        let targets = Arc::clone(&raw);
        let frames = ExternalFrames::with_textures(gpu, TARGET_FORMAT, move |width, height| {
            targets.render_target(width, height)
        });
        Some(Self {
            gpu: gpu.clone(),
            raw,
            writer: frames.writer(),
            frames,
            size: (0, 0),
            pipeline: None,
        })
    }

    // Converts `frame` into the next texture of the ring and returns the bitmap that draws it.
    // `None` when the picture cannot be imported, or when the ring has no free texture because
    // the composition has not drawn the pictures that it holds.
    pub(crate) fn convert(&mut self, frame: VideoFrame) -> Option<ImageBitmap> {
        let crop = frame.crop();
        let size = (crop.width, crop.height);
        if size != self.size {
            self.frames.set_size(size.0, size.1);
            self.size = size;
        }
        let buffer = frame.as_hardware_buffer_ptr();
        let properties = BufferProperties::of(&self.raw, buffer)?;
        let pipeline = self.pipeline_for(properties.format)?;
        let set = pipeline.lease().or_else(|| {
            log::warn!("player surface: the GPU holds too many pictures");
            None
        })?;
        let image =
            HardwareBufferImage::import(&self.raw, buffer, &properties, pipeline.conversion)?;
        set.bind(image.view);
        let target = self.writer.begin()?;
        let framebuffer = Framebuffer::new(&self.raw, &target, size)?;
        let window = CropWindow::new(crop, properties.size);
        let commands = self.raw.record(|commands| {
            self.record_pass(commands, &set, &image, &framebuffer, size, &window);
        })?;
        let number = target.number();
        let work = InFlight {
            _framebuffer: framebuffer,
            _target: target,
            _image: image,
            _set: set,
            _frame: frame,
        };
        self.gpu.queue.submit([commands]);
        self.gpu.queue.on_submitted_work_done(move || drop(work));
        self.writer.publish(number);
        self.frames.next_frame()
    }

    // The pipeline for pictures of `format`, built when the format is not the last one's.
    fn pipeline_for(&mut self, format: YcbcrFormat) -> Option<Arc<YcbcrPipeline>> {
        if let Some(pipeline) = &self.pipeline
            && pipeline.format == format
        {
            return Some(Arc::clone(pipeline));
        }
        let pipeline = YcbcrPipeline::new(&self.raw, format)?;
        self.pipeline = Some(Arc::clone(&pipeline));
        Some(pipeline)
    }

    // Records the pass: the acquire of the picture's image from the decoder, then one triangle
    // that covers the target and samples the picture.
    fn record_pass(
        &self,
        commands: vk::CommandBuffer,
        set: &SetLease,
        image: &HardwareBufferImage,
        framebuffer: &Framebuffer,
        size: (u32, u32),
        window: &CropWindow,
    ) {
        let device = &self.raw.device;
        let pipeline = set.pipeline();
        let area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: size.0,
                height: size.1,
            },
        };
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: size.0 as f32,
            height: size.1 as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        // The decoder wrote the image, and the sampler reads it: the image changes queue family
        // from the foreign one, which keeps what the decoder wrote
        let acquire = vk::ImageMemoryBarrier::default()
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .dst_queue_family_index(self.raw.queue_family)
            .image(image.image)
            .subresource_range(COLOR);
        let begin = vk::RenderPassBeginInfo::default()
            .render_pass(self.raw.render_pass)
            .framebuffer(framebuffer.raw_framebuffer)
            .render_area(area);
        // SAFETY: the commands are recording. Every handle belongs to the renderer's device, and
        // the objects that the commands name live until the GPU has run them.
        unsafe {
            device.cmd_pipeline_barrier(
                commands,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[acquire],
            );
            device.cmd_begin_render_pass(commands, &begin, vk::SubpassContents::INLINE);
            device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, pipeline.pipeline);
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            device.cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                pipeline.layout,
                0,
                &[set.set()],
                &[],
            );
            device.cmd_push_constants(
                commands,
                pipeline.layout,
                vk::ShaderStageFlags::FRAGMENT,
                0,
                &window.to_bytes(),
            );
            device.cmd_draw(commands, 3, 1, 0, 0);
            device.cmd_end_render_pass(commands);
        }
    }
}

impl Framebuffer {
    // The framebuffer of the texture that `target` renders into, `size` pixels large.
    fn new(raw: &Arc<RawDevice>, target: &FrameTexture, size: (u32, u32)) -> Option<Self> {
        // SAFETY: the guard reads the view's handle while this block runs.
        let view = unsafe {
            let hal = target.view().as_hal::<Vulkan>()?;
            hal.raw_handle()
        };
        let attachments = [view];
        let info = vk::FramebufferCreateInfo::default()
            .render_pass(raw.render_pass)
            .attachments(&attachments)
            .width(size.0)
            .height(size.1)
            .layers(1);
        // SAFETY: the info lives through the call, and the view and the render pass belong to the
        // device.
        let raw_framebuffer = unsafe { raw.device.create_framebuffer(&info, None) }
            .map_err(|error| log::warn!("android video import: no framebuffer: {error}"))
            .ok()?;
        Some(Self {
            raw: Arc::clone(raw),
            raw_framebuffer,
        })
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        // SAFETY: the converter drops the framebuffer after the GPU has run the pass.
        unsafe {
            self.raw
                .device
                .destroy_framebuffer(self.raw_framebuffer, None);
        }
    }
}
