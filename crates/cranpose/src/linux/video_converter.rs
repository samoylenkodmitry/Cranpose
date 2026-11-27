//! Imports Linux VA-API DMABUF planes into wgpu Vulkan with zero CPU copy, and supports explicit
//! software bi-planar YUV upload when VA-API hardware decode or DMABUF import is unavailable.
#![expect(
    unsafe_code,
    reason = "DMABUF file descriptor duplication and wgpu-hal Vulkan import use OS/Vulkan FFI"
)]

use std::os::unix::io::{FromRawFd, OwnedFd};

use cranpose_services::{
    DmaBufFrame, DmaBufPlane, LinuxFrameMemory, LinuxPixelFormat, SoftwareBiPlanarPlanes,
};
use cranpose_ui::ImageBitmap;

use crate::{
    GpuContext, VideoFrame,
    video_convert::{BiPlanarConverter, BiPlanarFrameMeta, YuvBitDepth},
};

struct CachedSoftwarePlanes {
    size: (u32, u32),
    format: LinuxPixelFormat,
    luma_tex: wgpu::Texture,
    chroma_tex: wgpu::Texture,
    luma_view: wgpu::TextureView,
    chroma_view: wgpu::TextureView,
}

// Converts Linux VA-API DMABUF (zero-copy) or software bi-planar frames into RGBA8 `ImageBitmap`s.
pub(crate) struct LinuxVideoConverter {
    gpu: GpuContext,
    converter: BiPlanarConverter,
    supports_dmabuf: bool,
    supports_16bit_norm: bool,
    software_planes: Option<CachedSoftwarePlanes>,
}

