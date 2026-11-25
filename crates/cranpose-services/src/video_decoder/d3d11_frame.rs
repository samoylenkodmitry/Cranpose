//! Shared D3D11 GPU surface pool and fence synchronization for zero-copy Windows video frames.
//!
//! Media Foundation hardware decoders output slices of a decoder-owned `ID3D11Texture2D` array
//! that cannot be shared directly across D3D11/D3D12. This module maintains a fixed pool of
//! single-slice `NV12` / `P010` shared textures with NT handles and a shared `ID3D11Fence`. Each
//! decoded picture performs one GPU-to-GPU `CopySubresourceRegion` into a recycled pool slot and
//! signals the fence; the D3D12 renderer waits on the fence on the GPU queue and samples the
//! planes directly with zero CPU pixel copies.
#![expect(
    unsafe_code,
    reason = "D3D11, DXGI, and Media Foundation device manager are Win32 COM APIs"
)]

use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;
use windows::{
    Win32::{
        Foundation::{CloseHandle, GENERIC_ALL, HANDLE, HMODULE},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1},
            Direct3D11::{
                D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_FENCE_FLAG_SHARED,
                D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_SDK_VERSION,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
                ID3D11Device5, ID3D11DeviceContext, ID3D11DeviceContext4, ID3D11Fence,
                ID3D11Multithread, ID3D11Texture2D,
            },
            Dxgi::{
                Common::{DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010, DXGI_SAMPLE_DESC},
                DXGI_SHARED_RESOURCE_READ, IDXGIResource1,
            },
        },
        Media::MediaFoundation::{IMFDXGIDeviceManager, MFCreateDXGIDeviceManager},
    },
    core::{Interface, PCWSTR},
};

use super::DecoderError;

// Count of decoded pictures currently held by the queue or the application.
pub(super) type LivePictures = Arc<AtomicUsize>;

// Maximum reusable shared texture slots kept in the pool per decoder.
const MAX_POOLED_SLOTS: usize = 16;

// Monotonic process-wide identifiers so cached D3D12 imports never alias recycled OS HANDLEs.
static NEXT_SURFACE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_FENCE_ID: AtomicU64 = AtomicU64::new(1);

/// Bi-planar DXGI pixel format of a Windows decoded [`VideoFrame`](super::VideoFrame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DxgiPixelFormat {
    /// 8-bit 4:2:0 bi-planar YUV (`DXGI_FORMAT_NV12`).
    Nv12,
    /// 10-bit 4:2:0 bi-planar YUV in the upper 10 bits of 16-bit channels (`DXGI_FORMAT_P010`).
    P010,
}

impl DxgiPixelFormat {
    pub(super) fn dxgi_format(self) -> DXGI_FORMAT {
        match self {
            Self::Nv12 => DXGI_FORMAT_NV12,
            Self::P010 => DXGI_FORMAT_P010,
        }
    }

    pub(super) fn from_dxgi_format(format: DXGI_FORMAT) -> Option<Self> {
        if format == DXGI_FORMAT_P010 {
            Some(Self::P010)
        } else if format == DXGI_FORMAT_NV12 {
            Some(Self::Nv12)
        } else {
            None
        }
    }
}

/// Shared D3D11/DXGI GPU texture and fence metadata exported by a Windows
/// [`VideoFrame`](super::VideoFrame) for zero-copy D3D12 import.
///
/// Both NT handles remain valid for as long as the owning [`VideoFrame`](super::VideoFrame) lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DxgiSharedFrame {
    /// Shared NT `HANDLE` of the single-slice `NV12` or `P010` `ID3D11Texture2D`.
    pub texture_handle: *mut c_void,
    /// Process-unique identifier of this pooled shared texture (never reused across decoders).
    pub texture_id: u64,
    /// Shared NT `HANDLE` of the `ID3D11Fence` signaled when GPU writes finish.
    pub fence_handle: *mut c_void,
    /// Process-unique identifier of the shared fence.
    pub fence_id: u64,
    /// Fence value that must be waited on before reading the texture on D3D12.
    pub fence_value: u64,
    /// Pixel format of the shared texture.
    pub format: DxgiPixelFormat,
}

struct SharedFence {
    fence: ID3D11Fence,
    shared_handle: HANDLE,
    fence_id: u64,
    next_value: AtomicU64,
}

// SAFETY: D3D11 multithread protection is enabled on the owning device, and `HANDLE` is a
// process-wide kernel object handle closed only when the last `Arc<SharedFence>` drops.
unsafe impl Send for SharedFence {}
// SAFETY: as for Send; `next_value` is atomic and `shared_handle` is immutable.
unsafe impl Sync for SharedFence {}

