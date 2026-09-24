use super::*;

fn frame(width: u32, height: u32, fill: u8) -> Vec<u8> {
    vec![fill; (width * height * 4) as usize]
}

fn paint(pixels: &mut [u8], width: u32, x: u32, y: u32) {
    let offset = ((y * width + x) * 4) as usize;
    pixels[offset..offset + 4].copy_from_slice(&[1, 2, 3, 255]);
}

#[test]
fn rows_are_padded_to_the_copy_alignment() {
    assert_eq!(padded_row_bytes(64), 256);
    assert_eq!(padded_row_bytes(65), 512);
    assert_eq!(padded_row_bytes(1), 256);
}

#[test]
fn padding_is_stripped_from_every_row() {
    let padded = [1, 2, 0, 0, 3, 4, 0, 0];
    let mut pixels = vec![9; 10];
    copy_unpadded_rows(&padded, 4, 2, &mut pixels);

    assert_eq!(pixels, vec![1, 2, 3, 4]);
}

#[test]
fn identical_frames_have_no_changed_rect() {
    let before = frame(8, 6, 7);
    assert_eq!(changed_rect(&before, &before.clone(), 8, 6), None);
}

#[test]
fn a_frame_of_another_size_changes_entirely() {
    assert_eq!(
        changed_rect(&[], &frame(8, 6, 0), 8, 6),
        Some(PixelRect::full(8, 6))
    );
}

#[test]
fn one_changed_pixel_is_its_own_rect() {
    let before = frame(8, 6, 0);
    let mut after = before.clone();
    paint(&mut after, 8, 5, 3);

    assert_eq!(
        changed_rect(&before, &after, 8, 6),
        Some(PixelRect {
            x: 5,
            y: 3,
            width: 1,
            height: 1,
        })
    );
}

#[test]
fn scattered_changes_are_bounded_by_one_rect() {
    let before = frame(10, 10, 0);
    let mut after = before.clone();
    paint(&mut after, 10, 7, 1);
    paint(&mut after, 10, 2, 4);
    paint(&mut after, 10, 4, 8);

    assert_eq!(
        changed_rect(&before, &after, 10, 10),
        Some(PixelRect {
            x: 2,
            y: 1,
            width: 6,
            height: 8,
        })
    );
}
