//! The surfaces of the surface panels: a surface swapchain for each, which the compositor shows
//! as quad layers.
//!
//! A decoder or a camera writes into the surface from its own thread. The compositor reads the
//! pictures with no copy, so a surface costs the frame loop nothing but its layers.

use openxr as xr;

use crate::{
    METERS_PER_DP,
    openxr::{
        graphics::Api,
        panel_layers::DENSITY,
        session::{LayerSource, QuadLayer, XrSession},
        surface_swapchain,
    },
    space::{PanelKind, PanelSlot, Space},
};

struct SurfaceImage {
    id: u64,
    size: (u32, u32),
    // `None` when the runtime could not make the swapchain, so the panel shows nothing.
    swapchain: Option<xr::Swapchain<Api>>,
}

pub(crate) struct SurfaceLayers {
    images: Vec<SurfaceImage>,
}

impl SurfaceLayers {
    pub(crate) fn new() -> Self {
        Self { images: Vec::new() }
    }

    // Makes the swapchain of each new surface panel and gives the panel its surface. Drops the
    // swapchain of a panel that left.
    pub(crate) fn sync(&mut self, xr_session: &XrSession, space: &Space, panels: &[PanelSlot]) {
        self.images
            .retain(|image| panels.iter().any(|slot| slot.id == image.id));
        for slot in panels
            .iter()
            .filter(|slot| matches!(slot.kind, PanelKind::Surface(_)))
        {
            if self.images.iter().any(|image| image.id == slot.id) {
                continue;
            }
            // The producer picks the size of its buffers. This one is the size that fills the
            // panel at the density of the panels' images.
            let [width, height] = slot.size().map(|dp| ((dp * DENSITY).round() as u32).max(1));
            let swapchain = match surface_swapchain::create(&xr_session.session, (width, height)) {
                Ok((swapchain, surface)) => {
                    space.publish_surface(slot.id, surface);
                    Some(swapchain)
                }
                Err(error) => {
                    log::error!("cranpose xr: no surface for panel {}: {error}", slot.id);
                    None
                }
            };
            self.images.push(SurfaceImage {
                id: slot.id,
                size: (width, height),
                swapchain,
            });
        }
    }

    // The layers of a surface panel: one layer for a mono panel and one for each eye of a stereo
    // panel. Another kind of panel has none.
    pub(crate) fn layers<'a>(
        &'a self,
        slot: &'a PanelSlot,
    ) -> impl Iterator<Item = QuadLayer<'a>> + use<'a> {
        let shown = match slot.kind {
            PanelKind::Surface(stereo_mode) => self
                .images
                .iter()
                .find(|image| image.id == slot.id)
                .and_then(|image| Some((image.swapchain.as_ref()?, image.size, stereo_mode))),
            PanelKind::Content | PanelKind::Volume { .. } => None,
        };
        shown
            .into_iter()
            .flat_map(move |(swapchain, size, stereo_mode)| {
                stereo_mode.views(size).map(move |view| QuadLayer {
                    swapchain,
                    source: LayerSource::Surface(view),
                    pose: slot.pose,
                    size: slot.size().map(|dp| dp * METERS_PER_DP),
                })
            })
    }
}
