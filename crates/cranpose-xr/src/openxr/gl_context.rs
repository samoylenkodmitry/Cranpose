//! The GL context an OpenXR session draws with, and Cranpose's wgpu device on it.
//!
//! The session and the device share one EGL context, current on the thread that runs the
//! session. wgpu's GL backend draws on whatever context is current, so the thread keeps this
//! one current while the device lives.
#![expect(
    unsafe_code,
    reason = "EGL, and wgpu's GL device and textures made from GL names"
)]

use std::{ffi::c_void, num::NonZeroU32, sync::Arc};

use cranpose::GpuContext;
use khronos_egl as egl;

// `EGL_OPENGL_ES3_BIT`, which EGL 1.4's constants leave out.
const OPENGL_ES3_BIT: egl::Int = 0x0040;

// An EGL context of OpenGL ES 3 with a small pbuffer, current on the thread that made it.
pub(crate) struct GlContext {
    egl: egl::DynamicInstance<egl::EGL1_4>,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
    surface: egl::Surface,
}

impl GlContext {
    pub(crate) fn new() -> Result<Self, String> {
        // SAFETY: loads the system's libEGL, which every Android device has, and keeps it
        // loaded while the instance lives.
        let egl = unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required() }
            .map_err(|error| format!("EGL: {error}"))?;
        // SAFETY: the default display needs no native display handle.
        let display = unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.ok_or("no EGL display")?;
        egl.initialize(display)
            .map_err(|error| format!("EGL initialize: {error}"))?;
        let attributes = [
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::DEPTH_SIZE,
            0,
            egl::RENDERABLE_TYPE,
            OPENGL_ES3_BIT,
            egl::SURFACE_TYPE,
            egl::PBUFFER_BIT,
            egl::NONE,
        ];
        let config = egl
            .choose_first_config(display, &attributes)
            .map_err(|error| format!("EGL config: {error}"))?
            .ok_or("no EGL config for OpenGL ES 3")?;
        let context = egl
            .create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE],
            )
            .map_err(|error| format!("EGL context: {error}"))?;
        let surface = egl
            .create_pbuffer_surface(
                display,
                config,
                &[egl::WIDTH, 16, egl::HEIGHT, 16, egl::NONE],
            )
            .map_err(|error| format!("EGL pbuffer: {error}"))?;
        egl.make_current(display, Some(surface), Some(surface), Some(context))
            .map_err(|error| format!("EGL make current: {error}"))?;
        Ok(Self {
            egl,
            display,
            config,
            context,
            surface,
        })
    }

    // The display, config and context as the raw handles OpenXR's GLES binding takes.
    pub(crate) fn handles(&self) -> (*mut c_void, *mut c_void, *mut c_void) {
        (
            self.display.as_ptr(),
            self.config.as_ptr(),
            self.context.as_ptr(),
        )
    }

    // The address of the GL or EGL function `name`, or `None` if the driver lacks it.
    pub(crate) fn proc_address(&self, name: &str) -> Option<*const c_void> {
        self.egl
            .get_proc_address(name)
            .map(|function| function as *const c_void)
    }

    // A wgpu device that draws on this context.
    pub(crate) fn gpu(&self) -> Result<GpuContext, String> {
        let lookup = |name: &str| {
            self.egl
                .get_proc_address(name)
                .map_or(std::ptr::null(), |function| function as *const c_void)
        };
        // SAFETY: the context is current on this thread, and the runner keeps it current there
        // for as long as the device lives.
        let hal = unsafe {
            wgpu::hal::gles::Adapter::new_external(lookup, wgpu::GlBackendOptions::default())
        }
        .ok_or("no GL adapter on the EGL context")?;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        // SAFETY: the adapter is a GL one, a backend the instance was made for.
        let adapter = unsafe { instance.create_adapter_from_hal(hal) };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("cranpose xr"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|error| format!("wgpu device: {error}"))?;
        Ok(GpuContext {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }
}

impl Drop for GlContext {
    fn drop(&mut self) {
        let released = self.egl.make_current(self.display, None, None, None);
        let surface = self.egl.destroy_surface(self.display, self.surface);
        let context = self.egl.destroy_context(self.display, self.context);
        if let Err(error) = released.and(surface).and(context) {
            log::warn!("cranpose xr: EGL teardown: {error}");
        }
    }
}

// The GL texture `name`, which another owner such as the OpenXR runtime keeps, as a wgpu
// texture of `size` and `format` that wgpu never deletes.
pub(crate) fn wrap_texture(
    device: &wgpu::Device,
    name: u32,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> Result<wgpu::Texture, String> {
    let size = wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    let mut uses = wgpu::wgt::TextureUses::empty();
    for (usage_bit, use_bit) in [
        (
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            wgpu::wgt::TextureUses::COLOR_TARGET,
        ),
        (
            wgpu::TextureUsages::COPY_DST,
            wgpu::wgt::TextureUses::COPY_DST,
        ),
        (
            wgpu::TextureUsages::COPY_SRC,
            wgpu::wgt::TextureUses::COPY_SRC,
        ),
    ] {
        if usage.contains(usage_bit) {
            uses |= use_bit;
        }
    }
    let hal_descriptor = wgpu::hal::TextureDescriptor {
        label: Some("cranpose xr swapchain image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: uses,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };
    let descriptor = wgpu::TextureDescriptor {
        label: Some("cranpose xr swapchain image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    };
    let name = NonZeroU32::new(name).ok_or("a swapchain image with no GL name")?;
    // SAFETY: the device is a GL one on the context the texture belongs to. The texture outlives
    // the wgpu texture: its owner deletes it only after the wgpu texture is dropped, and the
    // empty drop callback keeps wgpu from deleting it.
    unsafe {
        let hal_device = device
            .as_hal::<wgpu::hal::api::Gles>()
            .ok_or("not a GL device")?;
        let raw = hal_device.texture_from_raw(name, &hal_descriptor, Some(Box::new(|| {})));
        drop(hal_device);
        Ok(device.create_texture_from_hal::<wgpu::hal::api::Gles>(
            raw,
            &descriptor,
            wgpu::wgt::TextureUses::UNINITIALIZED,
        ))
    }
}
