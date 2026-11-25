#![expect(
    unsafe_code,
    reason = "the pipeline of the conversion pass is built and bound through Vulkan calls"
)]

//! The pipeline that converts the pictures of one hardware buffer format to RGB.
//!
//! The sampler of a hardware buffer's YCbCr conversion is part of the pipeline's layout, so each
//! format that a decoder outputs has a pipeline of its own. A stream has one format, and the
//! converter builds the pipeline when the first picture comes.

use std::{
    io::Cursor,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use ash::vk;
use cranpose_services::{ColorPrimaries, ColorRange, ColorTransfer, VideoColorInfo};

use super::{crop_window::WINDOW_SIZE, raw_device::RawDevice};

const VERTEX_SHADER: &[u8] = include_bytes!("ycbcr_cover.vert.spv");
const FRAGMENT_SHADER: &[u8] = include_bytes!("ycbcr_to_rgb.frag.spv");

// The sets that a pipeline hands out: one for each picture that the GPU may still read. The
// pictures wait in the decoder's queue and in the renderer, so the sets that the converter has in
// flight stay far below this.
const SET_COUNT: u32 = 8;
const SETS: usize = SET_COUNT as usize;

// What decides the conversion of a hardware buffer: its format, and how the format's samples
// become R'G'B'. The values come from the driver's properties of the buffer, which suggest the
// model and the range that the decoder wrote.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct YcbcrFormat {
    pub(super) format: vk::Format,
    pub(super) external_format: u64,
    model: vk::SamplerYcbcrModelConversion,
    range: vk::SamplerYcbcrRange,
    components: [vk::ComponentSwizzle; 4],
    x_offset: vk::ChromaLocation,
    y_offset: vk::ChromaLocation,
    chroma_filter: vk::Filter,
    transfer: ColorTransfer,
    gamut_2020: bool,
}

impl YcbcrFormat {
    pub(super) fn of(
        properties: &vk::AndroidHardwareBufferFormatPropertiesANDROID<'_>,
        color: VideoColorInfo,
    ) -> Self {
        // The sampler reads the chroma samples with the filter that the format can do. Its own
        // filters equal it, because a format that cannot filter apart from the chroma samples
        // needs equal filters.
        let chroma_filter = if properties
            .format_features
            .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_YCBCR_CONVERSION_LINEAR_FILTER)
        {
            vk::Filter::LINEAR
        } else {
            vk::Filter::NEAREST
        };
        let (model, range) =
            if properties.suggested_ycbcr_model == vk::SamplerYcbcrModelConversion::RGB_IDENTITY {
                (
                    properties.suggested_ycbcr_model,
                    properties.suggested_ycbcr_range,
                )
            } else {
                let model = match color.primaries {
                    ColorPrimaries::Bt601 => vk::SamplerYcbcrModelConversion::YCBCR_601,
                    ColorPrimaries::Bt709 => vk::SamplerYcbcrModelConversion::YCBCR_709,
                    ColorPrimaries::Bt2020 => vk::SamplerYcbcrModelConversion::YCBCR_2020,
                };
                let range = match color.range {
                    ColorRange::Full => vk::SamplerYcbcrRange::ITU_FULL,
                    ColorRange::Limited => vk::SamplerYcbcrRange::ITU_NARROW,
                };
                (model, range)
            };
        Self {
            format: properties.format,
            external_format: properties.external_format,
            model,
            range,
            components: [
                properties.sampler_ycbcr_conversion_components.r,
                properties.sampler_ycbcr_conversion_components.g,
                properties.sampler_ycbcr_conversion_components.b,
                properties.sampler_ycbcr_conversion_components.a,
            ],
            x_offset: properties.suggested_x_chroma_offset,
            y_offset: properties.suggested_y_chroma_offset,
            chroma_filter,
            transfer: color.transfer,
            gamut_2020: matches!(color.primaries, ColorPrimaries::Bt2020),
        }
    }
}

