//! The GL context an OpenXR session draws with, and Cranpose's wgpu device on it.
//!
//! The session and the device share one EGL context, current on the thread that runs the
//! session. wgpu's GL backend draws on whatever context is current, so the thread keeps this
//! one current while the device lives.
#![expect(
    unsafe_code,
    reason = "EGL, an OpenXR session on a GL context, and wgpu's GL device and textures made from GL names"
)]

use std::{ffi::c_void, num::NonZeroU32, sync::Arc};

use cranpose::GpuContext;
use khronos_egl as egl;
use openxr as xr;

use super::{SwapchainFormat, texture_descriptors::texture_descriptors};

// OpenXR's name for the graphics API of this module.
pub(crate) type Api = xr::OpenGlEs;

// A swapchain image: its GL texture name.
type Image = <Api as xr::Graphics>::SwapchainImage;

// `EGL_OPENGL_ES3_BIT`, which EGL 1.4's constants leave out.
const OPENGL_ES3_BIT: egl::Int = 0x0040;

const GL_SRGB8_ALPHA8: u32 = 0x8C43;
const GL_RGBA8: u32 = 0x8058;

// An EGL context of OpenGL ES 3 with a small pbuffer, current on the thread that made it.
pub(crate) struct Graphics {
    egl: egl::DynamicInstance<egl::EGL1_4>,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
    surface: egl::Surface,
}

impl Graphics {
    // The formats that Cranpose draws, the best first.
    pub(crate) const FORMATS: [SwapchainFormat; 2] = [
        SwapchainFormat {
            api: GL_SRGB8_ALPHA8,
            wgpu: wgpu::TextureFormat::Rgba8UnormSrgb,
        },
        SwapchainFormat {
            api: GL_RGBA8,
            wgpu: wgpu::TextureFormat::Rgba8Unorm,
        },
    ];

    // Whether an image's first row is its top one, which is how wgpu writes it. GL's first row is
    // the bottom one, so the runtime must flip layers that say so.
    pub(crate) const TOP_ROW_FIRST: bool = false;

    // Adds the extension that binds a session to a GL context.
    pub(crate) fn require(
        available: &xr::ExtensionSet,
        extensions: &mut xr::ExtensionSet,
    ) -> Result<(), String> {
        if !available.khr_opengl_es_enable {
            return Err("the OpenXR runtime has no OpenGL ES".into());
        }
        extensions.khr_opengl_es_enable = true;
        Ok(())
    }

    // Makes the context and a wgpu device on it for `system`.
    pub(crate) fn open(
        instance: &xr::Instance,
        system: xr::SystemId,
    ) -> Result<(Self, GpuContext), String> {
        // OpenXR asks for the requirements before a session; GLES 3 meets every runtime's
        instance
            .graphics_requirements::<Api>(system)
            .map_err(|error| format!("OpenXR GLES requirements: {error}"))?;
        let graphics = Self::new()?;
        let gpu = graphics.gpu()?;
        Ok((graphics, gpu))
    }

    // Makes the session that draws on this context. It must end before the context does.
    pub(crate) fn create_session(
        &self,
        instance: &xr::Instance,
        system: xr::SystemId,
    ) -> Result<(xr::Session<Api>, xr::FrameWaiter, xr::FrameStream<Api>), String> {
        // SAFETY: the handles are the current EGL context's, which outlives the session.
        unsafe {
            instance.create_session::<Api>(
                system,
                &xr::opengles::SessionCreateInfo::Android {
                    display: self.display.as_ptr(),
                    config: self.config.as_ptr(),
                    context: self.context.as_ptr(),
                },
            )
        }
        .map_err(|error| format!("OpenXR session: {error}"))
    }

    fn new() -> Result<Self, String> {
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

    // A wgpu device that draws on this context.
    fn gpu(&self) -> Result<GpuContext, String> {
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

    // The GL texture `image`, which another owner such as the OpenXR runtime keeps, as a wgpu
    // texture of `size` and `format` that wgpu never deletes.
    pub(crate) fn wrap_image(
        device: &wgpu::Device,
        image: Image,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> Result<wgpu::Texture, String> {
        let (hal_descriptor, descriptor) = texture_descriptors(size, format, usage);
        let name = NonZeroU32::new(image).ok_or("a swapchain image with no GL name")?;
        // SAFETY: the device is a GL one on the context the texture belongs to. The texture
        // outlives the wgpu texture: its owner deletes it only after the wgpu texture is dropped,
        // and the empty drop callback keeps wgpu from deleting it.
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

    // Readies the image `texture` wraps for the compositor after the commands in `encoder`. GL
    // has no image layouts, so nothing is left to do.
    pub(crate) fn ready_for_release(_encoder: &mut wgpu::CommandEncoder, _texture: &wgpu::Texture) {
    }
}

impl Drop for Graphics {
    fn drop(&mut self) {
        let released = self.egl.make_current(self.display, None, None, None);
        let surface = self.egl.destroy_surface(self.display, self.surface);
        let context = self.egl.destroy_context(self.display, self.context);
        if let Err(error) = released.and(surface).and(context) {
            log::warn!("cranpose xr: EGL teardown: {error}");
        }
    }
}
