//! How many samples the scene of the volumes takes in each pixel.

use cranpose::{SCENE_DEPTH_FORMAT, space::Multisample};

// The samples for a scene that draws into images of `color`: four where the device takes four
// samples in the scene's colour format and in its depth format, one elsewhere. The renderer draws
// the scene in the non-sRGB form of the image's format, or in `Rgba8Unorm` for a format that it
// has no such form of. `takes_four` tells whether the device takes four samples in a format.
pub(crate) fn volume_samples(
    takes_four: impl Fn(wgpu::TextureFormat) -> bool,
    color: wgpu::TextureFormat,
) -> Multisample {
    let drawn = match color.remove_srgb_suffix() {
        format @ (wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm) => format,
        _ => wgpu::TextureFormat::Rgba8Unorm,
    };
    if takes_four(drawn) && takes_four(SCENE_DEPTH_FORMAT) {
        Multisample::X4
    } else {
        Multisample::Off
    }
}

#[cfg(test)]
#[path = "tests/volume_samples_tests.rs"]
mod tests;
