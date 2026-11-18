use super::*;

fn floats(window: &CropWindow) -> [f32; 8] {
    let bytes = window.to_bytes();
    let (words, _) = bytes.as_chunks::<4>();
    std::array::from_fn(|index| f32::from_ne_bytes(words[index]))
}

fn assert_close(have: [f32; 8], want: [f32; 8]) {
    for (index, (have, want)) in have.iter().zip(want).enumerate() {
        assert!(
            (have - want).abs() < 1e-6,
            "float {index} is {have}, not {want}"
        );
    }
}

#[test]
fn a_picture_in_padded_rows_samples_inside_the_picture() {
    // A decoder codes 360 rows in a buffer of 368 rows
    let window = CropWindow::new(
        CropRect {
            x: 0,
            y: 0,
            width: 640,
            height: 360,
        },
        (640, 368),
    );
    assert_close(
        floats(&window),
        [
            0.0,
            0.0,
            1.0 / 640.0,
            1.0 / 368.0,
            0.5 / 640.0,
            0.5 / 368.0,
            639.0 / 640.0,
            359.0 / 368.0,
        ],
    );
}

#[test]
fn a_part_that_starts_inside_the_buffer_moves_the_first_pixel() {
    let window = CropWindow::new(
        CropRect {
            x: 16,
            y: 8,
            width: 100,
            height: 50,
        },
        (160, 80),
    );
    assert_close(
        floats(&window),
        [
            16.0 / 160.0,
            8.0 / 80.0,
            1.0 / 160.0,
            1.0 / 80.0,
            16.5 / 160.0,
            8.5 / 80.0,
            115.0 / 160.0,
            57.0 / 80.0,
        ],
    );
}

#[test]
fn a_part_of_one_pixel_keeps_its_lowest_coordinate_below_its_highest() {
    let window = CropWindow::new(
        CropRect {
            x: 3,
            y: 4,
            width: 1,
            height: 1,
        },
        (8, 8),
    );
    let [.., low_x, low_y, high_x, high_y] = floats(&window);
    assert!(low_x <= high_x && low_y <= high_y);
}
