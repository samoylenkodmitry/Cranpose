//! The format of a swapchain's images.

use openxr as xr;

use super::{Api, Graphics};

// A swapchain format in the graphics API's number and the wgpu format that draws into it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SwapchainFormat {
    pub(crate) api: <Api as xr::Graphics>::Format,
    pub(crate) wgpu: wgpu::TextureFormat,
}

impl SwapchainFormat {
    // The first of the API's formats that the runtime offers, which is an sRGB one if it offers
    // any.
    pub(crate) fn choose(session: &xr::Session<Api>) -> Result<Self, String> {
        let offered = session
            .enumerate_swapchain_formats()
            .map_err(|error| format!("OpenXR swapchain formats: {error}"))?;
        log::info!("cranpose xr: the runtime offers swapchain formats {offered:x?}");
        let format = Graphics::FORMATS
            .into_iter()
            .find(|format| offered.contains(&format.api))
            .ok_or_else(|| format!("no swapchain format Cranpose draws: {offered:x?}"))?;
        log::info!("cranpose xr: swapchain format {:?}", format.wgpu);
        if !format.wgpu.is_srgb() {
            log::warn!("cranpose xr: no sRGB swapchains; colors show too dark");
        }
        Ok(format)
    }

    // The depth and stencil format that the scenes draw against, if the runtime offers it.
    pub(crate) fn choose_depth(session: &xr::Session<Api>) -> Option<Self> {
        let offered = session.enumerate_swapchain_formats().ok()?;
        let format = Graphics::DEPTH_FORMAT.filter(|format| offered.contains(&format.api));
        log::info!(
            "cranpose xr: depth swapchain format {:?}",
            format.map(|format| format.wgpu)
        );
        format
    }
}
