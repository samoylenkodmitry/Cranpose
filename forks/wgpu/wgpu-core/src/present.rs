/*! Presentation.

## Lifecycle

Whenever a submission detects the use of any surface texture, it adds it to the device
tracker for the duration of the submission (temporarily, while recording).
It's added with `UNINITIALIZED` state and transitioned into `empty()` state.
When this texture is presented, we remove it from the device tracker as well as
extract it from the hub.
!*/

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{mem::ManuallyDrop, sync::atomic::Ordering};

#[cfg(feature = "trace")]
use crate::device::trace::{Action, IntoTrace};
use crate::{
    conv,
    device::{queue::Queue, Device, DeviceError, MissingDownlevelFlags, WaitIdleError},
    global::Global,
    hal_label, id,
    instance::Surface,
    resource::{self, Labeled},
};

use thiserror::Error;
use wgt::{
    error::{ErrorType, WebGpuError},
    SurfaceStatus as Status,
};

const FRAME_TIMEOUT_MS: u32 = 1000;

#[derive(Debug)]
pub(crate) struct Presentation {
    pub(crate) device: Arc<Device>,
    pub(crate) config: wgt::SurfaceConfiguration<Vec<wgt::TextureFormat>>,
    pub(crate) acquired_texture: Option<Arc<resource::Texture>>,
    /// The slot of the acquired texture's image in `images`, where the
    /// backend tells its images apart.
    pub(crate) acquired_image: Option<u32>,
    /// The images the surface handed out since it was configured.
    pub(crate) images: PresentedImages,
}

/// A presentable image a surface texture is (Cranpose fork).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceImage {
    /// The image's slot, the same each time the surface hands the image out
    /// again until it is configured anew.
    pub index: u32,
    /// Whether the texture holds what the image last presented: the surface
    /// keeps presented images and this one was presented since the surface
    /// was configured. Otherwise its contents are undefined.
    pub kept: bool,
}

/// The images a surface followed at once; a surface with more hands the
/// oldest followed one's slot to the next new image, which holds nothing.
const IMAGE_SLOTS: usize = 8;

/// The presentable images a surface handed out since it was configured, by
/// the identity their backend gives them (`image_id`), and which of them
/// hold what they last presented (Cranpose fork).
#[derive(Debug, Default)]
pub(crate) struct PresentedImages {
    /// By slot, the identity of the image in it.
    ids: [Option<u64>; IMAGE_SLOTS],
    /// The slots, as bits, whose image holds what it last presented.
    kept: u8,
    /// The slot a new image takes once every slot is taken.
    next: usize,
}

impl PresentedImages {
    /// The slot of the image of `id`, and whether it holds what it last
    /// presented; an image not seen before takes a slot and holds nothing.
    fn acquire(&mut self, id: u64) -> (u32, bool) {
        let slot = match self.ids.iter().position(|known| *known == Some(id)) {
            Some(slot) => slot,
            None => {
                let slot = self
                    .ids
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(self.next);
                self.next = (slot + 1) % IMAGE_SLOTS;
                self.ids[slot] = Some(id);
                self.kept &= !(1 << slot);
                slot
            }
        };
        (slot as u32, self.kept & (1 << slot) != 0)
    }

    /// Marks the image in `slot` as holding what it presented, or not.
    fn keep(&mut self, slot: u32, kept: bool) {
        let bit = 1u8 << slot;
        if kept {
            self.kept |= bit;
        } else {
            self.kept &= !bit;
        }
    }
}

/// Forgets the acquired image of a presentation, which goes back
/// unpresented: its pixels and layout are whatever the frame left.
fn forget_acquired_image(acquired_image: &mut Option<u32>, images: &mut PresentedImages) {
    if let Some(slot) = acquired_image.take() {
        images.keep(slot, false);
    }
}

#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum SurfaceError {
    #[error("Surface is invalid")]
    Invalid,
    #[error("Surface is not configured for presentation")]
    NotConfigured,
    #[error(transparent)]
    Device(#[from] DeviceError),
    #[error("Surface image is already acquired")]
    AlreadyAcquired,
    #[error("No surface image is currently acquired to present")]
    NothingToPresent,
    #[error("Texture has been destroyed")]
    TextureDestroyed,
}

