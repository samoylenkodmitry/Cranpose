//! The mouse pointer's appearance: the shape a platform draws under the
//! pointing device while it hovers a region of the UI.
//!
//! A [`PointerIcon`] is either one of the standard shapes every windowing
//! system and browser already knows ([`CursorIcon`], the CSS cursor
//! vocabulary) or a [`CustomPointerIcon`] the application draws itself from an
//! [`ImageBitmap`] and a hotspot. Applications attach one to a region with
//! `Modifier::pointer_icon`; the shell resolves the topmost hovered region's
//! icon and the platform layer applies it to the window (winit on desktop, the
//! canvas's CSS `cursor` on the web). Platforms with no pointing device —
//! Android and iOS — ignore it.

#[doc(inline)]
pub use cursor_icon::CursorIcon;

use crate::ImageBitmap;

/// The largest custom pointer icon a platform is asked to draw, in pixels.
///
/// Windowing systems reject or silently drop oversized cursors (browsers
/// commonly cap at 128x128), so a bitmap wider or taller than this is refused
/// when the icon is built rather than at the point where it would fail to
/// appear.
pub const MAX_POINTER_ICON_SIZE: u32 = 128;

/// Errors returned while constructing a [`CustomPointerIcon`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PointerIconError {
    /// The bitmap is larger than [`MAX_POINTER_ICON_SIZE`] in one dimension.
    #[error(
        "pointer icon is {width}x{height}, larger than the {MAX_POINTER_ICON_SIZE}px platform limit"
    )]
    TooLarge {
        /// The rejected bitmap's width in pixels.
        width: u32,
        /// The rejected bitmap's height in pixels.
        height: u32,
    },
    /// The hotspot lies outside the bitmap.
    #[error("pointer icon hotspot ({x},{y}) lies outside its {width}x{height} bitmap")]
    HotspotOutsideBitmap {
        /// The rejected hotspot's x coordinate.
        x: u32,
        /// The rejected hotspot's y coordinate.
        y: u32,
        /// The bitmap's width in pixels.
        width: u32,
        /// The bitmap's height in pixels.
        height: u32,
    },
}

/// An application-drawn pointer shape: RGBA pixels plus the hotspot, the pixel
/// inside the image that sits exactly on the pointer's position.
///
/// The alpha channel is **not** premultiplied, which is what both winit and the
/// browser expect of cursor images.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CustomPointerIcon {
    image: ImageBitmap,
    hotspot_x: u32,
    hotspot_y: u32,
}

impl CustomPointerIcon {
    /// Builds a custom pointer icon from `image`, with its hotspot at
    /// (`hotspot_x`, `hotspot_y`) pixels from the image's top-left corner.
    pub fn new(
        image: ImageBitmap,
        hotspot_x: u32,
        hotspot_y: u32,
    ) -> Result<Self, PointerIconError> {
        let (width, height) = (image.width(), image.height());
        if width > MAX_POINTER_ICON_SIZE || height > MAX_POINTER_ICON_SIZE {
            return Err(PointerIconError::TooLarge { width, height });
        }
        if hotspot_x >= width || hotspot_y >= height {
            return Err(PointerIconError::HotspotOutsideBitmap {
                x: hotspot_x,
                y: hotspot_y,
                width,
                height,
            });
        }
        Ok(Self {
            image,
            hotspot_x,
            hotspot_y,
        })
    }

    /// The icon's pixels, tightly packed RGBA8 with straight (not
    /// premultiplied) alpha.
    pub fn image(&self) -> &ImageBitmap {
        &self.image
    }

    /// The hotspot's x offset from the image's left edge, in pixels.
    pub fn hotspot_x(&self) -> u32 {
        self.hotspot_x
    }

    /// The hotspot's y offset from the image's top edge, in pixels.
    pub fn hotspot_y(&self) -> u32 {
        self.hotspot_y
    }

    /// A stable identity derived from the pixels and the hotspot. Platform
    /// backends key their per-window cursor caches on it so an icon that
    /// reappears across frames is uploaded to the windowing system once.
    pub fn id(&self) -> u64 {
        self.image.id().rotate_left(17) ^ ((self.hotspot_x as u64) << 32 | self.hotspot_y as u64)
    }
}

/// The pointer's appearance over a region of the UI.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PointerIcon {
    /// One of the standard shapes the platform already draws, named by the CSS
    /// cursor vocabulary.
    System(CursorIcon),
    /// A shape the application draws itself.
    Custom(CustomPointerIcon),
}

