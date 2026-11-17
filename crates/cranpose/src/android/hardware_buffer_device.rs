#![expect(
    unsafe_code,
    reason = "the device is opened through wgpu-hal with a callback that adds Vulkan extensions"
)]

//! Opens the renderer's Vulkan device so that it imports the hardware buffers of a decoder.
//!
//! wgpu's device request has no field for a device extension that it does not know, so the device
//! of a Vulkan adapter whose driver has the import extensions is opened through wgpu-hal, which
//! takes a callback that adds the extensions. The YCbCr conversion feature that the decoder's
//! pictures need belongs to wgpu-hal's list of features, which leaves it off, so the callback chains
//! the feature itself.

use std::ffi::CStr;

use ash::vk;
use wgpu::hal::api::Vulkan;

use super::hardware_buffer_extensions::extensions_to_add;

/// The device Cranpose asks an Android adapter for.
pub(crate) fn android_device(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        label: Some("Android Device"),
        // Display timing lets the frame pacer see when frames reach the
        // screen; see `android_display_timing`.
        required_features: cranpose_render_wgpu::optional_device_features(adapter)
            | (adapter.features() & wgpu::Features::VULKAN_GOOGLE_DISPLAY_TIMING),
        required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: crate::gpu_limits::mobile_memory_hints(),
        trace: wgpu::Trace::Off,
    }
}

// Opens the device that Cranpose asks `adapter` for, with the import extensions where the adapter
// is a Vulkan adapter with them, and as wgpu opens it everywhere else.
pub(crate) fn request_device(
    adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    let descriptor = android_device(adapter);
    if let Some(opened) = open_importing(adapter, &descriptor) {
        // SAFETY: the device was opened on this adapter with the features and the limits that the
        // descriptor names.
        match unsafe { adapter.create_device_from_hal(opened, &descriptor) } {
            Ok(device) => return Ok(device),
            Err(error) => log::warn!("android video import: wgpu took no device: {error}"),
        }
    }
    pollster::block_on(adapter.request_device(&descriptor))
}

fn open_importing(
    adapter: &wgpu::Adapter,
    descriptor: &wgpu::DeviceDescriptor<'_>,
) -> Option<wgpu::hal::OpenDevice<Vulkan>> {
    // SAFETY: the guard reads the adapter's handles while this function runs.
    let hal = unsafe { adapter.as_hal::<Vulkan>() }?;
    let instance = hal.shared_instance().raw_instance();
    let physical = hal.raw_physical_device();
    // SAFETY: the physical device belongs to the instance.
    let properties = unsafe { instance.enumerate_device_extension_properties(physical) }.ok()?;
    let available: Vec<&CStr> = properties
        .iter()
        .filter_map(|property| property.extension_name_as_c_str().ok())
        .collect();
    let enabled = hal.required_device_extensions(descriptor.required_features);
    let added = extensions_to_add(&available, &enabled)?;
    let mut supported = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default();
    let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut supported);
    // SAFETY: the physical device belongs to the instance, and the chain lives through the call.
    unsafe { instance.get_physical_device_features2(physical, &mut features) };
    if supported.sampler_ycbcr_conversion == vk::FALSE {
        return None;
    }
    let mut conversion =
        vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default().sampler_ycbcr_conversion(true);
    // SAFETY: the callback only adds extensions that the driver lists, and the feature that it
    // supports, and it removes none.
    let opened = unsafe {
        hal.open_with_callback(
            descriptor.required_features,
            &descriptor.required_limits,
            &descriptor.memory_hints,
            Some(Box::new(|args| {
                args.extensions.extend(added);
                *args.create_info = std::mem::take(args.create_info).push_next(&mut conversion);
            })),
        )
    };
    match opened {
        Ok(opened) => Some(opened),
        Err(error) => {
            log::warn!("android video import: no device with the import extensions: {error}");
            None
        }
    }
}