// The conversion, its sampler, the layouts that hold the sampler and the pipeline, with the
// descriptor sets that bind one picture each.
pub(super) struct YcbcrPipeline {
    raw: Arc<RawDevice>,
    pub(super) format: YcbcrFormat,
    pub(super) conversion: vk::SamplerYcbcrConversion,
    sampler: vk::Sampler,
    set_layout: vk::DescriptorSetLayout,
    pub(super) layout: vk::PipelineLayout,
    pub(super) pipeline: vk::Pipeline,
    pool: vk::DescriptorPool,
    sets: Vec<vk::DescriptorSet>,
    free: Mutex<Vec<usize>>,
}

// A descriptor set that a picture holds until the GPU has read it.
pub(super) struct SetLease {
    pipeline: Arc<YcbcrPipeline>,
    index: usize,
}

impl YcbcrPipeline {
    // Builds the pipeline for `format`, or `None` when the driver refuses it.
    pub(super) fn new(raw: &Arc<RawDevice>, format: YcbcrFormat) -> Option<Arc<Self>> {
        // The parts are owned from here on, so the struct's `Drop` frees what a failure leaves.
        let mut pipeline = Self {
            raw: Arc::clone(raw),
            format,
            conversion: vk::SamplerYcbcrConversion::null(),
            sampler: vk::Sampler::null(),
            set_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            pool: vk::DescriptorPool::null(),
            sets: Vec::new(),
            free: Mutex::new((0..SETS).collect()),
        };
        pipeline
            .build()
            .map_err(|error| log::warn!("android video import: no pipeline: {error}"))
            .ok()?;
        Some(Arc::new(pipeline))
    }

    fn build(&mut self) -> Result<(), vk::Result> {
        let device = &self.raw.device;
        let format = self.format;
        let mut external =
            vk::ExternalFormatANDROID::default().external_format(format.external_format);
        let mut conversion_info = vk::SamplerYcbcrConversionCreateInfo::default()
            .format(format.format)
            .ycbcr_model(format.model)
            .ycbcr_range(format.range)
            .components(vk::ComponentMapping {
                r: format.components[0],
                g: format.components[1],
                b: format.components[2],
                a: format.components[3],
            })
            .x_chroma_offset(format.x_offset)
            .y_chroma_offset(format.y_offset)
            .chroma_filter(format.chroma_filter);
        if format.format == vk::Format::UNDEFINED {
            conversion_info = conversion_info.push_next(&mut external);
        }
        // SAFETY: the infos live through each call, and the handles that the calls return are
        // stored in `self` at once, so `Drop` frees them.
        unsafe {
            self.conversion = device.create_sampler_ycbcr_conversion(&conversion_info, None)?;
            let mut with_conversion =
                vk::SamplerYcbcrConversionInfo::default().conversion(self.conversion);
            let sampler_info = vk::SamplerCreateInfo::default()
                .mag_filter(format.chroma_filter)
                .min_filter(format.chroma_filter)
                .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .push_next(&mut with_conversion);
            self.sampler = device.create_sampler(&sampler_info, None)?;
            let samplers = [self.sampler];
            let bindings = [vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .immutable_samplers(&samplers)];
            self.set_layout = device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            let set_layouts = [self.set_layout];
            let window = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .offset(0)
                .size(WINDOW_SIZE)];
            self.layout = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&window),
                None,
            )?;
            self.pipeline = self.create_pipeline()?;
            let sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(SET_COUNT)];
            self.pool = device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(SET_COUNT)
                    .pool_sizes(&sizes),
                None,
            )?;
            let layouts = [self.set_layout; SETS];
            self.sets = device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.pool)
                    .set_layouts(&layouts),
            )?;
        }
        Ok(())
    }

    // Creates the graphics pipeline of the pass. The viewport and the scissor are dynamic, so the
    // pipeline does not depend on the size of the picture.
    //
    // # Safety
    //
    // `self.layout` is a valid pipeline layout.
    unsafe fn create_pipeline(&self) -> Result<vk::Pipeline, vk::Result> {
        let device = &self.raw.device;
        let vertex = ShaderModule::new(device, VERTEX_SHADER)?;
        let fragment = ShaderModule::new(device, FRAGMENT_SHADER)?;
        let transfer_id: u32 = match self.format.transfer {
            ColorTransfer::Sdr => 0,
            ColorTransfer::Pq => 1,
            ColorTransfer::Hlg => 2,
        };
        let gamut_id: u32 = u32::from(self.format.gamut_2020);
        let mut spec_bytes = [0_u8; 8];
        spec_bytes[..4].copy_from_slice(&transfer_id.to_ne_bytes());
        spec_bytes[4..].copy_from_slice(&gamut_id.to_ne_bytes());
        let spec_entries = [
            vk::SpecializationMapEntry {
                constant_id: 0,
                offset: 0,
                size: 4,
            },
            vk::SpecializationMapEntry {
                constant_id: 1,
                offset: 4,
                size: 4,
            },
        ];
        let spec_info = vk::SpecializationInfo::default()
            .map_entries(&spec_entries)
            .data(&spec_bytes);
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vertex.raw)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fragment.raw)
                .name(c"main")
                .specialization_info(&spec_info),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let attachments = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&rasterization)
            .multisample_state(&multisample)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.layout)
            .render_pass(self.raw.render_pass)
            .subpass(0);
        // SAFETY: the infos live through the call, and the shader modules until it returns.
        let pipelines =
            unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) }
                .map_err(|(_, error)| error)?;
        Ok(pipelines[0])
    }

    // Takes the descriptor set of one picture, or `None` while every set is in use.
    pub(super) fn lease(self: &Arc<Self>) -> Option<SetLease> {
        let index = free(&self.free).pop()?;
        Some(SetLease {
            pipeline: Arc::clone(self),
            index,
        })
    }
}