impl Drop for SharedFence {
    fn drop(&mut self) {
        if !self.shared_handle.is_invalid() {
            // SAFETY: `shared_handle` was created by `ID3D11Fence::CreateSharedHandle` and is
            // owned exclusively by this `SharedFence`.
            let _ = unsafe { CloseHandle(self.shared_handle) };
        }
    }
}

struct SharedSlot {
    texture: ID3D11Texture2D,
    shared_handle: HANDLE,
    surface_id: u64,
    width: u32,
    height: u32,
    format: DxgiPixelFormat,
}

// SAFETY: D3D11 multithread protection is enabled on the owning device, and a slot is accessed
// exclusively by either the decoder thread (writing) or the GPU renderer (reading after fence).
unsafe impl Send for SharedSlot {}
// SAFETY: as for Send; fields are immutable after creation.
unsafe impl Sync for SharedSlot {}

impl Drop for SharedSlot {
    fn drop(&mut self) {
        if !self.shared_handle.is_invalid() {
            // SAFETY: `shared_handle` was created by `IDXGIResource1::CreateSharedHandle` and is
            // owned exclusively by this `SharedSlot`.
            let _ = unsafe { CloseHandle(self.shared_handle) };
        }
    }
}

// One decoded picture backed by a pooled D3D11 shared texture. Dropping returns the slot to the
// decoder's pool so steady-state playback allocates no textures or NT handles.
pub(super) struct Image {
    slot: Option<SharedSlot>,
    recycler: Arc<Mutex<Vec<SharedSlot>>>,
    fence: Arc<SharedFence>,
    fence_value: u64,
    live: LivePictures,
}

impl Image {
    pub(super) fn shared_frame(&self) -> DxgiSharedFrame {
        let slot = self.slot.as_ref().expect("slot present until drop");
        DxgiSharedFrame {
            texture_handle: slot.shared_handle.0,
            texture_id: slot.surface_id,
            fence_handle: self.fence.shared_handle.0,
            fence_id: self.fence.fence_id,
            fence_value: self.fence_value,
            format: slot.format,
        }
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            let mut pool = self.recycler.lock();
            if pool.len() < MAX_POOLED_SLOTS {
                pool.push(slot);
            }
        }
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}

// Owns the decoder's D3D11 device, shared fence, and recycled shared texture slots.
pub(super) struct SharedSurfacePool {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    context4: ID3D11DeviceContext4,
    fence: Arc<SharedFence>,
    recycler: Arc<Mutex<Vec<SharedSlot>>>,
}