impl WebGpuError for SurfaceError {
    fn webgpu_error_type(&self) -> ErrorType {
        match self {
            Self::Device(e) => e.webgpu_error_type(),
            Self::Invalid
            | Self::NotConfigured
            | Self::AlreadyAcquired
            | Self::NothingToPresent
            | Self::TextureDestroyed => ErrorType::Validation,
        }
    }
}

#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ConfigureSurfaceError {
    #[error(transparent)]
    Device(#[from] DeviceError),
    #[error("Invalid surface")]
    InvalidSurface,
    #[error("The view format {0:?} is not compatible with texture format {1:?}, only changing srgb-ness is allowed.")]
    InvalidViewFormat(wgt::TextureFormat, wgt::TextureFormat),
    #[error(transparent)]
    MissingDownlevelFlags(#[from] MissingDownlevelFlags),
    #[error("The `SurfaceOutput` returned by `get_current_texture` must be dropped before re-configuring via `configure` or  retrieving a new texture via `get_current_texture`.")]
    PreviousOutputExists,
    #[error("Failed to wait for GPU to come idle before reconfiguring the Surface")]
    GpuWaitTimeout,
    #[error("Both `Surface` width and height must be non-zero. Wait to recreate the `Surface` until the window has non-zero area.")]
    ZeroArea,
    #[error("`Surface` width and height must be within the maximum supported texture size. Requested was ({width}, {height}), maximum extent for either dimension is {max_texture_dimension_2d}.")]
    TooLarge {
        width: u32,
        height: u32,
        max_texture_dimension_2d: u32,
    },
    #[error("Surface does not support the adapter's queue family")]
    UnsupportedQueueFamily,
    #[error("Requested format {requested:?} is not in list of supported formats: {available:?}")]
    UnsupportedFormat {
        requested: wgt::TextureFormat,
        available: Vec<wgt::TextureFormat>,
    },
    #[error("Requested color space {requested:?} is not in the list of color spaces supported for format {format:?}: {available:?}")]
    UnsupportedColorSpace {
        requested: wgt::SurfaceColorSpace,
        format: wgt::TextureFormat,
        available: wgt::SurfaceColorSpaces,
    },
    #[error("Requested present mode {requested:?} is not in the list of supported present modes: {available:?}")]
    UnsupportedPresentMode {
        requested: wgt::PresentMode,
        available: Vec<wgt::PresentMode>,
    },
    #[error("Requested alpha mode {requested:?} is not in the list of supported alpha modes: {available:?}")]
    UnsupportedAlphaMode {
        requested: wgt::CompositeAlphaMode,
        available: Vec<wgt::CompositeAlphaMode>,
    },
    #[error("Requested usage {requested:?} is not in the list of supported usages: {available:?}")]
    UnsupportedUsage {
        requested: wgt::TextureUses,
        available: wgt::TextureUses,
    },
}

impl From<WaitIdleError> for ConfigureSurfaceError {
    fn from(e: WaitIdleError) -> Self {
        match e {
            WaitIdleError::Device(d) => ConfigureSurfaceError::Device(d),
            WaitIdleError::WrongSubmissionIndex(..) => unreachable!(),
            WaitIdleError::Timeout => ConfigureSurfaceError::GpuWaitTimeout,
        }
    }
}

impl WebGpuError for ConfigureSurfaceError {
    fn webgpu_error_type(&self) -> ErrorType {
        match self {
            Self::Device(e) => e.webgpu_error_type(),
            Self::MissingDownlevelFlags(e) => e.webgpu_error_type(),
            Self::InvalidSurface
            | Self::InvalidViewFormat(..)
            | Self::PreviousOutputExists
            | Self::GpuWaitTimeout
            | Self::ZeroArea
            | Self::TooLarge { .. }
            | Self::UnsupportedQueueFamily
            | Self::UnsupportedFormat { .. }
            | Self::UnsupportedColorSpace { .. }
            | Self::UnsupportedPresentMode { .. }
            | Self::UnsupportedAlphaMode { .. }
            | Self::UnsupportedUsage { .. } => ErrorType::Validation,
        }
    }
}

pub type ResolvedSurfaceOutput = SurfaceOutput<Arc<resource::Texture>>;

#[repr(C)]
#[derive(Debug)]
pub struct SurfaceOutput<T = id::TextureId> {
    pub status: Status,
    pub texture: Option<T>,
    /// The swapchain image the texture is, where the backend numbers them.
    pub image: Option<SurfaceImage>,
}

impl Surface {
    pub fn get_current_texture(&self) -> Result<ResolvedSurfaceOutput, SurfaceError> {
        profiling::scope!("Surface::get_current_texture");

        let (device, config) = if let Some(ref present) = *self.presentation.lock() {
            present.device.check_is_valid()?;
            (present.device.clone(), present.config.clone())
        } else {
            return Err(SurfaceError::NotConfigured);
        };

        let suf = self.raw(device.backend()).unwrap();
        let mut image = None;
        let (texture, status) = match unsafe {
            suf.acquire_texture(
                Some(core::time::Duration::from_millis(FRAME_TIMEOUT_MS as u64)),
                device.fence.as_ref(),
            )
        } {
            Ok(ast) => {
                let texture_desc = wgt::TextureDescriptor {
                    label: hal_label(
                        Some(alloc::borrow::Cow::Borrowed("<Surface Texture>")),
                        device.instance_flags,
                    ),
                    size: wgt::Extent3d {
                        width: config.width,
                        height: config.height,
                        depth_or_array_layers: 1,
                    },
                    sample_count: 1,
                    mip_level_count: 1,
                    format: config.format,
                    dimension: wgt::TextureDimension::D2,
                    usage: config.usage,
                    view_formats: config.view_formats,
                };
                let format_features = wgt::TextureFormatFeatures {
                    allowed_usages: wgt::TextureUsages::RENDER_ATTACHMENT,
                    flags: wgt::TextureFormatFeatureFlags::MULTISAMPLE_X4
                        | wgt::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE,
                };
                let hal_usage = conv::map_texture_usage(
                    config.usage,
                    config.format.into(),
                    format_features.flags,
                );
                let clear_view_desc = hal::TextureViewDescriptor {
                    label: hal_label(
                        Some("(wgpu internal) clear surface texture view"),
                        device.instance_flags,
                    ),
                    format: config.format,
                    dimension: wgt::TextureViewDimension::D2,
                    usage: wgt::TextureUses::COLOR_TARGET,
                    range: wgt::ImageSubresourceRange::default(),
                };
                let clear_view = unsafe {
                    device
                        .raw()
                        .create_texture_view(ast.texture.as_ref().borrow(), &clear_view_desc)
                }
                .map_err(|e| device.handle_hal_error(e))?;

                let image_id = ast.texture.image_id();
                let mut presentation = self.presentation.lock();
                let present = presentation.as_mut().unwrap();
                let slot = image_id.map(|id| present.images.acquire(id));
                // An image that holds what it presented stays as it is: in
                // the layout it was presented in, and initialized, so a pass
                // that loads it reads those pixels instead of a clear.
                let kept = self.keep_images.load(Ordering::Acquire)
                    && slot.is_some_and(|(_, kept)| kept);
                let texture = resource::Texture::new(
                    &device,
                    resource::TextureInner::Surface { raw: ast.texture },
                    hal_usage,
                    &texture_desc,
                    format_features,
                    resource::TextureClearMode::Surface {
                        clear_view: ManuallyDrop::new(clear_view),
                    },
                    !kept,
                );

                let texture = Arc::new(texture);

                device.trackers.lock().textures.insert_single(
                    &texture,
                    if kept {
                        wgt::TextureUses::PRESENT
                    } else {
                        wgt::TextureUses::UNINITIALIZED
                    },
                );

                if present.acquired_texture.is_some() {
                    return Err(SurfaceError::AlreadyAcquired);
                }
                present.acquired_texture = Some(texture.clone());
                present.acquired_image = slot.map(|(index, _)| index);
                image = slot.map(|(index, _)| SurfaceImage { index, kept });

                let status = if ast.suboptimal {
                    Status::Suboptimal
                } else {
                    Status::Good
                };
                (Some(texture), status)
            }
            Err(err) => (
                None,
                match err {
                    hal::SurfaceError::Timeout => Status::Timeout,
                    hal::SurfaceError::Occluded => Status::Occluded,
                    hal::SurfaceError::Lost => Status::Lost,
                    hal::SurfaceError::Device(err) => {
                        return Err(device.handle_hal_error(err).into());
                    }
                    hal::SurfaceError::Outdated => Status::Outdated,
                    hal::SurfaceError::Other(msg) => {
                        log::error!("acquire error: {msg}");
                        Status::Lost
                    }
                },
            ),
        };

        Ok(ResolvedSurfaceOutput {
            status,
            texture,
            image,
        })
    }

    /// Marks the image in `slot` as holding what it presented.
    fn keep_image(&self, slot: Option<u32>) {
        let Some(slot) = slot else {
            return;
        };
        if let Some(present) = self.presentation.lock().as_mut() {
            present.images.keep(slot, true);
        }
    }

    pub fn present(&self) -> Result<Status, SurfaceError> {
        profiling::scope!("Surface::present");

        let presentation = self.presentation.lock();
        let present = match presentation.as_ref() {
            Some(present) => present,
            None => return Err(SurfaceError::NotConfigured),
        };

        present.device.check_is_valid()?;
        let queue = present
            .device
            .get_queue()
            .ok_or(SurfaceError::Device(DeviceError::Lost))?;
        drop(presentation);

        queue.present(self)
    }
}

impl Queue {
    pub fn present(&self, surface: &Surface) -> Result<Status, SurfaceError> {
        profiling::scope!("Queue::present");

        let (texture, image) = {
            let mut presentation = surface.presentation.lock();
            let present = match presentation.as_mut() {
                Some(present) => present,
                None => return Err(SurfaceError::NotConfigured),
            };

            let device = &self.device;

            // Check the surface is configured for this device.
            if !Arc::ptr_eq(&present.device, device) {
                return Err(SurfaceError::Device(DeviceError::DeviceMismatch(Box::new(
                    crate::device::DeviceMismatch {
                        res: self.error_ident(),
                        res_device: device.error_ident(),
                        target: None,
                        target_device: present.device.error_ident(),
                    },
                ))));
            }

            let texture = present
                .acquired_texture
                .take()
                .ok_or(SurfaceError::NothingToPresent)?;
            // Kept again only once it presented.
            let image = present.acquired_image;
            forget_acquired_image(&mut present.acquired_image, &mut present.images);
            (texture, image)
        };

        // If the texture was never rendered to, clear it and transition to
        // PRESENT state before presenting.
        // Fixes <https://github.com/gfx-rs/wgpu/issues/6748>
        self.prepare_surface_texture_for_present(&texture)?;

        let device = &self.device;

        let mut exclusive_snatch_guard = device.snatchable_lock.write();
        let inner = texture
            .inner
            .snatch(&mut exclusive_snatch_guard)
            .maybe_valid();
        drop(exclusive_snatch_guard);

        let result = match inner {
            None => return Err(SurfaceError::TextureDestroyed),
            Some(resource::TextureInner::Surface { raw }) => {
                let raw_surface = surface.raw(device.backend()).unwrap();
                let raw_queue = self.raw();
                // [`wgpu_hal::Queue::present`] requires the queue to be synchronized with submit calls and
                // other present calls. Locking command indices prevents submits which must increment the
                // submission index, and by `write`ing prevents other present calls.
                let _command_indices = device.command_indices.write();
                unsafe { raw_queue.present(raw_surface, raw) }
            }
            _ => unreachable!(),
        };
        if result.is_ok() {
            surface.keep_image(image);
        }

        match result {
            Ok(()) => Ok(Status::Good),
            Err(err) => match err {
                hal::SurfaceError::Timeout => Ok(Status::Timeout),
                hal::SurfaceError::Occluded => Ok(Status::Occluded),
                hal::SurfaceError::Lost => Ok(Status::Lost),
                hal::SurfaceError::Device(err) => {
                    Err(SurfaceError::from(device.handle_hal_error(err)))
                }
                hal::SurfaceError::Outdated => Ok(Status::Outdated),
                hal::SurfaceError::Other(msg) => {
                    log::error!("present error: {msg}");
                    Err(SurfaceError::Invalid)
                }
            },
        }
    }
}

impl Surface {
    pub fn discard(&self) -> Result<(), SurfaceError> {
        profiling::scope!("Surface::discard");

        let mut presentation = self.presentation.lock();
        let present = match presentation.as_mut() {
            Some(present) => present,
            None => return Err(SurfaceError::NotConfigured),
        };

        let device = &present.device;

        device.check_is_valid()?;

        let texture = present
            .acquired_texture
            .take()
            .ok_or(SurfaceError::NothingToPresent)?;
        forget_acquired_image(&mut present.acquired_image, &mut present.images);

        let mut exclusive_snatch_guard = device.snatchable_lock.write();
        let inner = texture
            .inner
            .snatch(&mut exclusive_snatch_guard)
            .maybe_valid();
        drop(exclusive_snatch_guard);

        match inner {
            None => return Err(SurfaceError::TextureDestroyed),
            Some(resource::TextureInner::Surface { raw }) => {
                let raw_surface = self.raw(device.backend()).unwrap();
                unsafe { raw_surface.discard_texture(raw) };
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    /// Like `discard`, drops the inner texture reference, but skips the
    /// HAL `discard_texture` call. Safe to call during unwinding
    pub fn release(&self) -> Result<(), SurfaceError> {
        profiling::scope!("Surface::release");

        let mut presentation = self.presentation.lock();
        let Some(present) = presentation.as_mut() else {
            return Err(SurfaceError::NotConfigured);
        };

        // `texture` is dropped here, decrementing the refcount of
        // Arc<SwapchainAcquireSemaphore>. If this was the last Arc, the Texture
        // is freed, which drops NativeSurfaceTextureMetadata and
        // its Arc<SwapchainAcquireSemaphore>.
        _ = present
            .acquired_texture
            .take()
            .ok_or(SurfaceError::NothingToPresent)?;
        forget_acquired_image(&mut present.acquired_image, &mut present.images);

        Ok(())
    }
}

impl Global {
    pub fn surface_get_current_texture(
        &self,
        surface_id: id::SurfaceId,
        texture_id_in: Option<id::TextureId>,
    ) -> Result<SurfaceOutput, SurfaceError> {
        let surface = self.surfaces.get(surface_id);

        let fid = self.hub.textures.prepare(texture_id_in);

        let output = surface.get_current_texture()?;

        #[cfg(feature = "trace")]
        if let Some(present) = surface.presentation.lock().as_ref() {
            if let Some(ref mut trace) = *present.device.trace.lock() {
                if let Some(texture) = present.acquired_texture.as_ref() {
                    trace.add(Action::GetSurfaceTexture {
                        id: texture.to_trace(),
                        parent: surface.to_trace(),
                    });
                }
            }
        }

        let status = output.status;
        let texture_id = output.texture.map(|texture| fid.assign(texture));

        Ok(SurfaceOutput {
            status,
            texture: texture_id,
            image: output.image,
        })
    }

    /// Asks the surface to hand each presented swapchain image back as it
    /// presented it, where the backend numbers images: an acquired image
    /// presented since the surface was configured is then initialized and
    /// holds its pixels, so a pass may load it and draw only what changed
    /// (Cranpose fork). The platform must keep presented images' contents,
    /// which Vulkan does not promise.
    pub fn surface_keep_presented_images(&self, surface_id: id::SurfaceId, keep: bool) {
        self.surfaces
            .get(surface_id)
            .keep_images
            .store(keep, Ordering::Release);
    }

    pub fn surface_present(&self, surface_id: id::SurfaceId) -> Result<Status, SurfaceError> {
        let surface = self.surfaces.get(surface_id);

        #[cfg(feature = "trace")]
        if let Some(present) = surface.presentation.lock().as_ref() {
            if let Some(ref mut trace) = *present.device.trace.lock() {
                trace.add(Action::Present(surface.to_trace()));
            }
        }

        surface.present()
    }

    pub fn surface_texture_discard(&self, surface_id: id::SurfaceId) -> Result<(), SurfaceError> {
        let surface = self.surfaces.get(surface_id);

        #[cfg(feature = "trace")]
        if let Some(present) = surface.presentation.lock().as_ref() {
            if let Some(ref mut trace) = *present.device.trace.lock() {
                trace.add(Action::DiscardSurfaceTexture(surface.to_trace()));
            }
        }

        surface.discard()
    }

    pub fn surface_texture_release(&self, surface_id: id::SurfaceId) -> Result<(), SurfaceError> {
        let surface = self.surfaces.get(surface_id);

        #[cfg(feature = "trace")]
        if let Some(present) = surface.presentation.lock().as_ref() {
            if let Some(ref mut trace) = *present.device.trace.lock() {
                trace.add(Action::ReleaseSurfaceTexture(surface.to_trace()));
            }
        }

        surface.release()
    }
}
