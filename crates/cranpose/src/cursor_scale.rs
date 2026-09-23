//! How big a custom cursor image has to be when it reaches the platform, so it
//! appears the size [`CustomCursorSize`] asks for everywhere.
//!
//! Platforms disagree about two things. macOS takes an image's pixels as
//! points and enlarges them by the display density and by the person's
//! pointer size itself. Windows, X11 and Wayland take them as physical pixels
//! and enlarge them by neither. Each backend states what its platform does in
//! a [`CursorSurface`], and [`image_scale`] says what is left for the app to do.

use crate::CustomCursorSize;

/// What the platform under a window does to a custom cursor image by itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorSurface {
    /// How much the person has enlarged the system pointer; 1 when they have
    /// not.
    pub(crate) pointer_scale: f64,
    /// The window's physical pixels per logical point.
    pub(crate) density: f64,
    /// The platform reads an image's pixels as points and enlarges them by
    /// the density itself.
    pub(crate) image_in_points: bool,
    /// The platform enlarges a custom image by the pointer scale itself.
    pub(crate) enlarges_custom_images: bool,
}

/// A factor to scale by: 1 for anything missing, below 1 or not a number.
pub(crate) fn usable_scale(raw: f64) -> f64 {
    if raw.is_finite() && raw > 1.0 {
        raw
    } else {
        1.0
    }
}

fn usable_density(raw: f64) -> f64 {
    if raw.is_finite() && raw > 0.0 {
        raw
    } else {
        1.0
    }
}

/// How much a custom cursor image drawn one pixel to a point has to be scaled
/// before it reaches `surface`, for it to appear the size `size` asks for: the
/// drawn size in points, enlarged by the pointer scale when it follows the
/// system. 1 hands it over as drawn.
pub(crate) fn image_scale(size: CustomCursorSize, surface: CursorSurface) -> f64 {
    let pointer = usable_scale(surface.pointer_scale);
    let density = usable_density(surface.density);
    let wanted = density
        * match size {
            CustomCursorSize::FollowSystem => pointer,
            CustomCursorSize::AsDrawn => 1.0,
        };
    let given = if surface.image_in_points {
        density
    } else {
        1.0
    } * if surface.enlarges_custom_images {
        pointer
    } else {
        1.0
    };
    wanted / given
}

/// Whether `factor` changes the image at all.
pub(crate) fn rescales(factor: f64) -> bool {
    (factor - 1.0).abs() > 1e-3
}

/// The pointer scale an X11 or Wayland session asks for: its `XCURSOR_SIZE`
/// over the 24 pixels cursor themes are drawn at, the size winit's own themed
/// cursors take.
#[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
pub(crate) fn xcursor_scale(xcursor_size: Option<&str>) -> f64 {
    xcursor_size
        .and_then(|size| size.trim().parse::<f64>().ok())
        .map_or(1.0, |size| usable_scale(size / 24.0))
}

/// The pointer scale Windows asks for: its `CursorBaseSize` over the 32
/// pixels of the standard cursors.
#[cfg(any(test, target_os = "windows"))]
pub(crate) fn windows_cursor_scale(cursor_base_size: Option<u32>) -> f64 {
    cursor_base_size.map_or(1.0, |size| usable_scale(f64::from(size) / 32.0))
}

/// `pixels`, RGBA `width` by `height`, scaled by `factor` nearest-neighbour,
/// so a cursor drawn as pixel art keeps hard pixel edges, with its `hotspot`
/// moved to the same place on the scaled image.
pub(crate) fn scaled_image(
    pixels: &[u8],
    width: u32,
    height: u32,
    hotspot: (u32, u32),
    factor: f64,
) -> (Vec<u8>, u32, u32, (u32, u32)) {
    let scale = |length: u32| ((f64::from(length) * factor).round() as u32).max(1);
    let (scaled_width, scaled_height) = (scale(width), scale(height));
    let source = |at: u32, length: u32| {
        (((f64::from(at) + 0.5) / factor) as u32).min(length.saturating_sub(1))
    };
    let mut scaled = Vec::with_capacity(scaled_width as usize * scaled_height as usize * 4);
    for y in 0..scaled_height {
        let row = source(y, height) as usize * width as usize;
        for x in 0..scaled_width {
            let at = (row + source(x, width) as usize) * 4;
            scaled.extend_from_slice(&pixels[at..at + 4]);
        }
    }
    let place =
        |at: u32, length: u32| ((f64::from(at) * factor) as u32).min(length.saturating_sub(1));
    let hotspot = (
        place(hotspot.0, scaled_width),
        place(hotspot.1, scaled_height),
    );
    (scaled, scaled_width, scaled_height, hotspot)
}

#[cfg(test)]
#[path = "tests/cursor_scale_tests.rs"]
mod tests;
