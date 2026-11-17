//! A Vulkan device that OpenXR creates for a headset's session, and Cranpose's wgpu device on it.
//!
//! With `XR_KHR_vulkan_enable2` the runtime creates the Vulkan instance and the device, so that
//! they suit the headset's GPU and the compositor. wgpu wraps the two objects and makes none of
//! its own. The swapchain's images are Vulkan images that wgpu wraps as textures.
#![expect(
    unsafe_code,
    reason = "the Vulkan loader, an OpenXR session on a Vulkan device, and wgpu's Vulkan device and textures made from raw handles"
)]

use std::{
    ffi::{CStr, c_char, c_void},
    iter, mem, ptr,
    sync::Arc,
};

use ash::vk::{self, Handle};
use cranpose::GpuContext;
use openxr as xr;
use wgpu::{
    hal::{self, vulkan},
    wgt::{TextureTransition, TextureUses},
};

use super::{SwapchainFormat, texture_descriptors::texture_descriptors};

// OpenXR's name for the graphics API of this module.
pub(crate) type Api = xr::Vulkan;

// A swapchain image: its Vulkan image.
type Image = <Api as xr::Graphics>::SwapchainImage;

// The newest Vulkan version that wgpu supports, and the oldest.
const NEWEST: u32 = vk::API_VERSION_1_3;
const OLDEST: u32 = vk::API_VERSION_1_1;

// The Vulkan objects of the session. wgpu owns them and destroys them with its last device, so
// the session ends before the runner lets the device go.
pub(crate) struct Graphics {
    binding: xr::vulkan::SessionCreateInfo,
}

// What OpenXR made of the Vulkan instance, in wgpu-hal's form.
struct Instance {
    raw: ash::Instance,
    hal: vulkan::Instance,
}

impl Graphics {
    // The formats that Cranpose draws, the best first.
    pub(crate) const FORMATS: [SwapchainFormat; 4] = [
        format(
            vk::Format::R8G8B8A8_SRGB,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ),
        format(
            vk::Format::B8G8R8A8_SRGB,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ),
        format(vk::Format::R8G8B8A8_UNORM, wgpu::TextureFormat::Rgba8Unorm),
        format(vk::Format::B8G8R8A8_UNORM, wgpu::TextureFormat::Bgra8Unorm),
    ];

    // Whether an image's first row is its top one, which is how wgpu writes it. Vulkan's is.
    pub(crate) const TOP_ROW_FIRST: bool = true;

    // Adds the extension that lets the runtime make the Vulkan instance and device.
    pub(crate) fn require(
        available: &xr::ExtensionSet,
        extensions: &mut xr::ExtensionSet,
    ) -> Result<(), String> {
        if !available.khr_vulkan_enable2 {
            return Err("the OpenXR runtime has no XR_KHR_vulkan_enable2".into());
        }
        extensions.khr_vulkan_enable2 = true;
        Ok(())
    }

    // Has the runtime make a Vulkan instance and device for `system`, and wraps them in a wgpu
    // device.
    pub(crate) fn open(
        instance: &xr::Instance,
        system: xr::SystemId,
    ) -> Result<(Self, GpuContext), String> {
        // SAFETY: the library is the system's Vulkan loader, which the entry keeps loaded.
        let entry =
            unsafe { ash::Entry::load() }.map_err(|error| format!("the Vulkan loader: {error}"))?;
        let version = api_version(instance, system, &entry)?;
        let created = create_instance(instance, system, &entry, version)?;
        let physical = physical_device(instance, system, &created.raw)?;
        let exposed = created
            .hal
            .expose_adapter(physical)
            .ok_or("wgpu has no adapter for the headset's GPU")?;
        let family = graphics_family(&created.raw, physical)?;
        let (device, handle) = create_device(
            instance,
            system,
            &entry,
            &created.raw,
            &exposed,
            (physical, family),
        )?;
        let binding = xr::vulkan::SessionCreateInfo {
            instance: pointer(created.raw.handle().as_raw())?,
            physical_device: pointer(physical.as_raw())?,
            device: pointer(handle.as_raw())?,
            queue_family_index: family,
            queue_index: 0,
        };
        let gpu = wrap(created.hal, exposed, device)?;
        Ok((Self { binding }, gpu))
    }