impl LinuxVideoConverter {
    pub(crate) fn new(gpu: &GpuContext) -> Option<Self> {
        let features = gpu.device.features();
        let supports_dmabuf = features.contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF);
        let supports_16bit_norm = features.contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
        Some(Self {
            gpu: gpu.clone(),
            converter: BiPlanarConverter::new(gpu),
            supports_dmabuf,
            supports_16bit_norm,
            software_planes: None,
        })
    }

    pub(crate) fn convert(&mut self, frame: VideoFrame) -> Option<ImageBitmap> {
        let size = frame.size();
        let crop = frame.crop();
        let color = frame.color();
        let rotation = frame.rotation();
        match frame.linux_frame() {
            LinuxFrameMemory::DmaBuf(dmabuf) => {
                let bit_depth = self.bit_depth(dmabuf.format)?;
                let meta = BiPlanarFrameMeta {
                    buffer_size: size,
                    crop,
                    color,
                    rotation,
                    bit_depth,
                };
                self.convert_dmabuf(frame, dmabuf, meta)
            }
            LinuxFrameMemory::Software(planes) => {
                let bit_depth = self.bit_depth(planes.format)?;
                let meta = BiPlanarFrameMeta {
                    buffer_size: size,
                    crop,
                    color,
                    rotation,
                    bit_depth,
                };
                self.convert_software(planes, meta)
            }
        }
    }

    fn bit_depth(&self, format: LinuxPixelFormat) -> Option<YuvBitDepth> {
        match format {
            LinuxPixelFormat::Nv12 => Some(YuvBitDepth::Bit8),
            LinuxPixelFormat::P010 => {
                if !self.supports_16bit_norm {
                    log::warn!("player surface: 10-bit P010 frame needs TEXTURE_FORMAT_16BIT_NORM");
                    return None;
                }
                Some(YuvBitDepth::Bit10)
            }
        }
    }

    fn convert_dmabuf(
        &mut self,
        frame: VideoFrame,
        dmabuf: DmaBufFrame,
        meta: BiPlanarFrameMeta,
    ) -> Option<ImageBitmap> {
        if !self.supports_dmabuf {
            log::warn!("player surface: Vulkan device lacks VULKAN_EXTERNAL_MEMORY_DMA_BUF");
            return None;
        }
        let (luma_fmt, chroma_fmt) = plane_formats(dmabuf.format);
        let luma_tex = self.import_dmabuf_plane(dmabuf.luma, luma_fmt)?;
        let chroma_tex = self.import_dmabuf_plane(dmabuf.chroma, chroma_fmt)?;
        let luma_view = luma_tex.create_view(&Default::default());
        let chroma_view = chroma_tex.create_view(&Default::default());
        let bitmap = self.converter.convert(&luma_view, &chroma_view, meta);
        // Pin `frame` (keeping the exported DMABUF and underlying VA-API surface alive) until
        // the Vulkan GPU pass completes.
        self.gpu.queue.on_submitted_work_done(move || drop(frame));
        bitmap
    }

    fn import_dmabuf_plane(
        &self,
        plane: DmaBufPlane,
        format: wgpu::TextureFormat,
    ) -> Option<wgpu::Texture> {
        let owned_fd = dup_cloexec(plane.fd)?;
        let extent = wgpu::Extent3d {
            width: plane.width,
            height: plane.height,
            depth_or_array_layers: 1,
        };
        let hal_desc = wgpu::hal::TextureDescriptor {
            label: Some("decoded dmabuf plane"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUses::RESOURCE | wgpu::TextureUses::COPY_SRC,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: Vec::new(),
        };
        // SAFETY: `owned_fd` is a valid duplicated DMABUF FD matching `plane` layout and format.
        // Passing `RESOURCE | COPY_SRC` starts the imported image in `VK_IMAGE_LAYOUT_GENERAL` so
        // wgpu transitions it to `SHADER_READ_ONLY_OPTIMAL` without discarding its contents.
        let hal_texture = unsafe {
            let hal_device = self.gpu.device.as_hal::<wgpu::hal::api::Vulkan>()?;
            hal_device
                .texture_from_dmabuf_fd(
                    owned_fd,
                    &hal_desc,
                    plane.modifier,
                    plane.stride,
                    plane.offset,
                )
                .map_err(|error| log::warn!("player surface: DMABUF import failed: {error}"))
                .ok()?
        };
        // SAFETY: `hal_texture` was just created on `self.gpu.device` with matching descriptor.
        let texture = unsafe {
            self.gpu
                .device
                .create_texture_from_hal::<wgpu::hal::api::Vulkan>(
                    hal_texture,
                    &wgpu::TextureDescriptor {
                        label: Some("decoded dmabuf plane"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    },
                    wgpu::TextureUses::RESOURCE | wgpu::TextureUses::COPY_SRC,
                )
        };
        Some(texture)
    }

    fn convert_software(
        &mut self,
        planes: SoftwareBiPlanarPlanes<'_>,
        meta: BiPlanarFrameMeta,
    ) -> Option<ImageBitmap> {
        let size = meta.buffer_size;
        self.ensure_software_planes(size, planes.format);
        let cached = self.software_planes.as_ref()?;
        let chroma_size = (size.0.div_ceil(2), size.1.div_ceil(2));
        self.gpu.queue.write_texture(
            cached.luma_tex.as_image_copy(),
            planes.luma,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(planes.luma_stride),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.write_texture(
            cached.chroma_tex.as_image_copy(),
            planes.chroma,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(planes.chroma_stride),
                rows_per_image: Some(chroma_size.1),
            },
            wgpu::Extent3d {
                width: chroma_size.0,
                height: chroma_size.1,
                depth_or_array_layers: 1,
            },
        );
        self.converter
            .convert(&cached.luma_view, &cached.chroma_view, meta)
    }

    fn ensure_software_planes(&mut self, size: (u32, u32), format: LinuxPixelFormat) {
        if self
            .software_planes
            .as_ref()
            .is_some_and(|cached| cached.size == size && cached.format == format)
        {
            return;
        }
        let (luma_fmt, chroma_fmt) = plane_formats(format);
        let make = |label, (width, height), fmt| {
            self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: fmt,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let luma_tex = make("video software luma", size, luma_fmt);
        let chroma_tex = make(
            "video software chroma",
            (size.0.div_ceil(2), size.1.div_ceil(2)),
            chroma_fmt,
        );
        let luma_view = luma_tex.create_view(&Default::default());
        let chroma_view = chroma_tex.create_view(&Default::default());
        self.software_planes = Some(CachedSoftwarePlanes {
            size,
            format,
            luma_tex,
            chroma_tex,
            luma_view,
            chroma_view,
        });
    }
}

fn plane_formats(format: LinuxPixelFormat) -> (wgpu::TextureFormat, wgpu::TextureFormat) {
    match format {
        LinuxPixelFormat::Nv12 => (wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm),
        LinuxPixelFormat::P010 => (
            wgpu::TextureFormat::R16Unorm,
            wgpu::TextureFormat::Rg16Unorm,
        ),
    }
}

fn dup_cloexec(fd: i32) -> Option<OwnedFd> {
    if fd < 0 {
        return None;
    }
    // SAFETY: duplicates an open DMABUF descriptor with `O_CLOEXEC`.
    let dup = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup < 0 {
        return None;
    }
    // SAFETY: `dup` is a freshly duplicated file descriptor owned exclusively by the returned `OwnedFd`.
    Some(unsafe { OwnedFd::from_raw_fd(dup) })
}
