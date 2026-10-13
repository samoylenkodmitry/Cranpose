//! Cranpose on Godot's Metal device.

use cranpose::GpuContext;
use godot::{
    classes::{
        RenderingDevice, RenderingServer,
        rendering_device::{DataFormat, DriverResource},
    },
    prelude::*,
};

pub use crate::metal::Alpha;
use crate::{
    InteropError, NativeTexture,
    metal::{self, EngineTexture, QueueBridge},
};

/// Cranpose's GPU context on Godot's `MTLDevice`, with a queue of its own,
/// and the bridge that hands textures between that queue and Godot's.
pub struct GodotGpu {
    gpu: GpuContext,
    bridge: QueueBridge,
}

/// A texture of Godot's rendering device, with its size and format.
pub struct GodotTexture {
    raw: EngineTexture,
    native: NativeTexture,
}

/// The value Cranpose's frame signals when it is done.
#[must_use]
pub struct FrameTicket(u64);

impl GodotGpu {
    /// Builds wgpu objects on the device `rd` renders with. Godot must
    /// render with Metal.
    pub fn new(rd: &mut Gd<RenderingDevice>) -> Result<Self, InteropError> {
        let driver = RenderingServer::singleton()
            .get_current_rendering_driver_name()
            .to_string();
        if driver != "metal" {
            return Err(InteropError::UnsupportedDriver(driver));
        }
        let device = rd.get_driver_resource(DriverResource::LOGICAL_DEVICE, Rid::Invalid, 0);
        let queue = rd.get_driver_resource(DriverResource::COMMAND_QUEUE, Rid::Invalid, 0);
        if device == 0 || queue == 0 {
            return Err(InteropError::MissingHandle("Godot device"));
        }
        // SAFETY: Godot's Metal driver returns its live MTLDevice and the
        // MTLCommandQueue it submits to; both are retained.
        let (gpu, bridge) = unsafe { metal::gpu_context(device, queue) }?;
        Ok(Self { gpu, bridge })
    }

    /// The context Cranpose renders with.
    pub fn context(&self) -> &GpuContext {
        &self.gpu
    }

    /// `texture`, a texture of `rd`.
    pub fn texture(
        &self,
        rd: &mut Gd<RenderingDevice>,
        texture: Rid,
    ) -> Result<GodotTexture, InteropError> {
        let format = rd
            .texture_get_format(texture)
            .ok_or(InteropError::MissingHandle("texture format"))?;
        let handle = rd.get_driver_resource(DriverResource::TEXTURE, texture, 0);
        if handle == 0 {
            return Err(InteropError::MissingHandle("texture"));
        }
        // SAFETY: the handle is the live MTLTexture Godot's driver returns for
        // `texture`; it is retained.
        let raw = unsafe { metal::engine_texture(handle) }?;
        Ok(GodotTexture {
            raw,
            native: NativeTexture {
                width: format.get_width(),
                height: format.get_height(),
                format: texture_format(format.get_format())?,
            },
        })
    }

    /// A wgpu texture with the size and format of `texture`, which Cranpose
    /// renders into or samples.
    pub fn twin(&self, texture: &GodotTexture) -> wgpu::Texture {
        metal::twin_texture(&self.gpu, texture.native)
    }

    /// Copies `from`, as Godot's queue last wrote it, into `into`. Cranpose's
    /// next submit waits for the copy, and the copy waits for Cranpose's last
    /// frame.
    pub fn receive(&self, from: &GodotTexture, into: &wgpu::Texture, alpha: Alpha) {
        self.bridge.receive(&self.gpu, &from.raw, into, alpha);
    }

    /// Call before Cranpose draws a frame.
    pub fn begin_frame(&self) -> FrameTicket {
        FrameTicket(self.bridge.begin_frame(&self.gpu))
    }

    /// Call after Cranpose drew a frame into `from`, or found nothing to draw.
    /// A drawn frame is copied into `into` on Godot's queue once it is done,
    /// before Godot's next frame reads `into`.
    pub fn finish_frame(
        &self,
        ticket: FrameTicket,
        drew: bool,
        from: &wgpu::Texture,
        into: &GodotTexture,
    ) {
        self.bridge
            .finish_frame(&self.gpu, ticket.0, drew, from, &into.raw);
    }
}

impl GodotTexture {
    /// Size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.native.width, self.native.height)
    }
}

fn texture_format(format: DataFormat) -> Result<wgpu::TextureFormat, InteropError> {
    Ok(match format {
        DataFormat::R8G8B8A8_UNORM => wgpu::TextureFormat::Rgba8Unorm,
        DataFormat::R8G8B8A8_SRGB => wgpu::TextureFormat::Rgba8UnormSrgb,
        DataFormat::B8G8R8A8_UNORM => wgpu::TextureFormat::Bgra8Unorm,
        DataFormat::B8G8R8A8_SRGB => wgpu::TextureFormat::Bgra8UnormSrgb,
        DataFormat::R16G16B16A16_SFLOAT => wgpu::TextureFormat::Rgba16Float,
        DataFormat::A2B10G10R10_UNORM_PACK32 => wgpu::TextureFormat::Rgb10a2Unorm,
        other => return Err(InteropError::UnsupportedFormat(format!("{other:?}"))),
    })
}