impl SharedSurfacePool {
    pub(super) fn new() -> Result<(Self, IMFDXGIDeviceManager), DecoderError> {
        let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        let mut device = None;
        let mut context = None;
        // SAFETY: `D3D11CreateDevice` writes COM pointers into our `Option` out-params.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(|error| DecoderError::Failed(format!("D3D11 device: {error}")))?;
        let device = device.ok_or(DecoderError::Unavailable)?;
        let context = context.ok_or(DecoderError::Unavailable)?;

        if let Ok(mt) = device.cast::<ID3D11Multithread>() {
            // SAFETY: enables thread-safe access required by Media Foundation DXGI device manager.
            let _ = unsafe { mt.SetMultithreadProtected(true) };
        }
        let device5: ID3D11Device5 = device
            .cast()
            .map_err(|error| DecoderError::Failed(format!("ID3D11Device5: {error}")))?;
        let context4: ID3D11DeviceContext4 = context
            .cast()
            .map_err(|error| DecoderError::Failed(format!("ID3D11DeviceContext4: {error}")))?;

        let mut fence: Option<ID3D11Fence> = None;
        // SAFETY: creates a D3D11 fence shareable with D3D12 via NT handle.
        unsafe { device5.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut fence) }
            .map_err(|error| DecoderError::Failed(format!("D3D11 shared fence: {error}")))?;
        let fence = fence.ok_or(DecoderError::Unavailable)?;
        // SAFETY: exports an NT handle owned by `SharedFence` until its drop closes it.
        let fence_handle = unsafe { fence.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()) }
            .map_err(|error| DecoderError::Failed(format!("D3D11 fence handle: {error}")))?;

        let mut reset_token = 0u32;
        let mut manager = None;
        // SAFETY: allocates a Media Foundation DXGI device manager bound to our D3D11 device.
        unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut manager) }
            .map_err(|error| DecoderError::Failed(format!("DXGI device manager: {error}")))?;
        let manager = manager.ok_or(DecoderError::Unavailable)?;
        // SAFETY: binds the multithreaded D3D11 device with the matching reset token.
        unsafe { manager.ResetDevice(&device, reset_token) }
            .map_err(|error| DecoderError::Failed(format!("DXGI ResetDevice: {error}")))?;

        let pool = Self {
            device,
            context,
            context4,
            fence: Arc::new(SharedFence {
                fence,
                shared_handle: fence_handle,
                fence_id: NEXT_FENCE_ID.fetch_add(1, Ordering::Relaxed),
                next_value: AtomicU64::new(1),
            }),
            recycler: Arc::new(Mutex::new(Vec::with_capacity(MAX_POOLED_SLOTS))),
        };
        Ok((pool, manager))
    }

    // Copies one decoded subresource on the GPU into a recycled shared slot and signals the fence.
    pub(super) fn copy_gpu_texture(
        &mut self,
        src: &ID3D11Texture2D,
        subresource: u32,
        live: &LivePictures,
    ) -> Result<(Image, (u32, u32)), DecoderError> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `GetDesc` writes the texture's immutable descriptor.
        unsafe { src.GetDesc(&mut desc) };
        let format = DxgiPixelFormat::from_dxgi_format(desc.Format).ok_or_else(|| {
            DecoderError::Failed(format!("unsupported DXGI output format: {:?}", desc.Format))
        })?;
        let slot = self.acquire_slot(desc.Width, desc.Height, format)?;
        // SAFETY: both textures share the same device, dimensions, and DXGI format; subresource
        // index comes from `IMFDXGIBuffer::GetSubresourceIndex`.
        unsafe {
            self.context
                .CopySubresourceRegion(&slot.texture, 0, 0, 0, 0, src, subresource, None);
        }
        let image = self.finish_slot(slot, live)?;
        Ok((image, (desc.Width, desc.Height)))
    }

    // Uploads a CPU-resident software MFT output buffer into a recycled shared GPU texture.
    pub(super) fn upload_cpu_buffer(
        &mut self,
        src: *const u8,
        row_pitch: u32,
        size: (u32, u32),
        format: DxgiPixelFormat,
        live: &LivePictures,
    ) -> Result<Image, DecoderError> {
        let slot = self.acquire_slot(size.0, size.1, format)?;
        // SAFETY: `src` is a locked Media Foundation 2D buffer valid for `size.1 * 3 / 2` rows
        // with pitch `row_pitch`, matching the NV12/P010 layout of `slot.texture`.
        unsafe {
            self.context.UpdateSubresource(
                &slot.texture,
                0,
                None,
                src.cast(),
                row_pitch,
                row_pitch * size.1 * 3 / 2,
            );
        }
        self.finish_slot(slot, live)
    }

    fn finish_slot(&self, slot: SharedSlot, live: &LivePictures) -> Result<Image, DecoderError> {
        let fence_value = self.fence.next_value.fetch_add(1, Ordering::Relaxed);
        // SAFETY: signals the shared fence after the GPU copy/upload and flushes so D3D12 can see
        // the fence signal without stalling the CPU.
        unsafe {
            self.context4
                .Signal(&self.fence.fence, fence_value)
                .map_err(|error| DecoderError::Failed(format!("D3D11 fence signal: {error}")))?;
            self.context.Flush();
        }
        live.fetch_add(1, Ordering::Relaxed);
        Ok(Image {
            slot: Some(slot),
            recycler: Arc::clone(&self.recycler),
            fence: Arc::clone(&self.fence),
            fence_value,
            live: Arc::clone(live),
        })
    }

    fn acquire_slot(
        &self,
        width: u32,
        height: u32,
        format: DxgiPixelFormat,
    ) -> Result<SharedSlot, DecoderError> {
        let mut pool = self.recycler.lock();
        if let Some(pos) = pool
            .iter()
            .position(|slot| slot.width == width && slot.height == height && slot.format == format)
        {
            return Ok(pool.swap_remove(pos));
        }
        // Discard mismatched slots from an earlier resolution or bit depth.
        pool.clear();
        drop(pool);

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format.dxgi_format(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: u32::try_from(D3D11_BIND_SHADER_RESOURCE.0).unwrap_or(0),
            CPUAccessFlags: 0,
            MiscFlags: u32::try_from(
                D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0,
            )
            .unwrap_or(0),
        };
        let mut texture = None;
        // SAFETY: creates a single-slice default texture with NT shared handle flags.
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture)) }
            .map_err(|error| DecoderError::Failed(format!("D3D11 shared texture: {error}")))?;
        let texture = texture.ok_or(DecoderError::Unavailable)?;
        let resource: IDXGIResource1 = texture
            .cast()
            .map_err(|error| DecoderError::Failed(format!("IDXGIResource1: {error}")))?;
        // SAFETY: exports a read-only NT handle owned by `SharedSlot` until drop closes it.
        let shared_handle = unsafe {
            resource.CreateSharedHandle(None, DXGI_SHARED_RESOURCE_READ.0, PCWSTR::null())
        }
        .map_err(|error| DecoderError::Failed(format!("DXGI shared handle: {error}")))?;
        Ok(SharedSlot {
            texture,
            shared_handle,
            surface_id: NEXT_SURFACE_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            format,
        })
    }
}
