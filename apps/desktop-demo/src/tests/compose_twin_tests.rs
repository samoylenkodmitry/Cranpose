use crate::test_screens::compose_twin::{
    twin_stray_pixels,
    TwinTolerance::{Edges, Exact},
    MATRIX_FRAMES, MATRIX_GRID, TWIN_FRAME_HEIGHT, TWIN_FRAME_WIDTH, TWIN_STRAY_DELTA,
};

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
    assert_eq!(twin_stray_pixels(&frame, &frame, WIDTH, Edges), Some(0));
}

#[test]
fn antialiasing_on_an_edge_does_not_stray() {
    let reference = frame_with_box(2);
    let mut actual = reference.clone();
    let edge = (3 * WIDTH + 1) * 4;
    actual[edge] = DARK[0] + TWIN_STRAY_DELTA * 3;
    assert_eq!(
        twin_stray_pixels(&reference, &actual, WIDTH, Edges),
        Some(0)
    );
}

#[test]
fn a_difference_away_from_edges_strays() {
    let reference = frame_with_box(2);
    let mut actual = reference.clone();
    let interior = (3 * WIDTH + 4) * 4;
    actual[interior] = 0;
    assert_eq!(
        twin_stray_pixels(&reference, &actual, WIDTH, Edges),
        Some(1)
    );
}

#[test]
fn a_box_moved_by_two_pixels_strays_beside_its_edges() {
    let reference = frame_with_box(2);
    let actual = frame_with_box(4);
    assert_eq!(
        twin_stray_pixels(&reference, &actual, WIDTH, Edges),
        Some(2 * HEIGHT)
    );
}

#[test]
fn frames_of_different_sizes_do_not_compare() {
    let frame = frame_with_box(2);
    assert_eq!(twin_stray_pixels(&frame, &frame[4..], WIDTH, Edges), None);
    assert_eq!(twin_stray_pixels(&frame, &frame, 0, Edges), None);
}

#[test]
fn a_box_moved_by_one_pixel_strays_only_in_an_exact_comparison() {
    let reference = frame_with_box(2);
    let actual = frame_with_box(3);
    assert_eq!(
        twin_stray_pixels(&reference, &actual, WIDTH, Edges),
        Some(0)
    );
    assert_eq!(
        twin_stray_pixels(&reference, &actual, WIDTH, Exact),
        Some(2 * HEIGHT)
    );
}

#[test]
fn matrix_cells_tile_the_frame_from_its_origin() {
    let (x, y, width, height) = MATRIX_GRID.cell(0, 1.0);
    assert_eq!((x, y), (MATRIX_GRID.origin, MATRIX_GRID.origin));
    assert_eq!(MATRIX_GRID.cell(1, 1.0), (x + width, y, width, height));
    let columns = MATRIX_GRID.columns as usize;
    assert_eq!(
        MATRIX_GRID.cell(columns, 1.0),
        (x, y + height, width, height)
    );
}

#[test]
fn matrix_cells_round_to_device_pixels_as_layout_does() {
    // At 2.625 the 4 point origin is 10.5 pixels, which rounds up to 11,
    // and a 128 point cell is exactly 336.
    let (x, y, width, _) = MATRIX_GRID.cell(1, 2.625);
    assert_eq!((x, y, width), (11 + 336, 11, 336));
}

#[test]
fn every_matrix_cell_lies_inside_the_frame() {
    for frame in &MATRIX_FRAMES {
        assert!(!frame.cells.is_empty(), "{} has no cells", frame.name);
        let (x, y, width, height) = MATRIX_GRID.cell(frame.cells.len() - 1, frame.density);
        let bound = |points: u32| (points as f32 * frame.density) as u32;
        assert!(
            x + width <= bound(TWIN_FRAME_WIDTH) && y + height <= bound(TWIN_FRAME_HEIGHT),
            "{} lays a cell outside the frame",
            frame.name
        );
    }
}
