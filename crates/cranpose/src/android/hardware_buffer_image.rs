#![expect(
    unsafe_code,
    reason = "a hardware buffer is a C object behind a raw pointer that Vulkan imports as an image"
)]

//! A decoded picture's hardware buffer, imported into Vulkan as an image that the conversion pass
//! samples.
//!
//! The import shares the buffer's memory with the decoder, so no copy goes through the CPU or the
//! GPU. The picture stays valid while its `VideoFrame` lives.

use std::{ffi::c_void, sync::Arc};

use ash::vk;
use ndk_sys::AHardwareBuffer_Desc;

use super::{
    raw_device::{COLOR, RawDevice},
    ycbcr_pipeline::YcbcrFormat,
};

type Describe =
    unsafe extern "C" fn(buffer: *const vk::AHardwareBuffer, desc: *mut AHardwareBuffer_Desc);

// What the driver says of a hardware buffer: the size of the buffer, how much memory it takes and
// which kinds hold it, and the format that its pictures have.
pub(super) struct BufferProperties {
    pub(super) size: (u32, u32),
    allocation_size: vk::DeviceSize,
    memory_type_bits: u32,
    pub(super) format: YcbcrFormat,
}

// The image of a hardware buffer, its memory and its view. The view carries the pipeline's
// conversion. The image is destroyed with the struct, once the GPU has read it.
pub(super) struct HardwareBufferImage {
    raw: Arc<RawDevice>,
    pub(super) image: vk::Image,
    memory: vk::DeviceMemory,
    pub(super) view: vk::ImageView,
}

impl BufferProperties {
    // The properties of `buffer`, an `AHardwareBuffer*` that a decoder wrote a picture into.
    pub(super) fn of(raw: &RawDevice, buffer: *mut c_void) -> Option<Self> {
        let size = buffer_size(buffer)?;
        let mut format = vk::AndroidHardwareBufferFormatPropertiesANDROID::default();
        let mut properties =
            vk::AndroidHardwareBufferPropertiesANDROID::default().push_next(&mut format);
        // SAFETY: the buffer is live while its frame is, and the properties chain lives through the
        // call.
        unsafe {
            raw.hardware_buffer
                .get_android_hardware_buffer_properties(buffer.cast_const().cast(), &mut properties)
        }
        .map_err(|error| log::warn!("android video import: no buffer properties: {error}"))
        .ok()?;
        let (allocation_size, memory_type_bits) =
            (properties.allocation_size, properties.memory_type_bits);
        Some(Self {
            size,
            allocation_size,
            memory_type_bits,
            format: YcbcrFormat::of(&format),
        })
    }
}

impl HardwareBufferImage {
    // Imports `buffer` as an image that `conversion` samples. `None` when the driver refuses it.
    pub(super) fn import(
        raw: &Arc<RawDevice>,
        buffer: *mut c_void,
        properties: &BufferProperties,
        conversion: vk::SamplerYcbcrConversion,
    ) -> Option<Self> {
        // The handles are null until each call makes its object, so the `Drop` of a struct that a
        // failed call leaves frees what the earlier calls made.
        let mut image = Self {
            raw: Arc::clone(raw),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
        };
        image.create(buffer, properties, conversion)?;
        Some(image)
    }

    fn create(
        &mut self,
        buffer: *mut c_void,
        properties: &BufferProperties,
        conversion: vk::SamplerYcbcrConversion,
    ) -> Option<()> {
        let device = &self.raw.device;
        let format = properties.format;
        let memory_type = self.raw.memory_type(properties.memory_type_bits)?;
        let mut external_memory = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::ANDROID_HARDWARE_BUFFER_ANDROID);
        let mut external_format =
            vk::ExternalFormatANDROID::default().external_format(format.external_format);
        let mut info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format.format)
            .extent(vk::Extent3D {
                width: properties.size.0,
                height: properties.size.1,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external_memory);
        if format.format == vk::Format::UNDEFINED {
            info = info.push_next(&mut external_format);
        }
        // SAFETY: each call gets handles of the renderer's device and infos that live through it,
        // and the buffer is live while its frame is.
        unsafe {
            self.image = device.create_image(&info, None).ok()?;
            let mut import = vk::ImportAndroidHardwareBufferInfoANDROID::default()
                .buffer(buffer.cast::<vk::AHardwareBuffer>());
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(self.image);
            let allocate = vk::MemoryAllocateInfo::default()
                .allocation_size(properties.allocation_size)
                .memory_type_index(memory_type)
                .push_next(&mut import)
                .push_next(&mut dedicated);
            self.memory = device.allocate_memory(&allocate, None).ok()?;
            device.bind_image_memory(self.image, self.memory, 0).ok()?;
            let mut with_conversion =
                vk::SamplerYcbcrConversionInfo::default().conversion(conversion);
            let view = vk::ImageViewCreateInfo::default()
                .image(self.image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format.format)
                .subresource_range(COLOR)
                .push_next(&mut with_conversion);
            self.view = device.create_image_view(&view, None).ok()?;
        }
        Some(())
    }
}

impl Drop for HardwareBufferImage {
    fn drop(&mut self) {
        let device = &self.raw.device;
        // SAFETY: the converter drops the image after the GPU has read it. A handle that a failed
        // import never made is null, which Vulkan accepts.
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

// The width and the height of `buffer`. `AHardwareBuffer_describe` is a function of Android 8,
// and an app links against Android 7, so the function is found when the buffer first comes.
fn buffer_size(buffer: *mut c_void) -> Option<(u32, u32)> {
    let describe = describe()?;
    let mut desc = AHardwareBuffer_Desc {
        width: 0,
        height: 0,
        layers: 0,
        format: 0,
        usage: 0,
        stride: 0,
        rfu0: 0,
        rfu1: 0,
    };
    // SAFETY: the buffer is live while its frame is, and the description is a live local.
    unsafe { describe(buffer.cast_const().cast(), &mut desc) };
    Some((desc.width, desc.height))
}

fn describe() -> Option<Describe> {
    static FOUND: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let address = *FOUND.get_or_init(|| {
        // SAFETY: `dlsym` reads the symbols of the libraries that the process loaded, and the name
        // ends in a NUL.
        unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"AHardwareBuffer_describe".as_ptr()) as usize }
    });
    // SAFETY: a non-null address is the platform's `AHardwareBuffer_describe`, whose signature the
    // type states.
    (address != 0).then(|| unsafe { std::mem::transmute::<usize, Describe>(address) })
}
