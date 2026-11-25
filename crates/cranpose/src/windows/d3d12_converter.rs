//! Imports Windows D3D11/DXGI shared video textures into wgpu D3D12 with zero CPU copy.
//!
//! Each decoded `VideoFrame` carries an NT shared handle to a pooled `NV12` or `P010` texture and
//! an NT shared handle to the decoder's `ID3D11Fence`. This converter opens each pooled texture
//! and fence once (keyed by process-unique surface/fence IDs), waits on the fence directly on the
//! D3D12 command queue, and hands the `Plane0` and `Plane1` views to [`BiPlanarConverter`].
#![expect(
    unsafe_code,
    reason = "D3D12 shared handle import and wgpu-hal texture wrapping use Win32 COM FFI"
)]

use cranpose_services::DxgiPixelFormat;
use cranpose_ui::ImageBitmap;
use windows::Win32::{
    Foundation::HANDLE,
    Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource},
};

use crate::{
    GpuContext, VideoFrame,
    video_convert::{BiPlanarConverter, BiPlanarFrameMeta, YuvBitDepth},
};

const MAX_CACHED_TEXTURES: usize = 24;

struct CachedSharedTexture {
    texture_id: u64,
    _texture: wgpu::Texture,
    luma_view: wgpu::TextureView,
    chroma_view: wgpu::TextureView,
}

// Converts decoded Windows Media Foundation frames into RGBA8 `ImageBitmap`s on D3D12.
pub(crate) struct D3d12Converter {
    gpu: GpuContext,
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    converter: BiPlanarConverter,
    supports_p010: bool,
    fence: Option<(u64, ID3D12Fence)>,
    textures: Vec<CachedSharedTexture>,
}

impl D3d12Converter {
    // Makes the converter, or `None` when the renderer does not run on D3D12 with `NV12` support.
    pub(crate) fn new(gpu: &GpuContext) -> Option<Self> {
        let features = gpu.device.features();
        if !features.contains(wgpu::Features::TEXTURE_FORMAT_NV12) {
            log::warn!("player surface: D3D12 device lacks TEXTURE_FORMAT_NV12");
            return None;
        }
        // SAFETY: extracts cloned COM references to the active D3D12 device and direct queue.
        let Some(hal) = (unsafe { gpu.device.as_hal::<wgpu::hal::api::Dx12>() }) else {
            log::warn!("player surface: the renderer's device is not a D3D12 device");
            return None;
        };
        let device = hal.raw_device().clone();
        let queue = hal.raw_queue().clone();
        drop(hal);
        let supports_p010 = features.contains(wgpu::Features::TEXTURE_FORMAT_P010);
        Some(Self {
            gpu: gpu.clone(),
            device,
            queue,
            converter: BiPlanarConverter::new(gpu),
            supports_p010,
            fence: None,
            textures: Vec::with_capacity(MAX_CACHED_TEXTURES),
        })
    }

    // Waits on the decoder's shared fence on the GPU queue, converts the shared NV12/P010 planes
    // into the output ring, and keeps `frame` alive until the GPU finishes reading it.
    pub(crate) fn convert(&mut self, frame: VideoFrame) -> Option<ImageBitmap> {
        let shared = frame.dxgi_shared_frame();
        if shared.texture_handle.is_null() || shared.fence_handle.is_null() {
            return None;
        }
        let bit_depth = match shared.format {
            DxgiPixelFormat::Nv12 => YuvBitDepth::Bit8,
            DxgiPixelFormat::P010 => {
                if !self.supports_p010 {
                    log::warn!("player surface: 10-bit P010 frame needs TEXTURE_FORMAT_P010");
                    return None;
                }
                YuvBitDepth::Bit10
            }
        };
        let fence = self.ensure_fence(shared.fence_id, HANDLE(shared.fence_handle))?;
        // SAFETY: schedules a GPU-side queue wait on the decoder's signaled shared fence value.
        if let Err(error) = unsafe { self.queue.Wait(&fence, shared.fence_value) } {
            log::warn!("player surface: D3D12 fence wait failed: {error}");
            return None;
        }
        let size = frame.size();
        let meta = BiPlanarFrameMeta {
            buffer_size: size,
            crop: frame.crop(),
            color: frame.color(),
            rotation: frame.rotation(),
            bit_depth,
        };
        let slot_index = self.ensure_texture(
            shared.texture_id,
            HANDLE(shared.texture_handle),
            size,
            shared.format,
        )?;
        let cached = &self.textures[slot_index];
        let bitmap = self
            .converter
            .convert(&cached.luma_view, &cached.chroma_view, meta);
        // Pin `frame` (and its D3D11 shared pool slot) until the D3D12 GPU pass completes.
        self.gpu.queue.on_submitted_work_done(move || drop(frame));
        bitmap
    }

