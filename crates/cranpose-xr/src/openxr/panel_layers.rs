//! The panels' pixels: the atlas that the composition draws into, and a swapchain for each
//! panel that the compositor shows as a quad layer.
//!
//! The composition draws only when it changes. Then each panel's part of the atlas is copied
//! into the next image of the panel's swapchain. Between changes the compositor keeps showing
//! the images it has, so a still panel costs no GPU time.

use cranpose::{GpuContext, GuestView};
use openxr as xr;

use super::{
    gl_context::wrap_texture,
    session::{QuadLayer, XrSession},
};
use crate::{
    METERS_PER_DP,
    space::{PanelSlot, Space},
};

// Pixels of a panel's image in one dp. A dp is a millimeter, and a millimeter a meter away
// covers about one and a half pixels of a Quest 3's display, so the compositor samples the
// image close to one to one.
pub(crate) const DENSITY: f32 = 1.5;

const GL_SRGB8_ALPHA8: u32 = 0x8C43;
const GL_RGBA8: u32 = 0x8058;

struct PanelImage {
    id: u64,
    size: (u32, u32),
    swapchain: xr::Swapchain<xr::OpenGlEs>,
    textures: Vec<wgpu::Texture>,
    // Whether the swapchain has an image to show
    shown: bool,
}

pub(crate) struct PanelLayers {
    gl_format: u32,
    format: wgpu::TextureFormat,
    atlas: Option<(wgpu::Texture, wgpu::TextureView)>,
    images: Vec<PanelImage>,
}

impl PanelLayers {
    pub(crate) fn new(xr_session: &XrSession) -> Result<Self, String> {
        let formats = xr_session
            .session
            .enumerate_swapchain_formats()
            .map_err(|error| format!("OpenXR swapchain formats: {error}"))?;
        let (gl_format, format) = if formats.contains(&GL_SRGB8_ALPHA8) {
            (GL_SRGB8_ALPHA8, wgpu::TextureFormat::Rgba8UnormSrgb)
        } else if formats.contains(&GL_RGBA8) {
            log::warn!("cranpose xr: no sRGB swapchains; colors show too dark");
            (GL_RGBA8, wgpu::TextureFormat::Rgba8Unorm)
        } else {
            return Err(format!("no swapchain format Cranpose draws: {formats:x?}"));
        };
        Ok(Self {
            gl_format,
            format,
            atlas: None,
            images: Vec::new(),
        })
    }

    // The format the composition draws in.
    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    // Draws the composition if it changed and returns a layer for each panel that has an
    // image.
    pub(crate) fn draw<'a>(
        &'a mut self,
        guest: &mut GuestView,
        gpu: &GpuContext,
        xr_session: &XrSession,
        space: &Space,
    ) -> Result<Vec<QuadLayer<'a>>, String> {
        let panels = space.panels();
        let needed = space
            .atlas_size()
            .map(|dp| (dp * DENSITY).ceil() as u32 + 2);
        if guest.pixel_size() != (needed[0], needed[1]) {
            guest
                .resize(needed[0], needed[1], DENSITY)
                .map_err(|error| error.to_string())?;
        }
        let atlas_size = guest.pixel_size();
        let (texture, view) = match self.atlas.take() {
            Some((texture, view)) if (texture.width(), texture.height()) == atlas_size => {
                (texture, view)
            }
            _ => {
                let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("cranpose xr atlas"),
                    size: wgpu::Extent3d {
                        width: atlas_size.0,
                        height: atlas_size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (texture, view)
            }
        };
        let drawn = guest
            .draw(&texture, &view)
            .map_err(|error| error.to_string())?;
        self.images
            .retain(|image| panels.iter().any(|slot| slot.id == image.id));
        if drawn {
            self.copy_panels(&panels, &texture, gpu, xr_session)?;
        }
        self.atlas = Some((texture, view));
        Ok(panels
            .iter()
            .filter_map(|slot| {
                let image = self
                    .images
                    .iter()
                    .find(|image| image.id == slot.id && image.shown)?;
                Some(QuadLayer {
                    swapchain: &image.swapchain,
                    image_size: image.size,
                    pose: slot.pose,
                    size: [slot.atlas[2], slot.atlas[3]].map(|dp| dp * METERS_PER_DP),
                })
            })
            .collect())
    }

    fn copy_panels(
        &mut self,
        panels: &[PanelSlot],
        atlas: &wgpu::Texture,
        gpu: &GpuContext,
        xr_session: &XrSession,
    ) -> Result<(), String> {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cranpose xr panels"),
            });
        let mut acquired = Vec::new();
        for slot in panels {
            let [x, y, width, height] = slot.atlas.map(|dp| (dp * DENSITY).round() as u32);
            let size = (width.max(1), height.max(1));
            if x + size.0 > atlas.width() || y + size.1 > atlas.height() {
                // The atlas grows to hold the panel by the next frame
                continue;
            }
            let index = self.image_for(slot.id, size, gpu, xr_session)?;
            let image = &mut self.images[index];
            let next = image
                .swapchain
                .acquire_image()
                .map_err(|error| format!("OpenXR acquire: {error}"))?;
            image
                .swapchain
                .wait_image(xr::Duration::INFINITE)
                .map_err(|error| format!("OpenXR wait image: {error}"))?;
            let mut source = atlas.as_image_copy();
            source.origin = wgpu::Origin3d { x, y, z: 0 };
            encoder.copy_texture_to_texture(
                source,
                image.textures[next as usize].as_image_copy(),
                wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
            );
            acquired.push(index);
        }
        gpu.queue.submit([encoder.finish()]);
        for index in acquired {
            let image = &mut self.images[index];
            image
                .swapchain
                .release_image()
                .map_err(|error| format!("OpenXR release: {error}"))?;
            image.shown = true;
        }
        Ok(())
    }

    // The index of the panel's image, made anew when the panel is new or its size changed.
    fn image_for(
        &mut self,
        id: u64,
        size: (u32, u32),
        gpu: &GpuContext,
        xr_session: &XrSession,
    ) -> Result<usize, String> {
        if let Some(index) = self.images.iter().position(|image| image.id == id) {
            if self.images[index].size == size {
                return Ok(index);
            }
            self.images.swap_remove(index);
        }
        let swapchain = xr_session
            .session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | xr::SwapchainUsageFlags::TRANSFER_DST
                    | xr::SwapchainUsageFlags::SAMPLED,
                format: self.gl_format,
                sample_count: 1,
                width: size.0,
                height: size.1,
                face_count: 1,
                array_size: 1,
                mip_count: 1,
            })
            .map_err(|error| format!("OpenXR swapchain: {error}"))?;
        let textures = swapchain
            .enumerate_images()
            .map_err(|error| format!("OpenXR swapchain images: {error}"))?
            .into_iter()
            .map(|name| {
                wrap_texture(
                    &gpu.device,
                    name,
                    size,
                    self.format,
                    wgpu::TextureUsages::COPY_DST,
                )
            })
            .collect::<Result<_, _>>()?;
        self.images.push(PanelImage {
            id,
            size,
            swapchain,
            textures,
            shown: false,
        });
        Ok(self.images.len() - 1)
    }
}