impl SetLease {
    // Makes the set sample `view`, a view of a picture's image in the layout that the pass
    // samples it in.
    pub(super) fn bind(&self, view: vk::ImageView) {
        let image = [vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set())
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image);
        // SAFETY: the set is leased, so no pending command reads it, and the view is valid.
        unsafe {
            self.pipeline
                .raw
                .device
                .update_descriptor_sets(&[write], &[]);
        }
    }

    pub(super) fn set(&self) -> vk::DescriptorSet {
        self.pipeline.sets[self.index]
    }

    pub(super) fn pipeline(&self) -> &YcbcrPipeline {
        &self.pipeline
    }
}

impl Drop for SetLease {
    fn drop(&mut self) {
        free(&self.pipeline.free).push(self.index);
    }
}

impl Drop for YcbcrPipeline {
    fn drop(&mut self) {
        let device = &self.raw.device;
        // SAFETY: each lease holds the `Arc` of this pipeline, so no command that uses these
        // objects is pending. A handle that a failed build never made is null, which Vulkan
        // accepts.
        unsafe {
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_sampler_ycbcr_conversion(self.conversion, None);
        }
    }
}

// A shader module that is destroyed once the pipeline has its code.
struct ShaderModule<'a> {
    device: &'a ash::Device,
    raw: vk::ShaderModule,
}

impl<'a> ShaderModule<'a> {
    fn new(device: &'a ash::Device, spirv: &[u8]) -> Result<Self, vk::Result> {
        let words = ash::util::read_spv(&mut Cursor::new(spirv))
            .map_err(|_| vk::Result::ERROR_INVALID_SHADER_NV)?;
        // SAFETY: the code is the SPIR-V that `scripts/dev/compile_ycbcr_shaders.sh` checked.
        let raw = unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
        }?;
        Ok(Self { device, raw })
    }
}

impl Drop for ShaderModule<'_> {
    fn drop(&mut self) {
        // SAFETY: a pipeline keeps its own copy of the code, so no object uses the module.
        unsafe { self.device.destroy_shader_module(self.raw, None) };
    }
}

fn free(free: &Mutex<Vec<usize>>) -> MutexGuard<'_, Vec<usize>> {
    free.lock().unwrap_or_else(PoisonError::into_inner)
}
