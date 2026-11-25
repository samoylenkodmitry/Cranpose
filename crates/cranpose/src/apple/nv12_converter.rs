//! Shows a decoded picture on the GPU: the picture's IOSurface planes (8-bit NV12 or 10-bit P010)
//! become Metal textures, and the shared [`BiPlanarConverter`] turns them into RGB in one pass.
//!
//! The planes are imported with no copy. The picture stays alive until the GPU has read it: its
//! frame goes into the drop callback of the planes' textures, so VideoToolbox's pool does not
//! write the IOSurface before then.
#![expect(
    unsafe_code,
    reason = "a picture's CVPixelBuffer and IOSurface are C objects behind a raw pointer, and wgpu's Metal textures are imported unsafely"
)]

use std::sync::Arc;

use cranpose_ui::ImageBitmap;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetIOSurface,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidthOfPlane,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
};

use crate::{
    GpuContext, VideoFrame,
    video_convert::{BiPlanarConverter, BiPlanarFrameMeta, YuvBitDepth},
};

// A plane of the picture: its index in the IOSurface and how Metal and wgpu name its format.
struct Plane {
    index: usize,
    metal: MTLPixelFormat,
    wgpu: wgpu::TextureFormat,
}

const PLANES_8BIT: [Plane; 2] = [
    Plane {
        index: 0,
        metal: MTLPixelFormat::R8Unorm,
        wgpu: wgpu::TextureFormat::R8Unorm,
    },
    Plane {
        index: 1,
        metal: MTLPixelFormat::RG8Unorm,
        wgpu: wgpu::TextureFormat::Rg8Unorm,
    },
];

const PLANES_10BIT: [Plane; 2] = [
    Plane {
        index: 0,
        metal: MTLPixelFormat::R16Unorm,
        wgpu: wgpu::TextureFormat::R16Unorm,
    },
    Plane {
        index: 1,
        metal: MTLPixelFormat::RG16Unorm,
        wgpu: wgpu::TextureFormat::Rg16Unorm,
    },
];

// Converts the pictures of one decoder for one GPU context.
pub(crate) struct Nv12Converter {
    gpu: GpuContext,
    metal: Retained<ProtocolObject<dyn MTLDevice>>,
    converter: BiPlanarConverter,
    supports_16bit_norm: bool,
}

impl Nv12Converter {
    // Makes the converter, or `None` when the context's device is not a Metal device.
    pub(crate) fn new(gpu: &GpuContext) -> Option<Self> {
        // SAFETY: the guard only gives the Metal device's raw device, which this clones.
        let Some(hal) = (unsafe { gpu.device.as_hal::<wgpu::hal::api::Metal>() }) else {
            log::warn!("player surface: the renderer's device is not a Metal device");
            return None;
        };
        let metal = hal.raw_device().clone();
        drop(hal);
        let supports_16bit_norm = gpu
            .device
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
        Some(Self {
            gpu: gpu.clone(),
            metal,
            converter: BiPlanarConverter::new(gpu),
            supports_16bit_norm,
        })
    }

    // Converts `frame` into the next texture of the ring and returns the bitmap that draws it.
    // `None` when the picture cannot be imported, or when the ring has no free texture because
    // the composition has not drawn the pictures that it holds.
    pub(crate) fn convert(&mut self, frame: VideoFrame) -> Option<ImageBitmap> {
        // SAFETY: the pointer is the frame's CVPixelBuffer. The frame moves into the textures
        // below and stays alive until the GPU has read them, which outlives this reference.
        let buffer = unsafe { &*frame.as_pixel_buffer_ptr().cast::<CVPixelBuffer>() };
        if CVPixelBufferGetPlaneCount(buffer) != 2 {
            log::warn!("player surface: a picture that is not bi-planar YUV");
            return None;
        }
        let format = CVPixelBufferGetPixelFormatType(buffer);
        let (planes_desc, bit_depth) = if format == kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
            || format == kCVPixelFormatType_420YpCbCr10BiPlanarFullRange
        {
            if !self.supports_16bit_norm {
                log::warn!("player surface: 10-bit P010 picture needs TEXTURE_FORMAT_16BIT_NORM");
                return None;
            }
            (&PLANES_10BIT, YuvBitDepth::Bit10)
        } else {
            (&PLANES_8BIT, YuvBitDepth::Bit8)
        };
        let surface = CVPixelBufferGetIOSurface(Some(buffer))?;
        let meta = BiPlanarFrameMeta {
            buffer_size: frame.size(),
            crop: frame.crop(),
            color: frame.color(),
            rotation: frame.rotation(),
            bit_depth,
        };
        let frame = Arc::new(frame);
        let planes = planes_desc.each_ref().map(|plane| {
            let width = CVPixelBufferGetWidthOfPlane(buffer, plane.index);
            let height = CVPixelBufferGetHeightOfPlane(buffer, plane.index);
            self.import(&surface, plane, (width, height), &frame)
        });
        let [Some(luma), Some(chroma)] = planes else {
            return None;
        };
        let luma_view = luma.create_view(&Default::default());
        let chroma_view = chroma.create_view(&Default::default());
        self.converter.convert(&luma_view, &chroma_view, meta)
    }

    // Makes a wgpu texture of one plane of `surface`. The texture keeps `frame` alive until the
    // GPU is done with it.
    fn import(
        &self,
        surface: &IOSurfaceRef,
        plane: &Plane,
        size: (usize, usize),
        frame: &Arc<VideoFrame>,
    ) -> Option<wgpu::Texture> {
        let (width, height) = (u32::try_from(size.0).ok()?, u32::try_from(size.1).ok()?);
        // SAFETY: the pixel format and the size are valid for a texture.
        let descriptor = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                plane.metal,
                size.0,
                size.1,
                false,
            )
        };
        descriptor.setUsage(MTLTextureUsage::ShaderRead);
        let texture = self.metal.newTextureWithDescriptor_iosurface_plane(
            &descriptor,
            surface,
            plane.index,
        )?;
        let keep = Arc::clone(frame);
        // SAFETY: the texture is a 2D texture of the format and size given, made by the device
        // that `create_texture_from_hal` runs on. The callback only drops the frame.
        unsafe {
            let hal = wgpu::hal::metal::Device::texture_from_raw(
                texture,
                plane.wgpu,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width,
                    height,
                    depth: 1,
                },
                Some(Box::new(move || drop(keep))),
            );
            Some(
                self.gpu
                    .device
                    .create_texture_from_hal::<wgpu::hal::api::Metal>(
                        hal,
                        &wgpu::TextureDescriptor {
                            label: Some("decoded plane"),
                            size: wgpu::Extent3d {
                                width,
                                height,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: plane.wgpu,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        },
                        wgpu::TextureUses::RESOURCE,
                    ),
            )
        }
    }
}
