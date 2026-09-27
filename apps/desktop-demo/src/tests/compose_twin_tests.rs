use crate::test_screens::compose_twin::{twin_stray_pixels, TWIN_STRAY_DELTA};

const WIDTH: usize = 12;
const HEIGHT: usize = 8;
const DARK: [u8; 4] = [18, 18, 23, 255];
const LIGHT: [u8; 4] = [200, 120, 60, 255];

/// A dark frame with a light box from `left` to the frame's middle.
fn frame_with_box(left: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for _ in 0..HEIGHT {
        for x in 0..WIDTH {
            pixels.extend_from_slice(if (left..WIDTH / 2 + left).contains(&x) {
                &LIGHT
            } else {
                &DARK
            });
        }
    }
    pixels
}

#[test]
fn identical_frames_have_no_stray_pixels() {
    let frame = frame_with_box(2);
    assert_eq!(twin_stray_pixels(&frame, &frame, WIDTH), Some(0));
}

#[test]
fn antialiasing_on_an_edge_does_not_stray() {
    let reference = frame_with_box(2);
    let mut actual = reference.clone();
    let edge = (3 * WIDTH + 1) * 4;
    actual[edge] = DARK[0] + TWIN_STRAY_DELTA * 3;
    assert_eq!(twin_stray_pixels(&reference, &actual, WIDTH), Some(0));
}

#[test]
fn a_difference_away_from_edges_strays() {
    let reference = frame_with_box(2);
    let mut actual = reference.clone();
    let interior = (3 * WIDTH + 4) * 4;
    actual[interior] = 0;
    assert_eq!(twin_stray_pixels(&reference, &actual, WIDTH), Some(1));
}

#[test]
fn a_box_moved_by_two_pixels_strays_beside_its_edges() {
    let reference = frame_with_box(2);
    let actual = frame_with_box(4);
    assert_eq!(
        twin_stray_pixels(&reference, &actual, WIDTH),
        Some(2 * HEIGHT)
    );
}

#[test]
fn frames_of_different_sizes_do_not_compare() {
    let frame = frame_with_box(2);
    assert_eq!(twin_stray_pixels(&frame, &frame[4..], WIDTH), None);
    assert_eq!(twin_stray_pixels(&frame, &frame, 0), None);
}
