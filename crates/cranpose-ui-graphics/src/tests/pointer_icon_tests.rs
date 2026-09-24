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
