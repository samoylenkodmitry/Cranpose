//! Shows a decoded picture on the GPU: the picture's IOSurface planes become Metal textures,
//! and one pass converts the NV12 picture to RGB in a texture that Cranpose draws.
//!
//! The planes are imported with no copy. The picture stays alive until the GPU has read it: its
//! frame goes into the drop callback of the planes' textures, so VideoToolbox's pool does not
//! write the IOSurface before then.
#![expect(
    unsafe_code,
    reason = "a picture's CVPixelBuffer and IOSurface are C objects behind a raw pointer, and wgpu's Metal textures are imported unsafely"
)]

use std::{ptr, sync::Arc};

use cranpose_ui::ImageBitmap;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_core_foundation::{CFRetained, CFString, CFType};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetIOSurface,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidthOfPlane,
    kCVImageBufferYCbCrMatrixKey, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
};

use crate::{ExternalFrames, FrameWriter, GpuContext, VideoFrame, cover_pipeline::cover_pipeline};

// A plane of the picture: its index in the IOSurface and how Metal and wgpu name its format.
struct Plane {
    index: usize,
    metal: MTLPixelFormat,
    wgpu: wgpu::TextureFormat,
}

// The luma, then the blue and red differences interleaved at half the size.
const PLANES: [Plane; 2] = [
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

// The bytes of the shader's `Conversion`: eight floats.
const UNIFORM_SIZE: usize = 32;

// Converts the pictures of one decoder for one GPU context.
pub(crate) struct Nv12Converter {
    gpu: GpuContext,
    metal: Retained<ProtocolObject<dyn MTLDevice>>,
    frames: ExternalFrames,
    writer: FrameWriter,
    size: (u32, u32),
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    // The matrix that the last picture's attachment named, kept with that attachment: a stream's
    // pictures carry equal attachments
    matrix: Option<(CFRetained<CFType>, Option<Matrix>)>,
}

impl Nv12Converter {
    // Makes the converter, or `None` when the context's device is not a Metal device.
    pub(crate) fn new(gpu: &GpuContext) -> Option<Self> {
        // SAFETY: the guard only gives the Metal device's raw device, which this clones.
        let metal = unsafe { gpu.device.as_hal::<wgpu::hal::api::Metal>() }?
            .raw_device()
            .clone();
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nv12 to rgb"),
            source: wgpu::ShaderSource::Wgsl(include_str!("nv12_to_rgb.wgsl").into()),
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nv12 to rgb"),
            entries: &[
                texture(0),
                texture(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE as u64),
                    },
                    count: None,
                },
            ],
        });
        let pipeline = cover_pipeline(
            device,
            "nv12 to rgb",
            &layout,
            &module,
            "convert",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nv12 to rgb"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nv12 to rgb"),
            size: UNIFORM_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frames = ExternalFrames::new(
            gpu,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            true,
        );
        Some(Self {
            gpu: gpu.clone(),
            metal,
            writer: frames.writer(),
            frames,
            size: (0, 0),
            pipeline,
            layout,
            sampler,
            uniforms,
            matrix: None,
        })
    }

    // Converts `frame` into the next texture of the ring and returns the bitmap that draws it.
    // `None` when the picture cannot be imported, or when the ring has no free texture because
    // the composition has not drawn the pictures that it holds.
    pub(crate) fn convert(&mut self, frame: VideoFrame) -> Option<ImageBitmap> {
        let size = frame.size();
        if size != self.size {
            self.frames.set_size(size.0, size.1);
            self.size = size;
        }
        // SAFETY: the pointer is the frame's CVPixelBuffer. The frame moves into the textures
        // below and stays alive until the GPU has read them, which outlives this reference.
        let buffer = unsafe { &*frame.as_pixel_buffer_ptr().cast::<CVPixelBuffer>() };
        if CVPixelBufferGetPlaneCount(buffer) != PLANES.len() {
            log::warn!("player surface: a picture that is not NV12");
            return None;
        }
        let surface = CVPixelBufferGetIOSurface(Some(buffer))?;
        let conversion = Conversion::of(buffer, size.1, &mut self.matrix);
        let target = self.writer.begin()?;
        let frame = Arc::new(frame);
        let planes = PLANES.each_ref().map(|plane| {
            let width = CVPixelBufferGetWidthOfPlane(buffer, plane.index);
            let height = CVPixelBufferGetHeightOfPlane(buffer, plane.index);
            self.import(&surface, plane, (width, height), &frame)
        });
        let [Some(luma), Some(chroma)] = planes else {
            return None;
        };
        let views = [&luma, &chroma].map(|texture| texture.create_view(&Default::default()));
        let device = &self.gpu.device;
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nv12 to rgb"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.uniforms.as_entire_binding(),
                },
            ],
        });
        self.gpu
            .queue
            .write_buffer(&self.uniforms, 0, &conversion.to_bytes());
        let mut commands = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nv12 to rgb"),
        });
        {
            let mut pass = commands.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nv12 to rgb"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit([commands.finish()]);
        self.writer.publish(target.number());
        self.frames.next_frame()
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

// How a picture's values become RGB: the offsets and scales of its range, and the weights of
// its color matrix.
struct Conversion {
    range: [f32; 4],
    weights: [f32; 4],
}

impl Conversion {
    // The conversion of `buffer`, a picture `height` rows high. The picture's attachment names
    // the matrix. Without a name that is known, a picture of 720 rows or more takes BT.709 and a
    // smaller one BT.601, as players do.
    fn of(
        buffer: &CVPixelBuffer,
        height: u32,
        cache: &mut Option<(CFRetained<CFType>, Option<Matrix>)>,
    ) -> Self {
        let full = CVPixelBufferGetPixelFormatType(buffer)
            == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange;
        let matrix = matrix_of(buffer, cache).unwrap_or(if height >= 720 {
            Matrix::Bt709
        } else {
            Matrix::Bt601
        });
        let (kr, kb) = match matrix {
            Matrix::Bt601 => (0.299, 0.114),
            Matrix::Bt709 => (0.2126, 0.0722),
            Matrix::Bt2020 => (0.2627, 0.0593),
        };
        let kg = 1.0 - kr - kb;
        let (red, blue) = (2.0 * (1.0 - kr), 2.0 * (1.0 - kb));
        Self {
            // The video range holds luma from 16 to 235 and differences from 16 to 240 of 255.
            range: if full {
                [0.0, 1.0, 1.0, 0.0]
            } else {
                [16.0 / 255.0, 255.0 / 219.0, 255.0 / 224.0, 0.0]
            },
            weights: [red, -kb * blue / kg, -kr * red / kg, blue],
        }
    }

    fn to_bytes(&self) -> [u8; UNIFORM_SIZE] {
        let mut bytes = [0; UNIFORM_SIZE];
        let (words, _) = bytes.as_chunks_mut::<4>();
        for (word, value) in words
            .iter_mut()
            .zip(self.range.into_iter().chain(self.weights))
        {
            *word = value.to_ne_bytes();
        }
        bytes
    }
}

#[derive(Clone, Copy)]
enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Matrix {
    // The matrix that an attachment names. CoreVideo names the three common ones, and writes
    // another matrix of H.273 as `YCbCrMatrix#` and its number, such as 5 for BT.601 of 625
    // lines.
    fn named(attachment: &CFType) -> Option<Self> {
        let name = attachment.downcast_ref::<CFString>()?.to_string();
        match name.as_str() {
            "ITU_R_709_2" => Some(Self::Bt709),
            "ITU_R_601_4" => Some(Self::Bt601),
            "ITU_R_2020" => Some(Self::Bt2020),
            other => match other.strip_prefix("YCbCrMatrix#")?.parse::<u8>().ok()? {
                1 => Some(Self::Bt709),
                5 | 6 => Some(Self::Bt601),
                9 | 10 => Some(Self::Bt2020),
                _ => None,
            },
        }
    }
}

// The matrix of `buffer`, or `None` for a picture with no attachment or a matrix that is not
// known.
fn matrix_of(
    buffer: &CVPixelBuffer,
    cache: &mut Option<(CFRetained<CFType>, Option<Matrix>)>,
) -> Option<Matrix> {
    // SAFETY: the key is a constant of CoreVideo, and the attachment mode is not read.
    let attachment: CFRetained<CFType> =
        unsafe { buffer.attachment(kCVImageBufferYCbCrMatrixKey, ptr::null_mut()) }?;
    if let Some((known, matrix)) = cache
        && **known == *attachment
    {
        return *matrix;
    }
    let matrix = Matrix::named(&attachment);
    *cache = Some((attachment, matrix));
    matrix
}