impl PointerIcon {
    /// The platform's default pointer, usually an arrow.
    pub const DEFAULT: Self = Self::System(CursorIcon::Default);

    /// The hand shown over something that can be clicked.
    pub const POINTER: Self = Self::System(CursorIcon::Pointer);

    /// The I-beam shown over selectable text.
    pub const TEXT: Self = Self::System(CursorIcon::Text);

    /// Builds a custom icon from `image` with its hotspot at (`hotspot_x`,
    /// `hotspot_y`).
    pub fn custom(
        image: ImageBitmap,
        hotspot_x: u32,
        hotspot_y: u32,
    ) -> Result<Self, PointerIconError> {
        CustomPointerIcon::new(image, hotspot_x, hotspot_y).map(Self::Custom)
    }

    /// The CSS `cursor` keyword for a standard shape, or `None` for a custom
    /// one, which a browser names with a `url()` instead.
    pub fn css_keyword(&self) -> Option<&'static str> {
        match self {
            Self::System(icon) => Some(icon.name()),
            Self::Custom(_) => None,
        }
    }
}

impl Default for PointerIcon {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<CursorIcon> for PointerIcon {
    fn from(icon: CursorIcon) -> Self {
        Self::System(icon)
    }
}

impl From<CustomPointerIcon> for PointerIcon {
    fn from(icon: CustomPointerIcon) -> Self {
        Self::Custom(icon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(width: u32, height: u32) -> ImageBitmap {
        ImageBitmap::from_rgba8(width, height, vec![255; (width * height * 4) as usize])
            .expect("test bitmap")
    }

    #[test]
    fn custom_icon_keeps_image_and_hotspot() {
        let icon = CustomPointerIcon::new(bitmap(8, 8), 3, 5).expect("icon");
        assert_eq!(icon.image().width(), 8);
        assert_eq!(icon.hotspot_x(), 3);
        assert_eq!(icon.hotspot_y(), 5);
    }

    #[test]
    fn custom_icon_rejects_oversized_bitmaps() {
        let oversized = MAX_POINTER_ICON_SIZE + 1;
        assert_eq!(
            CustomPointerIcon::new(bitmap(oversized, 1), 0, 0),
            Err(PointerIconError::TooLarge {
                width: oversized,
                height: 1,
            })
        );
    }

    #[test]
    fn custom_icon_rejects_a_hotspot_outside_the_bitmap() {
        assert_eq!(
            CustomPointerIcon::new(bitmap(4, 4), 4, 0),
            Err(PointerIconError::HotspotOutsideBitmap {
                x: 4,
                y: 0,
                width: 4,
                height: 4,
            })
        );
    }

    #[test]
    fn custom_icon_accepts_the_bitmap_size_limit_and_its_last_pixel() {
        let size = MAX_POINTER_ICON_SIZE;
        CustomPointerIcon::new(bitmap(size, size), size - 1, size - 1)
            .expect("the limit itself is allowed");
    }

    #[test]
    fn ids_separate_pixels_and_hotspots() {
        let a = CustomPointerIcon::new(bitmap(8, 8), 0, 0).expect("icon");
        let same = CustomPointerIcon::new(bitmap(8, 8), 0, 0).expect("icon");
        let moved_hotspot = CustomPointerIcon::new(bitmap(8, 8), 1, 0).expect("icon");
        let other_pixels = CustomPointerIcon::new(bitmap(4, 4), 0, 0).expect("icon");

        assert_eq!(a.id(), same.id());
        assert_ne!(a.id(), moved_hotspot.id());
        assert_ne!(a.id(), other_pixels.id());
    }

    #[test]
    fn system_icons_name_their_css_keyword() {
        assert_eq!(PointerIcon::DEFAULT.css_keyword(), Some("default"));
        assert_eq!(PointerIcon::POINTER.css_keyword(), Some("pointer"));
        assert_eq!(
            PointerIcon::System(CursorIcon::NotAllowed).css_keyword(),
            Some("not-allowed")
        );
    }

    #[test]
    fn custom_icons_have_no_css_keyword() {
        let icon = PointerIcon::custom(bitmap(8, 8), 0, 0).expect("icon");
        assert_eq!(icon.css_keyword(), None);
    }

    #[test]
    fn the_default_icon_is_the_platform_arrow() {
        assert_eq!(PointerIcon::default(), PointerIcon::DEFAULT);
        assert_eq!(PointerIcon::from(CursorIcon::Default), PointerIcon::DEFAULT);
    }
}
