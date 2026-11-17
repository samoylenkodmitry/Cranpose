//! How a surface's picture divides between the eyes.

/// How the picture of a [`SpatialExternalSurface`](crate::SpatialExternalSurface) divides
/// between the eyes. Jetpack XR's `StereoMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StereoMode {
    /// Each eye sees the whole picture.
    #[default]
    Mono,
    /// The left half of the picture goes to the left eye, and the right half to the right eye.
    SideBySide,
    /// The top half of the picture goes to the left eye, and the bottom half to the right eye.
    TopBottom,
}

// The eyes that see a part of a picture.
#[cfg_attr(
    not(any(test, all(feature = "openxr", target_os = "android"))),
    expect(dead_code, reason = "the headset backend uses it")
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Eye {
    Both,
    Left,
    Right,
}

// The part of a picture that some eyes see: the pixel of its top left corner and its width and
// height in pixels.
#[cfg_attr(
    not(any(test, all(feature = "openxr", target_os = "android"))),
    expect(dead_code, reason = "the headset backend uses it")
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EyeView {
    pub(crate) eye: Eye,
    pub(crate) offset: [u32; 2],
    pub(crate) extent: [u32; 2],
}

impl StereoMode {
    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The parts of a picture of `size` pixels, one for each layer that shows the picture. An odd
    // side leaves its last pixel out of both halves.
    pub(crate) fn views(self, size: (u32, u32)) -> impl Iterator<Item = EyeView> {
        let (width, height) = size;
        let (half_width, half_height) = (width / 2, height / 2);
        let views = match self {
            Self::Mono => [
                Some(EyeView {
                    eye: Eye::Both,
                    offset: [0, 0],
                    extent: [width, height],
                }),
                None,
            ],
            Self::SideBySide => [
                Some(EyeView {
                    eye: Eye::Left,
                    offset: [0, 0],
                    extent: [half_width, height],
                }),
                Some(EyeView {
                    eye: Eye::Right,
                    offset: [half_width, 0],
                    extent: [half_width, height],
                }),
            ],
            Self::TopBottom => [
                Some(EyeView {
                    eye: Eye::Left,
                    offset: [0, 0],
                    extent: [width, half_height],
                }),
                Some(EyeView {
                    eye: Eye::Right,
                    offset: [0, half_height],
                    extent: [width, half_height],
                }),
            ],
        };
        views.into_iter().flatten()
    }
}

#[cfg(test)]
#[path = "tests/stereo_mode_tests.rs"]
mod tests;
