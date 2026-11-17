//! The panels' pixels: the atlas that the composition draws into, and a swapchain for each
//! panel that the compositor shows as a quad layer.
//!
//! The composition draws only when it changes. Then each panel's part of the atlas is copied
//! into the next image of the panel's swapchain. Between changes the compositor keeps showing
//! the images it has, so a still panel costs no GPU time.

use cranpose::{GpuContext, GuestView};
use openxr as xr;

use super::{
    cylinder_layers::CylinderLayer,
    graphics::{Api, Graphics, SwapchainFormat},
    session::{LayerSource, QuadLayer, XrSession},
};
use crate::{
    METERS_PER_DP,
    space::{PanelKind, PanelSlot, Shape, Space},
};

// Pixels of a panel's image in one dp. A dp is a millimeter, and a millimeter a meter away
// covers about one and a half pixels of a Quest 3's display, so the compositor samples the
// image close to one to one.
pub(crate) const DENSITY: f32 = 1.5;

struct PanelImage {
    id: u64,
    size: (u32, u32),
    swapchain: xr::Swapchain<Api>,
    textures: Vec<wgpu::Texture>,
    // Whether the swapchain has an image to show
    shown: bool,
}

// The layers of the panels in a frame: the flat panels and the curved ones.
pub(crate) struct PanelFrame<'a> {
    pub(crate) quads: Vec<QuadLayer<'a>>,
    pub(crate) cylinders: Vec<CylinderLayer<'a>>,
}

pub(crate) struct PanelLayers {
    format: SwapchainFormat,
    atlas: Option<(wgpu::Texture, wgpu::TextureView)>,
    images: Vec<PanelImage>,
}

impl PanelLayers {
    pub(crate) fn new(xr_session: &XrSession) -> Result<Self, String> {
        Ok(Self {
            format: SwapchainFormat::choose(&xr_session.session)?,
            atlas: None,
            images: Vec::new(),
        })
    }

    // The format of the swapchains, which the composition draws in too.
    pub(crate) fn format(&self) -> SwapchainFormat {
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
    ) -> Result<PanelFrame<'a>, String> {
        fit_atlas(guest, space)?;
        let (texture, view) = self.atlas_texture(gpu, guest.pixel_size());
        let drawn = guest
            .draw(&texture, &view)
            .map_err(|error| error.to_string())?;
        // The draw composes, so the panels are read after it: their parts are where it drew them
        let panels = space.panels();
        self.images
            .retain(|image| panels.iter().any(|slot| slot.id == image.id));
        if drawn {
            self.copy_panels(&panels, &texture, gpu, xr_session)?;
        }
        self.atlas = Some((texture, view));
        // A panel that grew in this draw gets its room by the next frame
        fit_atlas(guest, space)?;
        let quads = panels
            .iter()
            .filter(|slot| slot.shape == Shape::Flat)
            .filter_map(|slot| {
                let image = self
                    .images
                    .iter()
                    .find(|image| image.id == slot.id && image.shown)?;
                Some(QuadLayer {
                    swapchain: &image.swapchain,
                    source: LayerSource::Drawn(image.size),
                    pose: slot.pose,
                    size: slot.size().map(|dp| dp * METERS_PER_DP),
                })
            })
            .collect();
        Ok(PanelFrame {
            quads,
            cylinders: curved_layers(&panels, &self.images),
        })
    }

    // The texture the composition draws the atlas into, made anew when the atlas's size changes.
    fn atlas_texture(
        &mut self,
        gpu: &GpuContext,
        size: (u32, u32),
    ) -> (wgpu::Texture, wgpu::TextureView) {
        if let Some((texture, view)) = self.atlas.take()
            && (texture.width(), texture.height()) == size
        {
            return (texture, view);
        }
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cranpose xr atlas"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format.wgpu,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
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
        // A surface has no part of the atlas
        for slot in panels.iter().filter(|slot| slot.kind == PanelKind::Content) {
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
            Graphics::ready_for_release(&mut encoder, &image.textures[next as usize]);
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
        let known = self.images.iter().position(|image| image.id == id);
        if let Some(index) = known
            && self.images[index].size == size
        {
            return Ok(index);
        }
        let swapchain = xr_session
            .session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | xr::SwapchainUsageFlags::TRANSFER_DST
                    | xr::SwapchainUsageFlags::SAMPLED,
                format: self.format.api,
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
            .map(|image| {
                Graphics::wrap_image(
                    &gpu.device,
                    image,
                    size,
                    self.format.wgpu,
                    wgpu::TextureUsages::COPY_DST,
                )
            })
            .collect::<Result<_, _>>()?;
        let image = PanelImage {
            id,
            size,
            swapchain,
            textures,
            shown: false,
        };
        // A panel that changed size keeps its index, so the indices of the frame's other images
        // stay valid
        Ok(match known {
            Some(index) => {
                self.images[index] = image;
                index
            }
            None => {
                self.images.push(image);
                self.images.len() - 1
            }
        })
    }
}

// The layers of the curved panels that have an image.
fn curved_layers<'a>(panels: &[PanelSlot], images: &'a [PanelImage]) -> Vec<CylinderLayer<'a>> {
    panels
        .iter()
        .filter_map(|slot| {
            let Shape::Curved { radius } = slot.shape else {
                return None;
            };
            let image = images
                .iter()
                .find(|image| image.id == slot.id && image.shown)?;
            Some(CylinderLayer {
                swapchain: &image.swapchain,
                image_size: image.size,
                pose: slot.pose,
                radius,
                size: slot.size().map(|dp| dp * METERS_PER_DP),
            })
        })
        .collect()
}

// Sizes the composition's view to hold every panel's part of the atlas.
fn fit_atlas(guest: &mut GuestView, space: &Space) -> Result<(), String> {
    let needed = space
        .atlas_size()
        .map(|dp| (dp * DENSITY).ceil() as u32 + 2);
    if guest.pixel_size() != (needed[0], needed[1]) {
        guest
            .resize(needed[0], needed[1], DENSITY)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
