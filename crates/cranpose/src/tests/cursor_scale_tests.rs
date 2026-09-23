use super::*;

const MAC: CursorSurface = CursorSurface {
    pointer_scale: 4.0,
    density: 2.0,
    image_in_points: true,
    enlarges_custom_images: true,
};

const WINDOWS: CursorSurface = CursorSurface {
    pointer_scale: 2.0,
    density: 1.5,
    image_in_points: false,
    enlarges_custom_images: false,
};

#[test]
fn macos_enlarges_a_custom_cursor_itself_so_one_kept_as_drawn_is_handed_over_smaller() {
    assert_eq!(image_scale(CustomCursorSize::AsDrawn, MAC), 0.25);
    assert_eq!(
        image_scale(CustomCursorSize::FollowSystem, MAC),
        1.0,
        "macOS already grows it with the pointer and the display"
    );
}

#[test]
fn windows_and_linux_enlarge_nothing_so_the_app_scales_for_density_and_pointer() {
    assert_eq!(image_scale(CustomCursorSize::AsDrawn, WINDOWS), 1.5);
    assert_eq!(image_scale(CustomCursorSize::FollowSystem, WINDOWS), 3.0);
}

#[test]
fn an_unscaled_display_with_the_standard_pointer_leaves_every_cursor_as_drawn() {
    for surface in [
        CursorSurface {
            pointer_scale: 1.0,
            density: 1.0,
            ..MAC
        },
        CursorSurface {
            pointer_scale: 1.0,
            density: 1.0,
            ..WINDOWS
        },
    ] {
        for size in [CustomCursorSize::AsDrawn, CustomCursorSize::FollowSystem] {
            assert!(
                !rescales(image_scale(size, surface)),
                "{size:?} {surface:?}"
            );
        }
    }
}

#[test]
fn a_setting_never_made_enlarges_nothing() {
    assert_eq!(usable_scale(0.0), 1.0, "an unset preference reads as 0");
    assert_eq!(usable_scale(0.5), 1.0);
    assert_eq!(usable_scale(f64::NAN), 1.0);
    assert_eq!(usable_scale(2.5), 2.5);
    let broken = CursorSurface {
        pointer_scale: f64::NAN,
        density: 0.0,
        ..WINDOWS
    };
    assert_eq!(image_scale(CustomCursorSize::FollowSystem, broken), 1.0);
}

#[test]
fn the_pointer_scale_is_read_from_each_desktops_own_setting() {
    assert_eq!(xcursor_scale(Some("48")), 2.0);
    assert_eq!(xcursor_scale(Some(" 24 ")), 1.0);
    assert_eq!(xcursor_scale(Some("big")), 1.0);
    assert_eq!(xcursor_scale(None), 1.0);
    assert_eq!(windows_cursor_scale(Some(64)), 2.0);
    assert_eq!(windows_cursor_scale(Some(32)), 1.0);
    assert_eq!(windows_cursor_scale(None), 1.0);
}

#[test]
fn a_scaled_cursor_keeps_hard_pixel_edges_and_its_hotspot() {
    // A 2x1 cursor: red, then blue, pointing from its second pixel.
    let pixels = [255, 0, 0, 255, 0, 0, 255, 255];
    let (scaled, width, height, hotspot) = scaled_image(&pixels, 2, 1, (1, 0), 2.0);

    assert_eq!((width, height), (4, 2));
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let rows: Vec<&[u8]> = scaled.chunks(16).collect();
    for row in rows {
        assert_eq!(&row[0..4], red);
        assert_eq!(&row[4..8], red);
        assert_eq!(&row[8..12], blue);
        assert_eq!(&row[12..16], blue);
    }
    assert_eq!(hotspot, (2, 0), "the second pixel starts at 2 once doubled");

    let (_, width, height, hotspot) = scaled_image(&pixels, 2, 1, (1, 0), 1.5);
    assert_eq!((width, height), (3, 2));
    assert!(hotspot.0 < width && hotspot.1 < height);
}