    fn ensure_fence(&mut self, fence_id: u64, handle: HANDLE) -> Option<ID3D12Fence> {
        if let Some((cached_id, fence)) = &self.fence {
            if *cached_id == fence_id {
                return Some(fence.clone());
            }
            // A new fence ID means a new decoder instance; release cached textures of the old one.
            self.textures.clear();
        }
        let mut opened: Option<ID3D12Fence> = None;
        // SAFETY: `handle` is an NT shared fence handle kept alive by the caller's `VideoFrame`.
        if let Err(error) = unsafe { self.device.OpenSharedHandle(handle, &mut opened) } {
            log::warn!("player surface: OpenSharedHandle(fence) failed: {error}");
            return None;
        }
        let fence = opened?;
        self.fence = Some((fence_id, fence.clone()));
        Some(fence)
    }

    fn ensure_texture(
        &mut self,
        texture_id: u64,
        handle: HANDLE,
        size: (u32, u32),
        format: DxgiPixelFormat,
    ) -> Option<usize> {
        if let Some(index) = self
            .textures
            .iter()
            .position(|entry| entry.texture_id == texture_id)
        {
            return Some(index);
        }
        if self.textures.len() >= MAX_CACHED_TEXTURES {
            self.textures.remove(0);
        }
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: `handle` is an NT shared handle to a single-slice NV12/P010 texture.
        if let Err(error) = unsafe { self.device.OpenSharedHandle(handle, &mut resource) } {
            log::warn!("player surface: OpenSharedHandle(texture) failed: {error}");
            return None;
        }
        let resource = resource?;
        let (wgpu_format, luma_format, chroma_format) = match format {
            DxgiPixelFormat::Nv12 => (
                wgpu::TextureFormat::NV12,
                wgpu::TextureFormat::R8Unorm,
                wgpu::TextureFormat::Rg8Unorm,
            ),
            DxgiPixelFormat::P010 => (
                wgpu::TextureFormat::P010,
                wgpu::TextureFormat::R16Unorm,
                wgpu::TextureFormat::Rg16Unorm,
            ),
        };
        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        // SAFETY: `resource` was opened on this D3D12 device with matching format and dimensions,
        // and starts in `COMMON` which D3D12 implicitly promotes for read-only shader sampling.
        let texture = unsafe {
            let hal = wgpu::hal::dx12::Device::texture_from_raw(
                resource,
                wgpu_format,
                wgpu::TextureDimension::D2,
                extent,
                1,
                1,
            );
            self.gpu
                .device
                .create_texture_from_hal::<wgpu::hal::api::Dx12>(
                    hal,
                    &wgpu::TextureDescriptor {
                        label: Some("decoded dxgi video"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu_format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::TextureUses::RESOURCE,
                )
        };
        let luma_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("decoded dxgi luma"),
            format: Some(luma_format),
            aspect: wgpu::TextureAspect::Plane0,
            ..Default::default()
        });
        let chroma_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("decoded dxgi chroma"),
            format: Some(chroma_format),
            aspect: wgpu::TextureAspect::Plane1,
            ..Default::default()
        });
        self.textures.push(CachedSharedTexture {
            texture_id,
            _texture: texture,
            luma_view,
            chroma_view,
        });
        Some(self.textures.len() - 1)
    }
}
