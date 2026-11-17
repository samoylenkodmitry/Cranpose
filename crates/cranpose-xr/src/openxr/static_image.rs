//! A swapchain whose one image is written once, for a picture that never changes.

use cranpose::GpuContext;
use openxr as xr;

use super::{gl_context::wrap_texture, session::XrSession};

// Makes a static swapchain of `size` pixels and writes `pixels` into it: rows of RGBA bytes in
// `formats`, the GL format of the swapchain and the wgpu format that writes it.
pub(crate) fn static_image(
    xr_session: &XrSession,
    gpu: &GpuContext,
    formats: (u32, wgpu::TextureFormat),
    size: (u32, u32),
    pixels: &[u8],
) -> Result<xr::Swapchain<xr::OpenGlEs>, String> {
    let mut swapchain = xr_session
        .session
        .create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::STATIC_IMAGE,
            usage_flags: xr::SwapchainUsageFlags::TRANSFER_DST | xr::SwapchainUsageFlags::SAMPLED,
            format: formats.0,
            sample_count: 1,
            width: size.0,
            height: size.1,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        })
        .map_err(|error| format!("OpenXR static swapchain: {error}"))?;
    let names = swapchain
        .enumerate_images()
        .map_err(|error| format!("OpenXR static swapchain images: {error}"))?;
    let name = *names.first().ok_or("the static swapchain has no image")?;
    let texture = wrap_texture(
        &gpu.device,
        name,
        size,
        formats.1,
        wgpu::TextureUsages::COPY_DST,
    )?;
    swapchain
        .acquire_image()
        .map_err(|error| format!("OpenXR static acquire: {error}"))?;
    swapchain
        .wait_image(xr::Duration::INFINITE)
        .map_err(|error| format!("OpenXR static wait image: {error}"))?;
    gpu.queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.0 * 4),
            rows_per_image: Some(size.1),
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([]);
    // The image is written once, so the write must end before the compositor reads it
    if let Err(error) = gpu.device.poll(wgpu::PollType::wait_indefinitely()) {
        log::warn!("cranpose xr: waiting for a static image: {error}");
    }
    swapchain
        .release_image()
        .map_err(|error| format!("OpenXR static release: {error}"))?;
    Ok(swapchain)
}

// An RGBA pixel of `color` in linear light at `alpha`, premultiplied and sRGB encoded, as the
// compositor blends the layers.
pub(crate) fn premultiplied(color: [f32; 3], alpha: f32) -> [u8; 4] {
    let [r, g, b] = color.map(|channel| encode(channel * alpha));
    [r, g, b, (alpha * 255.0).round() as u8]
}

// The sRGB byte of a linear light value from 0 to 1.
fn encode(linear: f32) -> u8 {
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}
