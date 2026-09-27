use super::*;

fn loose() -> Constraints {
    Constraints {
        min_width: 0.0,
        max_width: 400.0,
        min_height: 0.0,
        max_height: 400.0,
    }
}

#[test]
fn a_small_control_grows_to_the_minimum_and_centres_its_content() {
    let result = minimum_interactive_placement(
        loose(),
        Size {
            width: 24.0,
            height: 20.0,
        },
        1.0,
    );
    assert_eq!((result.size.width, result.size.height), (48.0, 48.0));
    assert_eq!(
        (result.placement_offset_x, result.placement_offset_y),
        (12.0, 14.0)
    );
}

#[test]
fn a_large_control_keeps_its_own_size() {
    let result = minimum_interactive_placement(
        loose(),
        Size {
            width: 120.0,
            height: 56.0,
        },
        1.0,
    );
    assert_eq!((result.size.width, result.size.height), (120.0, 56.0));
    assert_eq!(
        (result.placement_offset_x, result.placement_offset_y),
        (0.0, 0.0)
    );
}

#[test]
fn a_tight_parent_wins_over_the_minimum() {
    let tight = Constraints {
        min_width: 0.0,
        max_width: 30.0,
        min_height: 0.0,
        max_height: 100.0,
    };
    let result = minimum_interactive_placement(
        tight,
        Size {
            width: 24.0,
            height: 24.0,
        },
        1.0,
    );
    assert_eq!((result.size.width, result.size.height), (30.0, 48.0));
    assert_eq!(
        (result.placement_offset_x, result.placement_offset_y),
        (3.0, 12.0)
    );
}

#[test]
fn content_is_centred_on_a_whole_device_pixel() {
    let density = 2.625;
    let result = minimum_interactive_placement(
        loose(),
        Size {
            width: 24.0,
            height: 20.0,
        },
        density,
    );
    // 24 of 48 points leaves 31.5 device pixels on each side, and Kotlin
    // rounds the half up; 20 of 48 leaves 36.75.
    assert!((result.placement_offset_x * density - 32.0).abs() < 1e-3);
    assert!((result.placement_offset_y * density - 37.0).abs() < 1e-3);
}