    // Makes the session that draws on the device. It must end before the device does.
    pub(crate) fn create_session(
        &self,
        instance: &xr::Instance,
        system: xr::SystemId,
    ) -> Result<(xr::Session<Api>, xr::FrameWaiter, xr::FrameStream<Api>), String> {
        // SAFETY: the handles are those that OpenXR made the instance and the device with. wgpu
        // keeps them alive until its last device drops, which the runner delays until the
        // session has ended.
        unsafe { instance.create_session::<Api>(system, &self.binding) }
            .map_err(|error| format!("OpenXR session: {error}"))
    }

    // The Vulkan image `image`, which the OpenXR runtime keeps, as a wgpu texture of `size` and
    // `format` that wgpu never destroys. The image is a render attachment too, because
    // `ready_for_release` returns it to that layout.
    pub(crate) fn wrap_image(
        device: &wgpu::Device,
        image: Image,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> Result<wgpu::Texture, String> {
        let usage = usage | wgpu::TextureUsages::RENDER_ATTACHMENT;
        let (hal_descriptor, descriptor) = texture_descriptors(size, format, usage);
        // SAFETY: the device is a Vulkan one, which the image belongs to. The runtime destroys
        // the image only after the wgpu texture is dropped, and the empty drop callback keeps
        // wgpu from destroying it. The image's memory is the runtime's too.
        unsafe {
            let hal_device = device
                .as_hal::<hal::api::Vulkan>()
                .ok_or("not a Vulkan device")?;
            let raw = hal_device.texture_from_raw(
                vk::Image::from_raw(image),
                &hal_descriptor,
                Some(Box::new(|| {})),
                vulkan::TextureMemory::External,
            );
            drop(hal_device);
            Ok(device.create_texture_from_hal::<hal::api::Vulkan>(
                raw,
                &descriptor,
                TextureUses::UNINITIALIZED,
            ))
        }
    }

    // Puts the image that `texture` wraps in the layout that the runtime expects when the app
    // releases it, which is the color attachment's, after the commands in `encoder`.
    pub(crate) fn ready_for_release(encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture) {
        encoder.transition_resources(
            iter::empty(),
            iter::once(TextureTransition {
                texture,
                selector: None,
                state: TextureUses::COLOR_TARGET,
            }),
        );
    }
}

const fn format(vulkan: vk::Format, wgpu: wgpu::TextureFormat) -> SwapchainFormat {
    SwapchainFormat {
        // Vulkan's format numbers are never negative.
        api: vulkan.as_raw().unsigned_abs(),
        wgpu,
    }
}

// The Vulkan version to make the instance with: the newest that wgpu and the loader take. The
// runtime's newest version is the one it was tested on, and a newer one may work, so only its
// oldest version and its major version limit the choice.
fn api_version(
    instance: &xr::Instance,
    system: xr::SystemId,
    entry: &ash::Entry,
) -> Result<u32, String> {
    let requirements = instance
        .graphics_requirements::<Api>(system)
        .map_err(|error| format!("OpenXR Vulkan requirements: {error}"))?;
    // SAFETY: asking for the loader's version needs no instance.
    let loader = unsafe { entry.try_enumerate_instance_version() }
        .map_err(|error| format!("Vulkan loader version: {error}"))?
        .unwrap_or(vk::API_VERSION_1_0);
    let version = loader.min(NEWEST);
    let oldest = requirements.min_api_version_supported;
    let oldest = vk::make_api_version(0, oldest.major().into(), oldest.minor().into(), 0);
    let major = u32::from(requirements.max_api_version_supported.major());
    if version < OLDEST || version < oldest || vk::api_version_major(version) > major {
        return Err(format!(
            "the OpenXR runtime takes Vulkan {} to {}, and the Vulkan loader offers {}.{}",
            requirements.min_api_version_supported,
            requirements.max_api_version_supported,
            vk::api_version_major(loader),
            vk::api_version_minor(loader),
        ));
    }
    Ok(version)
}

// OpenXR's `xrGetInstanceProcAddr`-style function that finds Vulkan's entry points.
fn proc_address(entry: &ash::Entry) -> xr::sys::platform::VkGetInstanceProcAddr {
    // SAFETY: the two function types differ only in the type of the instance handle: ash's
    // opaque handle and OpenXR's pointer have the same size and passing convention.
    unsafe {
        mem::transmute::<vk::PFN_vkGetInstanceProcAddr, xr::sys::platform::VkGetInstanceProcAddr>(
            entry.static_fn().get_instance_proc_addr,
        )
    }
}

// OpenXR's header names a Vulkan handle as a pointer, and ash as a number.
fn pointer(handle: u64) -> Result<*const c_void, String> {
    let address = usize::try_from(handle).map_err(|error| format!("a Vulkan handle: {error}"))?;
    Ok(ptr::with_exposed_provenance(address))
}

// Has the runtime make the Vulkan instance with the extensions that wgpu wants.
fn create_instance(
    instance: &xr::Instance,
    system: xr::SystemId,
    entry: &ash::Entry,
    version: u32,
) -> Result<Instance, String> {
    let flags = wgpu::InstanceFlags::empty();
    let extensions = vulkan::Instance::desired_extensions(entry, version, flags)
        .map_err(|error| format!("Vulkan instance extensions: {error}"))?;
    let names: Vec<*const c_char> = extensions.iter().map(|name| name.as_ptr()).collect();
    let application = vk::ApplicationInfo::default()
        .application_name(c"cranpose")
        .engine_name(c"cranpose")
        .api_version(version);
    let info = vk::InstanceCreateInfo::default()
        .application_info(&application)
        .enabled_extension_names(&names);
    // SAFETY: the info and what it points to live through the call, and OpenXR's requirements
    // were asked for before.
    let handle = unsafe {
        instance.create_vulkan_instance(system, proc_address(entry), ptr::from_ref(&info).cast())
    }
    .map_err(|error| format!("OpenXR Vulkan instance: {error}"))?
    .map_err(|result| format!("Vulkan instance: {:?}", vk::Result::from_raw(result)))?;
    let handle = vk::Instance::from_raw(handle.addr() as u64);
    // SAFETY: `handle` is the instance just made, and `entry` is the loader it was made with.
    let raw = unsafe { ash::Instance::load(entry.static_fn(), handle) };
    // SAFETY: the layer list needs no live instance.
    let has_nv_optimus =
        unsafe { entry.enumerate_instance_layer_properties() }.is_ok_and(|layers| {
            layers
                .iter()
                .any(|layer| layer.layer_name_as_c_str() == Ok(c"VK_LAYER_NV_optimus"))
        });
    // SAFETY: the instance was made with `version` and `extensions`, which are what wgpu-hal
    // wants. wgpu-hal owns it from here on, so the drop callback is none.
    let hal = unsafe {
        vulkan::Instance::from_raw(
            entry.clone(),
            raw.clone(),
            version,
            0,
            None,
            extensions,
            flags,
            wgpu::MemoryBudgetThresholds::default(),
            has_nv_optimus,
            None,
        )
    }
    .map_err(|error| format!("wgpu Vulkan instance: {error}"))?;
    Ok(Instance { raw, hal })
}

// The GPU that the headset is attached to.
fn physical_device(
    instance: &xr::Instance,
    system: xr::SystemId,
    raw: &ash::Instance,
) -> Result<vk::PhysicalDevice, String> {
    // SAFETY: the Vulkan instance is the one OpenXR made.
    let physical =
        unsafe { instance.vulkan_graphics_device(system, pointer(raw.handle().as_raw())?) }
            .map_err(|error| format!("OpenXR Vulkan GPU: {error}"))?;
    Ok(vk::PhysicalDevice::from_raw(physical.addr() as u64))
}

// The first queue family that draws, which the session submits to.
fn graphics_family(raw: &ash::Instance, physical: vk::PhysicalDevice) -> Result<u32, String> {
    // SAFETY: the physical device belongs to the instance.
    let families = unsafe { raw.get_physical_device_queue_family_properties(physical) };
    families
        .iter()
        .position(|family| family.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| "the headset's GPU has no graphics queue".into())
}

// Has the runtime make the Vulkan device on `exposed`'s GPU with one queue of `family`, and
// opens wgpu-hal's device on it. Returns the device's handle too, which the session needs.
fn create_device(
    instance: &xr::Instance,
    system: xr::SystemId,
    entry: &ash::Entry,
    raw: &ash::Instance,
    exposed: &hal::ExposedAdapter<vulkan::Api>,
    (physical, family): (vk::PhysicalDevice, u32),
) -> Result<(hal::OpenDevice<vulkan::Api>, vk::Device), String> {
    // Cranpose's renderer draws on a device without optional features.
    let features = wgpu::Features::empty();
    let extensions: Vec<&'static CStr> = exposed.adapter.required_device_extensions(features);
    let names: Vec<*const c_char> = extensions.iter().map(|name| name.as_ptr()).collect();
    let mut enabled = exposed
        .adapter
        .physical_device_features(&extensions, features);
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&[1.0])];
    let info = enabled
        .add_to_device_create(vk::DeviceCreateInfo::default().queue_create_infos(&queues))
        .enabled_extension_names(&names);
    // SAFETY: the info and what it points to live through the call, and the GPU is the one that
    // OpenXR chose.
    let handle = unsafe {
        instance.create_vulkan_device(
            system,
            proc_address(entry),
            pointer(physical.as_raw())?,
            ptr::from_ref(&info).cast(),
        )
    }
    .map_err(|error| format!("OpenXR Vulkan device: {error}"))?
    .map_err(|result| format!("Vulkan device: {:?}", vk::Result::from_raw(result)))?;
    let handle = vk::Device::from_raw(handle.addr() as u64);
    // SAFETY: `handle` is the device just made on this instance.
    let device = unsafe { ash::Device::load(raw.fp_v1_0(), handle) };
    // SAFETY: the device was made on this adapter's GPU with its required extensions and
    // features, and one queue of `family`. wgpu-hal owns it from here on.
    let opened = unsafe {
        exposed.adapter.device_from_raw(
            device,
            None,
            &extensions,
            features,
            &exposed.capabilities.limits,
            &wgpu::MemoryHints::Performance,
            family,
            0,
        )
    }
    .map_err(|error| format!("wgpu Vulkan device: {error}"))?;
    Ok((opened, handle))
}

// The wgpu instance, adapter, device and queue around what OpenXR made.
fn wrap(
    instance: vulkan::Instance,
    adapter: hal::ExposedAdapter<vulkan::Api>,
    device: hal::OpenDevice<vulkan::Api>,
) -> Result<GpuContext, String> {
    // SAFETY: the instance is a Vulkan one that wgpu takes over.
    let instance = unsafe { wgpu::Instance::from_hal::<hal::api::Vulkan>(instance) };
    // SAFETY: the adapter came from the instance's own Vulkan instance.
    let adapter = unsafe { instance.create_adapter_from_hal(adapter) };
    // SAFETY: the device was opened on this adapter, and it has no features.
    let (device, queue) = unsafe {
        adapter.create_device_from_hal(
            device,
            &wgpu::DeviceDescriptor {
                label: Some("cranpose xr"),
                required_limits: adapter.limits(),
                ..Default::default()
            },
        )
    }
    .map_err(|error| format!("wgpu device: {error}"))?;
    Ok(GpuContext {
        instance,
        adapter,
        device: Arc::new(device),
        queue: Arc::new(queue),
    })
}
