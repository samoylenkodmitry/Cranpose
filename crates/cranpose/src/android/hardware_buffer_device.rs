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

use ash::vk;
use wgpu::hal::api::Vulkan;

use super::hardware_buffer_extensions::extensions_to_add;

// The device that `descriptor` names, opened with the import extensions, or `None` where `adapter`
// is not a Vulkan adapter whose driver has them.
pub(crate) fn request_device(
    adapter: &wgpu::Adapter,
    descriptor: &wgpu::DeviceDescriptor<'_>,
) -> Option<(wgpu::Device, wgpu::Queue)> {
    let opened = open_importing(adapter, descriptor)?;
    // SAFETY: the device was opened on this adapter with the features and the limits that the
    // descriptor names.
    unsafe { adapter.create_device_from_hal(opened, descriptor) }
        .map_err(|error| log::warn!("android video import: wgpu took no device: {error}"))
        .ok()
}

fn open_importing(
    adapter: &wgpu::Adapter,
    descriptor: &wgpu::DeviceDescriptor<'_>,
) -> Option<wgpu::hal::OpenDevice<Vulkan>> {
    let Some(hal) = (unsafe { adapter.as_hal::<Vulkan>() }) else {
        log::warn!(
            "android video import: adapter {:?} is not Vulkan",
            adapter.get_info().backend
        );
        return None;
    };
    // wgpu-hal listed the driver's extensions when it made the adapter.
    let driver = hal.physical_device_capabilities();
    let enabled = hal.required_device_extensions(descriptor.required_features);
    let Some(added) = extensions_to_add(|name| driver.supports_extension(name), &enabled) else {
        log::warn!(
            "android video import: adapter {} lacks required AHardwareBuffer Vulkan extensions",
            adapter.get_info().name
        );
        return None;
    };
    let instance = hal.shared_instance().raw_instance();
    let physical = hal.raw_physical_device();
    // wgpu-hal reads this feature when it makes the adapter, but keeps the answer private.
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
